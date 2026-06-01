//! OSC notification parsing + byte-exact sequence builders (P5a core).
//!
//! Parity is pinned to cmux SHA `c4911439e3e99784bd5d6379096f315034a5259c`
//! (hard fork — NOT cmux `main`). cmux delegates the actual VT parse to ghostty's
//! native parser; the de-facto wire formats it accepts (and the `cmux notify`
//! docs/`notify_probe.sh` emit) are:
//!
//!   - **OSC 9**  (iTerm2):  `ESC ] 9 ; <body> BEL` — body-only, title defaults
//!     to the app/pane name. cmux's iTerm2 OSC 9 path treats the whole payload as
//!     the notification body.
//!   - **OSC 777** (rxvt):   `ESC ] 777 ; notify ; <title> ; <body> BEL` — the
//!     leading `notify` selector, then title, then body (body may contain `;`).
//!   - **OSC 99** (kitty):   `ESC ] 99 ; <key=val:...> ; <payload> ST` — keyed
//!     metadata before the final `;`, payload after. `p=title|body|subtitle`
//!     selects which field the payload fills; `d=0` = more chunks follow,
//!     `d=1`/absent = done. Subtitle is OSC-99-only (OSC 9/777 have none).
//!
//! The builders ([`build_osc9`], [`build_osc777`], [`build_osc99`]) are the
//! handoff to P13's `weft notify` subcommand: P13 wires the CLI entrypoint; the
//! byte-exact format lives here so it is unit-tested in one place.

use super::scanner::RawOsc;

/// A parsed notification, normalized across OSC 9 / 99 / 777 into one shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNotification {
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

impl ParsedNotification {
    fn empty() -> Self {
        ParsedNotification {
            title: String::new(),
            subtitle: String::new(),
            body: String::new(),
        }
    }

    fn body_only(body: impl Into<String>) -> Self {
        ParsedNotification {
            body: body.into(),
            ..ParsedNotification::empty()
        }
    }
}

/// Interpret a raw OSC as a notification. Returns `None` for codes we do not own
/// or payloads that carry no displayable content (e.g. OSC 99 `p=?` queries, or
/// an OSC 777 selector other than `notify`).
pub fn parse_notification(osc: &RawOsc) -> Option<ParsedNotification> {
    match osc.code {
        9 => parse_osc9(&osc.data),
        777 => parse_osc777(&osc.data),
        99 => parse_osc99(&osc.data),
        _ => None,
    }
}

/// OSC 9: the entire payload is the body. Empty payload → nothing to show.
fn parse_osc9(data: &str) -> Option<ParsedNotification> {
    if data.is_empty() {
        return None;
    }
    Some(ParsedNotification::body_only(data))
}

/// OSC 777: `notify ; <title> ; <body>`. Body keeps any further `;`.
fn parse_osc777(data: &str) -> Option<ParsedNotification> {
    let rest = data.strip_prefix("notify;")?;
    let (title, body) = match rest.split_once(';') {
        Some((t, b)) => (t.to_string(), b.to_string()),
        None => (rest.to_string(), String::new()),
    };
    if title.is_empty() && body.is_empty() {
        return None;
    }
    Some(ParsedNotification {
        title,
        subtitle: String::new(),
        body,
    })
}

/// OSC 99 (kitty): `<metadata> ; <payload>`. Metadata is `key=val` pairs joined
/// by `:`; `p=` picks the target field (title / subtitle / body, default body).
fn parse_osc99(data: &str) -> Option<ParsedNotification> {
    let (meta, payload) = data.split_once(';')?;
    let mut field = "body";
    for pair in meta.split(':') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "p" {
                field = v;
            }
        }
    }
    if payload.is_empty() {
        return None;
    }
    let mut n = ParsedNotification::empty();
    match field {
        "title" => n.title = payload.to_string(),
        "subtitle" => n.subtitle = payload.to_string(),
        // "body" and any unknown/unsupported target fall back to the body so a
        // payload is never silently dropped.
        _ => n.body = payload.to_string(),
    }
    Some(n)
}

const ESC: &str = "\x1b";
const BEL: &str = "\x07";

/// Build `ESC ] 9 ; <body> BEL` (iTerm2 body-only). Used by `weft notify` (P13).
pub fn build_osc9(body: &str) -> Vec<u8> {
    format!("{ESC}]9;{body}{BEL}").into_bytes()
}

/// Build `ESC ] 777 ; notify ; <title> ; <body> BEL` (rxvt). Used by `weft
/// notify` (P13) — matches the docs' shell example byte-for-byte.
pub fn build_osc777(title: &str, body: &str) -> Vec<u8> {
    format!("{ESC}]777;notify;{title};{body}{BEL}").into_bytes()
}

/// Build a single-field OSC 99 (kitty) `ESC ] 99 ; p=<field> ; <payload> ST`.
/// `field` is `title` | `subtitle` | `body`; ST terminator is `ESC \`.
pub fn build_osc99(field: &str, payload: &str) -> Vec<u8> {
    format!("{ESC}]99;p={field};{payload}{ESC}\\").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(code: u16, data: &str) -> RawOsc {
        RawOsc {
            code,
            data: data.to_string(),
        }
    }

    // --- parse ---

    #[test]
    fn osc9_payload_is_the_body() {
        let n = parse_notification(&raw(9, "Build finished")).unwrap();
        assert_eq!(n.title, "");
        assert_eq!(n.body, "Build finished");
    }

    #[test]
    fn osc9_empty_payload_is_none() {
        assert!(parse_notification(&raw(9, "")).is_none());
    }

    #[test]
    fn osc777_splits_title_and_body() {
        let n = parse_notification(&raw(777, "notify;Claude Code;Agent needs input")).unwrap();
        assert_eq!(n.title, "Claude Code");
        assert_eq!(n.body, "Agent needs input");
        assert_eq!(n.subtitle, "");
    }

    #[test]
    fn osc777_body_keeps_extra_semicolons() {
        let n = parse_notification(&raw(777, "notify;T;a;b;c")).unwrap();
        assert_eq!(n.title, "T");
        assert_eq!(n.body, "a;b;c");
    }

    #[test]
    fn osc777_title_only_has_empty_body() {
        let n = parse_notification(&raw(777, "notify;JustTitle")).unwrap();
        assert_eq!(n.title, "JustTitle");
        assert_eq!(n.body, "");
    }

    #[test]
    fn osc777_wrong_selector_is_none() {
        assert!(parse_notification(&raw(777, "other;T;B")).is_none());
    }

    #[test]
    fn osc99_default_field_is_body() {
        let n = parse_notification(&raw(99, "i=1:d=1;Hello World")).unwrap();
        assert_eq!(n.body, "Hello World");
        assert_eq!(n.title, "");
    }

    #[test]
    fn osc99_p_title_fills_title() {
        let n = parse_notification(&raw(99, "i=1:d=0:p=title;Build Complete")).unwrap();
        assert_eq!(n.title, "Build Complete");
        assert_eq!(n.body, "");
    }

    #[test]
    fn osc99_p_subtitle_fills_subtitle() {
        let n = parse_notification(&raw(99, "p=subtitle;Project X")).unwrap();
        assert_eq!(n.subtitle, "Project X");
    }

    #[test]
    fn osc99_empty_payload_is_none() {
        assert!(parse_notification(&raw(99, "i=1:d=0;")).is_none());
    }

    #[test]
    fn osc99_unknown_field_falls_back_to_body() {
        let n = parse_notification(&raw(99, "p=weird;text")).unwrap();
        assert_eq!(n.body, "text");
    }

    #[test]
    fn unknown_code_is_none() {
        assert!(parse_notification(&raw(8, "data")).is_none());
        assert!(parse_notification(&raw(7, "/cwd")).is_none());
    }

    // --- byte-exact builders (handoff to P13 `weft notify`) ---

    #[test]
    fn build_osc9_byte_exact() {
        assert_eq!(build_osc9("hi"), b"\x1b]9;hi\x07");
    }

    #[test]
    fn build_osc777_byte_exact() {
        // Matches the notifications doc: printf '\e]777;notify;My Title;Message\a'
        assert_eq!(
            build_osc777("My Title", "Message body here"),
            b"\x1b]777;notify;My Title;Message body here\x07"
        );
    }

    #[test]
    fn build_osc99_byte_exact() {
        assert_eq!(
            build_osc99("title", "Build Complete"),
            b"\x1b]99;p=title;Build Complete\x1b\\"
        );
    }

    #[test]
    fn builders_round_trip_through_the_scanner() {
        use super::super::scanner::OscScanner;
        let mut bytes = build_osc9("alpha");
        bytes.extend(build_osc777("T", "B"));
        bytes.extend(build_osc99("body", "C"));
        let oscs = OscScanner::new().push(&bytes);
        let parsed: Vec<_> = oscs.iter().filter_map(parse_notification).collect();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].body, "alpha");
        assert_eq!(parsed[1].title, "T");
        assert_eq!(parsed[1].body, "B");
        assert_eq!(parsed[2].body, "C");
    }
}
