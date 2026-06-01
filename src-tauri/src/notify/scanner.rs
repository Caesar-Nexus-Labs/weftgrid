//! Raw OSC sequence scanner (P5a core).
//!
//! Extracts `ESC ] <code> ; <data> (BEL | ST)` sequences from an arbitrary
//! terminal byte stream. xterm renders the same bytes in parallel; this scanner
//! is a passive tap that pulls notification OSCs back out (OSC 9 / 99 / 777) and
//! ignores everything else, so it never swallows terminal output.
//!
//! It is **stateful**: a sequence split across two `push` calls (the PTY reader
//! coalesces in 8–16ms windows, so a sequence can straddle a batch boundary) is
//! buffered and completed on the next call. An unterminated OSC that grows past
//! [`MAX_PENDING`] is dropped to bound memory against a hostile/buggy producer.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const OSC_INTRODUCER: u8 = b']';
const ST_FINAL: u8 = b'\\';

/// Cap on a single in-progress OSC payload. Past this we assume no terminator is
/// coming (malformed / hostile) and drop the pending bytes rather than grow
/// unbounded. Real notification payloads are far smaller.
const MAX_PENDING: usize = 8192;

/// A raw OSC sequence pulled from the stream, framing already stripped.
/// `code` is the numeric introducer (9 / 99 / 777); `data` is everything after
/// the first `;` up to the terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOsc {
    pub code: u16,
    pub data: String,
}

/// Scan position within a (possibly partial) stream.
#[derive(Debug, PartialEq, Eq)]
enum State {
    /// Outside any OSC; scanning for `ESC ]`.
    Idle,
    /// Saw a lone `ESC` while idle — may begin `ESC ]`.
    EscArmed,
    /// Inside an OSC body (after `ESC ]`), accumulating until a terminator.
    InBody,
    /// Saw `ESC` inside a body — the next byte decides: `\` completes an ST
    /// terminator, anything else abandons the body.
    BodyEsc,
}

/// Stateful per-stream OSC extractor. One scanner per pane keeps split sequences
/// from interleaving across panes.
#[derive(Debug)]
pub struct OscScanner {
    state: State,
    body: Vec<u8>,
}

impl Default for OscScanner {
    fn default() -> Self {
        OscScanner {
            state: State::Idle,
            body: Vec::new(),
        }
    }
}

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of stream bytes; return every OSC that completed in it.
    /// Incomplete trailing sequences stay buffered for the next call.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<RawOsc> {
        let mut out = Vec::new();
        for &b in bytes {
            match self.state {
                State::Idle => {
                    if b == ESC {
                        self.state = State::EscArmed;
                    }
                }
                State::EscArmed => match b {
                    OSC_INTRODUCER => {
                        self.state = State::InBody;
                        self.body.clear();
                    }
                    // A run of ESCs leaves the last one armed as an introducer.
                    ESC => {}
                    _ => self.state = State::Idle,
                },
                State::InBody => match b {
                    BEL => self.finish(&mut out),
                    ESC => self.state = State::BodyEsc,
                    _ => self.accumulate(b),
                },
                State::BodyEsc => match b {
                    ST_FINAL => self.finish(&mut out),
                    // Bare ESC inside a body was malformed framing; treat this ESC
                    // as the start of a possible new introducer.
                    ESC => {
                        self.body.clear();
                        self.state = State::EscArmed;
                    }
                    _ => {
                        self.body.clear();
                        self.state = State::Idle;
                    }
                },
            }
        }
        out
    }

    fn accumulate(&mut self, b: u8) {
        self.body.push(b);
        if self.body.len() > MAX_PENDING {
            self.reset();
        }
    }

    /// A terminator was reached: parse `code;data` out of the accumulated body.
    fn finish(&mut self, out: &mut Vec<RawOsc>) {
        if let Some(osc) = parse_body(&self.body) {
            out.push(osc);
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.body.clear();
    }
}

/// Split an OSC body (`<code>;<data>`) into a numeric code + remaining data.
/// Returns `None` for a missing `;` or a non-numeric code (not our concern).
fn parse_body(body: &[u8]) -> Option<RawOsc> {
    let text = String::from_utf8_lossy(body);
    let (code_str, data) = text.split_once(';')?;
    let code: u16 = code_str.parse().ok()?;
    Some(RawOsc {
        code,
        data: data.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(input: &[u8]) -> Vec<RawOsc> {
        OscScanner::new().push(input)
    }

    #[test]
    fn extracts_osc9_bel() {
        let got = scan_all(b"\x1b]9;hello world\x07");
        assert_eq!(
            got,
            vec![RawOsc {
                code: 9,
                data: "hello world".into()
            }]
        );
    }

    #[test]
    fn extracts_osc777_with_semicolons_in_data() {
        let got = scan_all(b"\x1b]777;notify;Title;Body text\x07");
        assert_eq!(
            got,
            vec![RawOsc {
                code: 777,
                data: "notify;Title;Body text".into()
            }]
        );
    }

    #[test]
    fn extracts_osc99_st_terminator() {
        let got = scan_all(b"\x1b]99;i=1:d=1;Hi\x1b\\");
        assert_eq!(
            got,
            vec![RawOsc {
                code: 99,
                data: "i=1:d=1;Hi".into()
            }]
        );
    }

    #[test]
    fn osc_embedded_between_normal_output() {
        let got = scan_all(b"prompt$ \x1b]9;ping\x07more text\r\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, "ping");
    }

    #[test]
    fn multiple_sequences_in_one_chunk() {
        let got = scan_all(b"\x1b]9;a\x07\x1b]777;notify;t;b\x07");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].code, 9);
        assert_eq!(got[1].code, 777);
    }

    #[test]
    fn sequence_split_across_two_pushes() {
        let mut s = OscScanner::new();
        assert!(s.push(b"\x1b]777;notify;Build;Don").is_empty());
        let got = s.push(b"e now\x07");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, "notify;Build;Done now");
    }

    #[test]
    fn st_terminator_split_on_the_esc() {
        // ESC of the ST lands at the end of the first push.
        let mut s = OscScanner::new();
        assert!(s.push(b"\x1b]9;hi\x1b").is_empty());
        let got = s.push(b"\\");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, "hi");
    }

    #[test]
    fn partial_sequence_without_terminator_emits_nothing() {
        let mut s = OscScanner::new();
        assert!(s.push(b"\x1b]9;never terminated").is_empty());
    }

    #[test]
    fn malformed_non_numeric_code_is_ignored() {
        let got = scan_all(b"\x1b]oops;data\x07");
        assert!(got.is_empty());
    }

    #[test]
    fn body_without_payload_separator_is_ignored() {
        let got = scan_all(b"\x1b]bare\x07");
        assert!(got.is_empty());
    }

    #[test]
    fn bare_esc_in_body_abandons_then_recovers() {
        // `ESC X` (not `ESC \`) inside a body is malformed; the scanner drops the
        // partial body and a following well-formed OSC still parses.
        let got = scan_all(b"\x1b]9;ab\x1bX\x1b]9;ok\x07");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, "ok");
    }

    #[test]
    fn runaway_payload_is_dropped_then_recovers() {
        let mut s = OscScanner::new();
        let mut huge = b"\x1b]9;".to_vec();
        huge.extend(std::iter::repeat_n(b'x', MAX_PENDING + 10));
        assert!(s.push(&huge).is_empty());
        // Scanner recovered: a fresh, well-formed sequence still parses.
        let got = s.push(b"\x1b]9;ok\x07");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, "ok");
    }
}
