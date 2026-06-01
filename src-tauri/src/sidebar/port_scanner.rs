//! Port scanner (P15b, app-driven exception — behind a default-off toggle).
//!
//! Most sidebar metadata is PUSHED from the `weft` CLI / shell-integration; ports
//! are one of two app-driven exceptions (the other is git poll). Scanning is
//! expensive (spawns `lsof`/`ss`/`netstat`), so it never runs in the basic
//! sidebar — only when `sidebar.scanPorts` is enabled. This module owns the pure
//! PARSING of each platform's tool output plus coalescing several scans into one
//! deduped batch; spawning the process is the caller's job (kept out of here so
//! the parser unit-tests against sample strings with no subprocess).
//!
//! Std-only: no regex/serde — line scanning by hand.

/// A listening TCP port discovered by a scan.
pub type ListeningPort = u16;

/// Which platform tool produced the captured output (selects the parser).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanSource {
    /// macOS / Linux `lsof -nP -iTCP -sTCP:LISTEN`.
    Lsof,
    /// Linux `ss -ltnH` (or `ss -ltn`).
    Ss,
    /// Windows `netstat -ano -p tcp`.
    Netstat,
}

/// Parse one tool's captured stdout into the set of LISTENING ports, sorted +
/// deduped. Unrecognized lines are skipped (header rows, partial output).
pub fn parse_scan(source: ScanSource, output: &str) -> Vec<ListeningPort> {
    let ports = output.lines().filter_map(|line| match source {
        ScanSource::Lsof => parse_lsof_line(line),
        ScanSource::Ss => parse_ss_line(line),
        ScanSource::Netstat => parse_netstat_line(line),
    });
    dedup_sorted(ports.collect())
}

/// Coalesce multiple (possibly overlapping) scans into ONE deduped, sorted batch.
/// The toggle-driven scanner runs several tools / repeats within a debounce
/// window; the sidebar wants a single port list, not N noisy ones.
pub fn coalesce(batches: &[Vec<ListeningPort>]) -> Vec<ListeningPort> {
    dedup_sorted(batches.iter().flatten().copied().collect())
}

/// Gate parsing behind the default-off `sidebar.scanPorts` toggle. Returns `None`
/// when the toggle is off so a caller can't accidentally surface scan results
/// without the user opting in (the spawn itself the caller must also skip — this
/// is the structural reminder at the parse boundary). `enabled` is read from
/// [`super::state::SidebarState::port_scan_enabled`].
pub fn parse_scan_gated(
    enabled: bool,
    source: ScanSource,
    output: &str,
) -> Option<Vec<ListeningPort>> {
    if !enabled {
        return None;
    }
    Some(parse_scan(source, output))
}

fn dedup_sorted(mut ports: Vec<ListeningPort>) -> Vec<ListeningPort> {
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// `lsof` LISTEN row: the port is the trailing `:PORT` of the NAME column, e.g.
/// `node 1234 user 23u IPv4 ... TCP *:3000 (LISTEN)` or `... TCP 127.0.0.1:5432 (LISTEN)`.
fn parse_lsof_line(line: &str) -> Option<ListeningPort> {
    if !line.contains("(LISTEN)") {
        return None;
    }
    // The address token is the field right before "(LISTEN)".
    let addr = line.split_whitespace().rev().nth(1)?;
    port_after_last_colon(addr)
}

/// `ss -ltn` row: state in col 0, local address:port in col 4 (e.g.
/// `LISTEN 0 128 0.0.0.0:8080 0.0.0.0:*`). With `-H` there is no header.
fn parse_ss_line(line: &str) -> Option<ListeningPort> {
    let mut cols = line.split_whitespace();
    let state = cols.next()?;
    if !state.eq_ignore_ascii_case("LISTEN") {
        return None;
    }
    // Local address:port is the 4th column (skip Recv-Q, Send-Q).
    let local = cols.nth(2)?;
    port_after_last_colon(local)
}

/// `netstat -ano -p tcp` row: `TCP 0.0.0.0:135 0.0.0.0:0 LISTENING 1234`.
fn parse_netstat_line(line: &str) -> Option<ListeningPort> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("TCP") || !line.contains("LISTENING") {
        return None;
    }
    let local = trimmed.split_whitespace().nth(1)?;
    port_after_last_colon(local)
}

/// Extract the port after the LAST `:` (handles IPv6 `[::]:port` and `*:port`).
fn port_after_last_colon(addr: &str) -> Option<ListeningPort> {
    let (_, port) = addr.rsplit_once(':')?;
    port.parse::<u16>().ok().filter(|&p| p != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsof_listen_ports_ignoring_other_rows() {
        // Real-ish lsof output: header + a LISTEN row + an ESTABLISHED row.
        let out = "\
COMMAND  PID USER   FD  TYPE DEVICE SIZE/OFF NODE NAME
node    111 me   23u  IPv4  0x00      0t0  TCP *:3000 (LISTEN)
node    111 me   24u  IPv4  0x00      0t0  TCP 127.0.0.1:5432 (LISTEN)
node    111 me   25u  IPv4  0x00      0t0  TCP 10.0.0.1:51000->1.2.3.4:443 (ESTABLISHED)";
        assert_eq!(parse_scan(ScanSource::Lsof, out), vec![3000, 5432]);
    }

    #[test]
    fn parses_ss_listen_ports_with_ipv6_and_header() {
        let out = "\
State  Recv-Q Send-Q Local Address:Port Peer Address:Port
LISTEN 0      128    0.0.0.0:8080       0.0.0.0:*
LISTEN 0      128    [::]:9229          [::]:*
ESTAB  0      0      10.0.0.1:51000     1.2.3.4:443";
        assert_eq!(parse_scan(ScanSource::Ss, out), vec![8080, 9229]);
    }

    #[test]
    fn parses_windows_netstat_listening_ports() {
        let out = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1234
  TCP    127.0.0.1:5173         0.0.0.0:0              LISTENING       5678
  TCP    10.0.0.1:51000         1.2.3.4:443            ESTABLISHED     9012";
        assert_eq!(parse_scan(ScanSource::Netstat, out), vec![135, 5173]);
    }

    #[test]
    fn skips_port_zero_and_malformed_addresses() {
        let out = "x x x x TCP *:0 (LISTEN)\nx x x x TCP not-an-addr (LISTEN)";
        assert!(parse_scan(ScanSource::Lsof, out).is_empty());
    }

    #[test]
    fn coalesce_merges_dedups_and_sorts_multiple_scans() {
        let lsof = parse_scan(ScanSource::Lsof, "x x x x TCP *:3000 (LISTEN)");
        let ss = parse_scan(
            ScanSource::Ss,
            "LISTEN 0 128 0.0.0.0:3000 0.0.0.0:*\nLISTEN 0 128 0.0.0.0:8080 0.0.0.0:*",
        );
        // 3000 appears in both batches → coalesced to one entry; result sorted.
        assert_eq!(coalesce(&[lsof, ss]), vec![3000, 8080]);
    }

    #[test]
    fn empty_or_unrecognized_output_yields_no_ports() {
        assert!(parse_scan(ScanSource::Ss, "").is_empty());
        assert!(parse_scan(ScanSource::Netstat, "garbage\nmore garbage").is_empty());
    }

    #[test]
    fn gated_parse_returns_none_when_toggle_off() {
        // Default-off invariant: a disabled toggle yields no ports regardless of
        // the (real) output, so results never surface without the user opting in.
        let out = "x x x x TCP *:3000 (LISTEN)";
        assert_eq!(parse_scan_gated(false, ScanSource::Lsof, out), None);
        assert_eq!(
            parse_scan_gated(true, ScanSource::Lsof, out),
            Some(vec![3000])
        );
    }
}
