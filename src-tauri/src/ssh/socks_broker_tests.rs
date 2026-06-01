//! Tests for the SOCKS5h broker (P10a). Use in-memory/loopback doubles for the
//! channel opener so the broker is exercised end-to-end without a live SSH host.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::*;

/// Records every `(host, port)` the broker asked to open, and tunnels bytes to an
/// in-memory peer the test drives. Proves the broker forwards the *raw hostname*
/// (socks5h) and never resolves it locally.
struct RecordingOpener {
    seen: Arc<Mutex<Vec<(String, u16)>>>,
    /// bytes the "remote" side will echo-prefix, to verify the pipe direction.
    response: Vec<u8>,
}

impl ChannelOpener for RecordingOpener {
    fn open(&self, host: String, port: u16) -> OpenFuture<'_> {
        self.seen.lock().unwrap().push((host.clone(), port));
        let response = self.response.clone();
        Box::pin(async move {
            // Build a duplex: broker writes into `remote_side`'s read; we preload
            // a response so the client can read it back through the broker.
            let (broker_side, mut remote_side) = tokio::io::duplex(1024);
            tokio::spawn(async move {
                // Echo everything the client sends back, prefixed once with
                // `response`, so the test sees data flow both ways.
                let _ = remote_side.write_all(&response).await;
                let mut buf = [0u8; 256];
                loop {
                    match remote_side.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if remote_side.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(Box::new(broker_side) as TunnelStream)
        })
    }
}

/// Always fails to open — drives the ConnectionRefused reply path.
struct FailingOpener;
impl ChannelOpener for FailingOpener {
    fn open(&self, _host: String, _port: u16) -> OpenFuture<'_> {
        Box::pin(async { Err("no route".to_string()) })
    }
}

/// Perform the SOCKS5 greeting + CONNECT(domain) against a real broker socket and
/// return the client stream positioned right after the success reply.
async fn connect_and_request(stream: &mut TcpStream, host: &str, port: u16) -> Vec<u8> {
    // greeting
    stream.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut method = [0u8; 2];
    stream.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [0x05, 0x00]);

    // CONNECT domain
    let mut req = vec![0x05, 0x01, 0x00, 0x03, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await.unwrap();

    // reply (10 bytes)
    let mut reply = [0u8; 10];
    stream.read_exact(&mut reply).await.unwrap();
    reply.to_vec()
}

#[tokio::test]
async fn rejects_non_loopback_bind() {
    let opener = Arc::new(FailingOpener);
    let addr = "0.0.0.0:0".parse().unwrap();
    let res = SocksBroker::bind_on(addr, opener).await;
    match res {
        Ok(_) => panic!("broker must refuse a non-loopback bind"),
        Err(e) => assert!(e.contains("loopback"), "unexpected error: {e}"),
    }
}

#[tokio::test]
async fn binds_loopback_and_reports_socks5h_url() {
    let opener = Arc::new(FailingOpener);
    let broker = SocksBroker::bind(opener).await.unwrap();
    assert!(broker.local_addr().ip().is_loopback());
    assert!(broker.proxy_url().starts_with("socks5h://127.0.0.1:"));
}

#[tokio::test]
async fn s6_forwards_raw_hostname_to_opener() {
    // S6 hard-gate (broker form): a CONNECT to a hostname must reach the opener
    // as the verbatim hostname + port — NOT a resolved IP. The opener stands in
    // for the SSH direct-tcpip channel where the *remote* resolves the name.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let opener = Arc::new(RecordingOpener {
        seen: seen.clone(),
        response: b"HELLO-FROM-REMOTE".to_vec(),
    });
    let broker = SocksBroker::bind(opener).await.unwrap();

    let mut client = TcpStream::connect(broker.local_addr()).await.unwrap();
    let reply = connect_and_request(&mut client, "only.internal.remote", 443).await;
    assert_eq!(reply[..2], [0x05, 0x00], "expected SOCKS success reply");

    // The opener saw the raw hostname — proof of socks5h (no local resolution).
    let recorded = seen.lock().unwrap().clone();
    assert_eq!(recorded, vec![("only.internal.remote".to_string(), 443)]);
}

#[tokio::test]
async fn pipes_bytes_bidirectionally() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let opener = Arc::new(RecordingOpener {
        seen,
        response: b"REMOTE>".to_vec(),
    });
    let broker = SocksBroker::bind(opener).await.unwrap();

    let mut client = TcpStream::connect(broker.local_addr()).await.unwrap();
    connect_and_request(&mut client, "example.test", 80).await;

    // Remote preloaded "REMOTE>" — client reads it (remote → client direction).
    let mut head = [0u8; 7];
    client.read_exact(&mut head).await.unwrap();
    assert_eq!(&head, b"REMOTE>");

    // Client sends bytes; the echo opener returns them (client → remote → client).
    client.write_all(b"ping123").await.unwrap();
    let mut echo = [0u8; 7];
    client.read_exact(&mut echo).await.unwrap();
    assert_eq!(&echo, b"ping123");
}

#[tokio::test]
async fn open_failure_yields_connection_refused() {
    let opener = Arc::new(FailingOpener);
    let broker = SocksBroker::bind(opener).await.unwrap();

    let mut client = TcpStream::connect(broker.local_addr()).await.unwrap();
    let reply = connect_and_request(&mut client, "nope.remote", 22).await;
    // REP=0x05 ConnectionRefused
    assert_eq!(reply[1], 0x05);
}

#[tokio::test]
async fn swap_opener_keeps_port_stable() {
    // Reconnect contract: replacing the opener must not change the bound port, so
    // P6 overlays keep their proxy_url across a reconnect.
    let first = Arc::new(FailingOpener);
    let broker = SocksBroker::bind(first).await.unwrap();
    let port_before = broker.local_addr().port();

    let seen = Arc::new(Mutex::new(Vec::new()));
    broker.swap_opener(Arc::new(RecordingOpener {
        seen: seen.clone(),
        response: vec![],
    }));
    assert_eq!(broker.local_addr().port(), port_before);

    // New opener is the one now used.
    let mut client = TcpStream::connect(broker.local_addr()).await.unwrap();
    connect_and_request(&mut client, "after.swap", 8443).await;
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec![("after.swap".to_string(), 8443)]
    );
}
