//! SOCKS5 wire protocol — parsing + reply framing (P10a, RFC 1928).
//!
//! **socks5h, not socks5.** The whole SSH-routing feature hinges on the proxy
//! forwarding the *hostname* to the remote for DNS resolution, NOT resolving it
//! locally. So when a client sends an `ATYP=DOMAINNAME` CONNECT, we keep the
//! domain string verbatim ([`TargetAddr::Domain`]) and hand it to the SSH
//! `direct-tcpip` channel — the remote sshd resolves it. We never call a local
//! resolver here. This is the S6 hard-gate invariant; the parser is built so a
//! local DNS lookup is structurally impossible (there is no resolver dependency
//! in this file).
//!
//! Pure async read/write over any `AsyncRead`/`AsyncWrite` so it unit-tests
//! against in-memory duplex streams without a socket or a live SSH host.

use std::fmt;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const SOCKS5_VERSION: u8 = 0x05;
const CMD_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAINNAME: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;
const AUTH_NONE: u8 = 0x00;
const AUTH_NO_ACCEPTABLE: u8 = 0xFF;

/// SOCKS5 reply codes (RFC 1928 §6). Only the ones we emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyCode {
    Succeeded = 0x00,
    GeneralFailure = 0x01,
    ConnectionRefused = 0x05,
    CommandNotSupported = 0x07,
    AddressTypeNotSupported = 0x08,
}

/// A SOCKS5 CONNECT target. `Domain` carries the hostname *unresolved* — that is
/// the socks5h guarantee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetAddr {
    /// Hostname forwarded to the remote for DNS resolution (socks5h).
    Domain(String, u16),
    Ip(std::net::IpAddr, u16),
}

impl TargetAddr {
    /// Host string for `channel_open_direct_tcpip`. For `Domain` this is the raw
    /// hostname (remote resolves it); for `Ip` the literal address.
    pub fn host(&self) -> String {
        match self {
            TargetAddr::Domain(h, _) => h.clone(),
            TargetAddr::Ip(ip, _) => ip.to_string(),
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            TargetAddr::Domain(_, p) | TargetAddr::Ip(_, p) => *p,
        }
    }
}

impl fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host(), self.port())
    }
}

/// Errors during the SOCKS5 negotiation. Carry enough to pick a reply code.
#[derive(Debug)]
pub enum SocksError {
    Io(std::io::Error),
    BadVersion(u8),
    NoAcceptableAuth,
    UnsupportedCommand(u8),
    UnsupportedAddressType(u8),
    BadDomain,
}

impl fmt::Display for SocksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocksError::Io(e) => write!(f, "socks io: {e}"),
            SocksError::BadVersion(v) => write!(f, "unsupported SOCKS version {v:#x}"),
            SocksError::NoAcceptableAuth => write!(f, "no acceptable auth method (need no-auth)"),
            SocksError::UnsupportedCommand(c) => write!(f, "unsupported SOCKS command {c:#x}"),
            SocksError::UnsupportedAddressType(a) => write!(f, "unsupported address type {a:#x}"),
            SocksError::BadDomain => write!(f, "invalid domain name in request"),
        }
    }
}

impl std::error::Error for SocksError {}

impl From<std::io::Error> for SocksError {
    fn from(e: std::io::Error) -> Self {
        SocksError::Io(e)
    }
}

/// Phase 1: method-negotiation. Reads the client greeting and replies selecting
/// the no-auth method (the broker is loopback-only, so unauthenticated SOCKS is
/// acceptable — the OS already gates who can reach 127.0.0.1). Returns `Err` +
/// writes the `0xFF` rejection if the client offers no no-auth method.
pub async fn negotiate_auth<S>(stream: &mut S) -> Result<(), SocksError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let ver = stream.read_u8().await?;
    if ver != SOCKS5_VERSION {
        return Err(SocksError::BadVersion(ver));
    }
    let nmethods = stream.read_u8().await? as usize;
    let mut methods = vec![0u8; nmethods];
    stream.read_exact(&mut methods).await?;

    if methods.contains(&AUTH_NONE) {
        stream.write_all(&[SOCKS5_VERSION, AUTH_NONE]).await?;
        stream.flush().await?;
        Ok(())
    } else {
        stream
            .write_all(&[SOCKS5_VERSION, AUTH_NO_ACCEPTABLE])
            .await?;
        stream.flush().await?;
        Err(SocksError::NoAcceptableAuth)
    }
}

/// Phase 2: read the CONNECT request. **Does not resolve DNS.** A DOMAINNAME is
/// returned as [`TargetAddr::Domain`] with the bytes decoded as UTF-8, untouched.
/// Only `CONNECT` is supported (BIND/UDP-ASSOCIATE are not needed for browser
/// egress).
pub async fn read_connect_request<S>(stream: &mut S) -> Result<TargetAddr, SocksError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let ver = stream.read_u8().await?;
    if ver != SOCKS5_VERSION {
        return Err(SocksError::BadVersion(ver));
    }
    let cmd = stream.read_u8().await?;
    if cmd != CMD_CONNECT {
        return Err(SocksError::UnsupportedCommand(cmd));
    }
    let _rsv = stream.read_u8().await?; // reserved, must be 0x00
    let atyp = stream.read_u8().await?;

    let addr = match atyp {
        ATYP_IPV4 => {
            let mut octets = [0u8; 4];
            stream.read_exact(&mut octets).await?;
            let port = stream.read_u16().await?;
            TargetAddr::Ip(std::net::IpAddr::from(octets), port)
        }
        ATYP_IPV6 => {
            let mut octets = [0u8; 16];
            stream.read_exact(&mut octets).await?;
            let port = stream.read_u16().await?;
            TargetAddr::Ip(std::net::IpAddr::from(octets), port)
        }
        ATYP_DOMAINNAME => {
            let len = stream.read_u8().await? as usize;
            let mut host = vec![0u8; len];
            stream.read_exact(&mut host).await?;
            let port = stream.read_u16().await?;
            // Keep the hostname verbatim — NO local resolution (socks5h).
            let host = String::from_utf8(host).map_err(|_| SocksError::BadDomain)?;
            if host.is_empty() {
                return Err(SocksError::BadDomain);
            }
            TargetAddr::Domain(host, port)
        }
        other => return Err(SocksError::UnsupportedAddressType(other)),
    };
    Ok(addr)
}

/// Write a SOCKS5 reply. On success the bound-address is reported as `0.0.0.0:0`
/// (RFC-permitted: clients ignore it for CONNECT). On failure the matching reply
/// code lets the browser surface a real error instead of hanging.
pub async fn write_reply<S>(stream: &mut S, code: ReplyCode) -> Result<(), SocksError>
where
    S: AsyncWrite + Unpin,
{
    // VER, REP, RSV, ATYP=IPv4, BND.ADDR(0.0.0.0), BND.PORT(0)
    let frame = [
        SOCKS5_VERSION,
        code as u8,
        0x00,
        ATYP_IPV4,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

/// Map a [`SocksError`] from the request phase to the reply code the client
/// should see.
pub fn reply_code_for(err: &SocksError) -> ReplyCode {
    match err {
        SocksError::UnsupportedCommand(_) => ReplyCode::CommandNotSupported,
        SocksError::UnsupportedAddressType(_) => ReplyCode::AddressTypeNotSupported,
        _ => ReplyCode::GeneralFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Run ONLY the auth negotiation against a server task and return the 2-byte
    /// method-selection reply plus the auth result. Used by greeting tests where
    /// no CONNECT request follows (so we must not block reading one).
    async fn negotiate(client_bytes: &[u8]) -> ([u8; 2], Result<(), SocksError>) {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move { negotiate_auth(&mut server).await });
        client.write_all(client_bytes).await.unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        let result = task.await.unwrap();
        (reply, result)
    }

    /// Run the full handshake (greeting + CONNECT) against a server task. Returns
    /// the auth reply, the success/error reply, and the parsed target. The server
    /// runs in its own task so a blocking read can't deadlock the client.
    async fn run_handshake(client_bytes: &[u8]) -> Result<TargetAddr, SocksError> {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move {
            negotiate_auth(&mut server).await?;
            read_connect_request(&mut server).await
        });
        client.write_all(client_bytes).await.unwrap();
        // Drain the auth-selection reply so it doesn't mix with later asserts.
        let mut _method = [0u8; 2];
        let _ = client.read_exact(&mut _method).await;
        task.await.unwrap()
    }

    #[tokio::test]
    async fn greeting_selects_no_auth() {
        // VER=5, NMETHODS=1, METHODS=[0x00]
        let (reply, result) = negotiate(&[0x05, 0x01, 0x00]).await;
        assert_eq!(reply, [0x05, 0x00], "must select no-auth method");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn rejects_when_no_no_auth_offered() {
        // offers only GSSAPI (0x01), no 0x00
        let (reply, result) = negotiate(&[0x05, 0x01, 0x01]).await;
        assert_eq!(reply, [0x05, 0xFF]);
        assert!(matches!(result, Err(SocksError::NoAcceptableAuth)));
    }

    #[tokio::test]
    async fn s6_domainname_is_kept_raw_not_resolved() {
        // The S6 gate in protocol form: a CONNECT to a hostname must come back as
        // Domain("only.internal.remote", 443) — NOT an IP. If anything resolved
        // it locally we'd see an Ip variant (or an error for an unresolvable
        // name). The host is deliberately a name that does not resolve locally.
        let host = b"only.internal.remote";
        let mut req = vec![0x05, 0x01, 0x00]; // greeting: no-auth
        req.extend_from_slice(&[0x05, CMD_CONNECT, 0x00, ATYP_DOMAINNAME]);
        req.push(host.len() as u8);
        req.extend_from_slice(host);
        req.extend_from_slice(&443u16.to_be_bytes());

        let target = run_handshake(&req).await.expect("parse domain connect");
        assert_eq!(
            target,
            TargetAddr::Domain("only.internal.remote".into(), 443),
            "hostname must be preserved verbatim for remote resolution (socks5h)"
        );
        // And the host string handed to direct-tcpip is the raw name.
        assert_eq!(target.host(), "only.internal.remote");
        assert_eq!(target.port(), 443);
    }

    #[tokio::test]
    async fn parses_ipv4_connect() {
        let mut req = vec![0x05, 0x01, 0x00];
        req.extend_from_slice(&[0x05, CMD_CONNECT, 0x00, ATYP_IPV4, 127, 0, 0, 1]);
        req.extend_from_slice(&8080u16.to_be_bytes());
        let target = run_handshake(&req).await;
        assert_eq!(
            target.unwrap(),
            TargetAddr::Ip("127.0.0.1".parse().unwrap(), 8080)
        );
    }

    #[tokio::test]
    async fn parses_ipv6_connect() {
        let mut req = vec![0x05, 0x01, 0x00];
        req.extend_from_slice(&[0x05, CMD_CONNECT, 0x00, ATYP_IPV6]);
        req.extend_from_slice(&[0; 15]);
        req.push(1); // ::1
        req.extend_from_slice(&443u16.to_be_bytes());
        let target = run_handshake(&req).await;
        assert_eq!(target.unwrap(), TargetAddr::Ip("::1".parse().unwrap(), 443));
    }

    #[tokio::test]
    async fn rejects_non_connect_command() {
        // BIND (0x02) is unsupported
        let mut req = vec![0x05, 0x01, 0x00];
        req.extend_from_slice(&[0x05, 0x02, 0x00, ATYP_IPV4, 0, 0, 0, 0]);
        req.extend_from_slice(&0u16.to_be_bytes());
        let target = run_handshake(&req).await;
        assert!(matches!(target, Err(SocksError::UnsupportedCommand(0x02))));
        assert_eq!(
            reply_code_for(&target.unwrap_err()),
            ReplyCode::CommandNotSupported
        );
    }

    #[tokio::test]
    async fn rejects_bad_version() {
        // A bad version in the greeting fails auth negotiation outright. No reply
        // is written (we reject before selecting a method), so drive the server
        // directly rather than waiting for a 2-byte reply.
        let (mut client, mut server) = tokio::io::duplex(64);
        client.write_all(&[0x04, 0x01, 0x00]).await.unwrap();
        let result = negotiate_auth(&mut server).await;
        assert!(matches!(result, Err(SocksError::BadVersion(0x04))));
    }

    #[tokio::test]
    async fn success_reply_frame_is_well_formed() {
        let (mut a, mut b) = tokio::io::duplex(64);
        write_reply(&mut a, ReplyCode::Succeeded).await.unwrap();
        drop(a);
        let mut out = Vec::new();
        b.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, vec![0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
    }
}
