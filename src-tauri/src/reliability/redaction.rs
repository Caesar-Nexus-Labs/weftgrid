//! Secret redaction (P14 Security — project HARD rule: never let a secret value
//! reach disk).
//!
//! [`redact`] rewrites a log line so any recognised secret VALUE is replaced by a
//! `<redacted:KEYNAME>` marker that references the key NAME, not the value (the
//! project rule). It is deliberately CONSERVATIVE: when in doubt it over-redacts
//! rather than leak, because a single missed pattern defeats the whole layer.
//!
//! Three passes, broad to narrow:
//!   1. PEM private-key blocks (`-----BEGIN ... PRIVATE KEY----- ... -----END...`)
//!      — multiline, replaced wholesale.
//!   2. `Authorization:` / `Bearer ` headers — the scheme + credential after them.
//!   3. key=value / `"key": "value"` pairs for a fixed set of sensitive key names
//!      (cookie, token, password, secret, api-key, session id, the P13 RPC token
//!      and the P11 cookie seed all reduce to these names).
//!
//! std-only (no `regex` dep): a single left-to-right byte scan handles the kv pass,
//! since every key name and separator we match is ASCII.

/// Sensitive key names → the canonical name reported in the marker. Matched
/// case-insensitively as a whole word immediately followed (after optional quote/
/// space) by a `:` or `=` separator. Order: longer/more-specific keys first so a
/// single pass picks the most precise name (e.g. `access_token` before `token`).
const SENSITIVE_KEYS: &[(&str, &str)] = &[
    ("set-cookie", "cookie"),
    ("cookie", "cookie"),
    ("access_token", "token"),
    ("refresh_token", "token"),
    ("id_token", "token"),
    ("client_secret", "secret"),
    ("api_key", "api-key"),
    ("apikey", "api-key"),
    ("api-key", "api-key"),
    ("private_key", "private-key"),
    ("session_id", "session"),
    ("sessionid", "session"),
    ("session-token", "session"),
    ("token", "token"),
    ("password", "password"),
    ("passwd", "password"),
    ("secret", "secret"),
];

/// Redact every recognised secret in `input`, referencing the key NAME not value.
pub fn redact(input: &str) -> String {
    let pem = redact_pem_blocks(input);
    let auth = redact_authorization(&pem);
    redact_key_values(&auth)
}

/// Replace any `-----BEGIN ... PRIVATE KEY----- ... -----END ... PRIVATE KEY-----`
/// block (incl. its base64 body and newlines) with a single marker. Conservative:
/// matches any key flavour (RSA/OPENSSH/EC/DSA/PGP) by anchoring on the shared
/// `PRIVATE KEY` text.
fn redact_pem_blocks(input: &str) -> String {
    const BEGIN: &str = "-----BEGIN";
    const END_ANCHOR: &str = "-----END";
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(begin_at) = rest.find(BEGIN) {
        // Only treat it as a private key if the BEGIN line mentions PRIVATE KEY.
        let after_begin = &rest[begin_at..];
        let begin_line_end = after_begin.find('\n').unwrap_or(after_begin.len());
        let begin_line = &after_begin[..begin_line_end];
        if !begin_line.contains("PRIVATE KEY") {
            // Not a private-key header (e.g. a CERTIFICATE) — copy through and move on.
            out.push_str(&rest[..begin_at + BEGIN.len()]);
            rest = &rest[begin_at + BEGIN.len()..];
            continue;
        }
        out.push_str(&rest[..begin_at]);
        // Search for the closing line AFTER the begin line (the begin line itself
        // contains "PRIVATE KEY-----", so anchoring on `-----END` past it is what
        // distinguishes the real terminator).
        match after_begin[begin_line_end..].find(END_ANCHOR) {
            Some(end_rel) => {
                let end_line_start = begin_line_end + end_rel;
                // Consume through the end of the END line so its trailing `-----`
                // goes too, but preserve the newline (and any following text).
                let block_end = after_begin[end_line_start..]
                    .find('\n')
                    .map(|n| end_line_start + n)
                    .unwrap_or(after_begin.len());
                out.push_str("<redacted:ssh-private-key>");
                rest = &after_begin[block_end..];
            }
            None => {
                // No closing marker — redact to end-of-input rather than leak the body.
                out.push_str("<redacted:ssh-private-key>");
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Redact `Authorization: <scheme> <credential>` and standalone `Bearer <token>`.
/// For an `authorization` header we drop the ENTIRE value to end-of-line (scheme +
/// credential); for a bare `Bearer ` we drop the following token only.
fn redact_authorization(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if at_word_start(bytes, i) {
            if let Some(name) = match_keyword(&lower, i, "authorization") {
                // Skip the key, optional quote, spaces, then the `:` separator, and
                // redact the whole value (scheme + credential) up to the line end.
                let j = skip_separators(bytes, i + name);
                out.push_str("authorization: <redacted:authorization>");
                i = consume_to_line_end(bytes, j);
                continue;
            }
            if match_keyword(&lower, i, "bearer").is_some() {
                let j = i + "bearer".len();
                let val_start = skip_spaces(bytes, j);
                if val_start > j {
                    out.push_str("Bearer <redacted:token>");
                    i = consume_value(bytes, val_start);
                    continue;
                }
            }
        }
        out.push(input[i..].chars().next().unwrap());
        i += next_char_len(bytes, i);
    }
    out
}

/// Single pass over the text: when a sensitive key appears as a whole word followed
/// by a `:`/`=` separator, replace its value token with the marker.
fn redact_key_values(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if at_word_start(bytes, i) {
            if let Some((klen, name)) = match_sensitive_key(&lower, i) {
                let sep_start = i + klen;
                let after_sep = skip_separators(bytes, sep_start);
                // Require a real separator (`:` or `=`) so prose like "token list"
                // is not redacted — only "token=..." / "token: ..." is.
                if after_sep > sep_start && had_separator(bytes, sep_start, after_sep) {
                    out.push_str(&input[i..sep_start]);
                    out.push_str(": <redacted:");
                    out.push_str(name);
                    out.push('>');
                    i = consume_value(bytes, after_sep);
                    continue;
                }
            }
        }
        out.push(input[i..].chars().next().unwrap());
        i += next_char_len(bytes, i);
    }
    out
}

/// True when `i` begins a word (preceding byte is not an identifier char).
fn at_word_start(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let p = bytes[i - 1];
    !(p.is_ascii_alphanumeric() || p == b'_' || p == b'-')
}

/// If `keyword` matches at `i` as a whole word, return its byte length.
fn match_keyword(lower: &str, i: usize, keyword: &str) -> Option<usize> {
    let lb = lower.as_bytes();
    let kb = keyword.as_bytes();
    if i + kb.len() > lb.len() || &lb[i..i + kb.len()] != kb {
        return None;
    }
    let after = lb.get(i + kb.len()).copied().unwrap_or(b' ');
    // Next char must not continue the identifier (so `tokenizer` ≠ `token`).
    if after.is_ascii_alphanumeric() || after == b'_' {
        return None;
    }
    Some(kb.len())
}

/// Longest sensitive key matching at `i`; returns (byte len, canonical name).
fn match_sensitive_key(lower: &str, i: usize) -> Option<(usize, &'static str)> {
    for (key, name) in SENSITIVE_KEYS {
        if match_keyword(lower, i, key).is_some() {
            return Some((key.len(), name));
        }
    }
    None
}

/// Advance over the chars that sit between a key and its value (`:`, `=`, quotes,
/// spaces). Returns the index of the first value byte.
fn skip_separators(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b':' | b'=' | b'"' | b'\'' => i += 1,
            _ => break,
        }
    }
    i
}

/// Whether the span `[start, end)` (the skipped separators) actually contained a
/// `:` or `=` — guards against redacting a bare word with no assignment.
fn had_separator(bytes: &[u8], start: usize, end: usize) -> bool {
    bytes[start..end].iter().any(|&b| b == b':' || b == b'=')
}

fn skip_spaces(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

/// Consume a value token starting at `i`, stopping at a structural terminator so
/// surrounding text (closing quote/brace, next field, newline) survives.
fn consume_value(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' | b';' | b',' | b'"' | b'\'' | b'&' | b'}' | b']' => break,
            _ => i += 1,
        }
    }
    i
}

/// Consume an Authorization header value: it spans scheme + credential (so spaces
/// are PART of the value), stopping only at a line terminator or a JSON structural
/// boundary (`"`, `}`, `,`). This drops `Bearer <token>` wholesale while preserving
/// the surrounding JSON/line structure.
fn consume_to_line_end(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\r' | b'\n' | b'"' | b'\'' | b'}' | b',' => break,
            _ => i += 1,
        }
    }
    i
}

/// UTF-8 byte length of the char beginning at `i` (so the fallback copy advances
/// whole chars, never splitting a multibyte sequence).
fn next_char_len(bytes: &[u8], i: usize) -> usize {
    match bytes[i] {
        b if b < 0x80 => 1,
        b if b >> 5 == 0b110 => 2,
        b if b >> 4 == 0b1110 => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_leak(redacted: &str, secret: &str) {
        assert!(
            !redacted.contains(secret),
            "secret value leaked into output: {redacted:?}"
        );
    }

    #[test]
    fn redacts_cookie_value() {
        let out = redact("Cookie: session=abc123secret; theme=dark");
        assert_no_leak(&out, "abc123secret");
        assert!(out.contains("<redacted:cookie>"));
    }

    #[test]
    fn redacts_set_cookie_header() {
        let out = redact("set-cookie: SID=TOPSECRETVALUE; Path=/");
        assert_no_leak(&out, "TOPSECRETVALUE");
        assert!(out.contains("<redacted:cookie>"));
    }

    #[test]
    fn redacts_token_kv_and_json() {
        let kv = redact("token=deadbeefcafe");
        assert_no_leak(&kv, "deadbeefcafe");
        assert!(kv.contains("<redacted:token>"));

        let json = redact(r#"{"access_token":"xyz789abc","ok":true}"#);
        assert_no_leak(&json, "xyz789abc");
        assert!(json.contains("<redacted:token>"));
        // Surrounding JSON structure survives.
        assert!(json.contains("\"ok\":true"));
    }

    #[test]
    fn redacts_password_variants() {
        for line in [
            "password=hunter2",
            "passwd: hunter2",
            r#"{"password": "hunter2"}"#,
        ] {
            let out = redact(line);
            assert_no_leak(&out, "hunter2");
            assert!(out.contains("<redacted:password>"), "{out}");
        }
    }

    #[test]
    fn redacts_authorization_bearer() {
        let out = redact("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig");
        assert_no_leak(&out, "eyJhbGciOiJIUzI1NiJ9");
        assert!(out.contains("<redacted:authorization>"));
    }

    #[test]
    fn redacts_standalone_bearer() {
        let out = redact("calling api with Bearer sk-live-9988776655 now");
        assert_no_leak(&out, "sk-live-9988776655");
        assert!(out.contains("<redacted:token>"));
        // Trailing context is preserved.
        assert!(out.contains("now"));
    }

    #[test]
    fn redacts_ssh_private_key_block() {
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
                   b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAA\n\
                   AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB\n\
                   -----END OPENSSH PRIVATE KEY-----";
        let line = format!("loaded key:\n{key}\ndone");
        let out = redact(&line);
        assert_no_leak(&out, "b3BlbnNzaC1rZXktdjEA");
        assert!(out.contains("<redacted:ssh-private-key>"));
        assert!(out.contains("done"));
    }

    #[test]
    fn redacts_rsa_private_key_block() {
        let line =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\n-----END RSA PRIVATE KEY-----";
        let out = redact(line);
        assert_no_leak(&out, "MIIEpAIBAAKCAQEA");
        assert!(out.contains("<redacted:ssh-private-key>"));
    }

    #[test]
    fn unterminated_key_block_redacts_to_end() {
        // A truncated dump must still not leak the partial key body.
        let line = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEAleak";
        let out = redact(line);
        assert_no_leak(&out, "MIIEpAIBAAKCAQEAleak");
        assert!(out.contains("<redacted:ssh-private-key>"));
    }

    #[test]
    fn redacts_p13_rpc_token_by_key() {
        // P13 stores the per-session RPC token; if it ever lands in a log line via
        // a `token=` field the redactor catches it.
        let out = redact("rpc auth token=8f14e45fceea167a5a36dedd4bea2543");
        assert_no_leak(&out, "8f14e45fceea167a5a36dedd4bea2543");
        assert!(out.contains("<redacted:token>"));
    }

    #[test]
    fn redacts_p11_cookie_seed_value() {
        // P11 seeds real cookies; a seed line carries the cookie value.
        let out = redact("seeding cookie=NID=511=verylongcookieseedvalue domain=.google.com");
        assert_no_leak(&out, "verylongcookieseedvalue");
        assert!(out.contains("<redacted:cookie>"));
    }

    #[test]
    fn redacts_api_key_and_secret() {
        let out = redact("api_key=AKIA1234567890 client_secret: shh-very-secret");
        assert_no_leak(&out, "AKIA1234567890");
        assert_no_leak(&out, "shh-very-secret");
        assert!(out.contains("<redacted:api-key>"));
        assert!(out.contains("<redacted:secret>"));
    }

    #[test]
    fn keeps_non_secret_text_intact() {
        // No separator after the word → not an assignment → left alone.
        let out = redact("the session started and the token list is empty");
        assert_eq!(out, "the session started and the token list is empty");
    }

    #[test]
    fn does_not_match_substrings_of_longer_words() {
        // `tokenizer` / `passwordless` must not be treated as the secret key.
        let out = redact("tokenizer=fast passwordless=true");
        assert_eq!(out, "tokenizer=fast passwordless=true");
    }

    #[test]
    fn handles_multibyte_text_without_panicking() {
        let out = redact("café token=σecret-π done ✓");
        assert!(out.contains("café"));
        assert!(out.contains("<redacted:token>"));
        assert!(out.contains("done ✓"));
    }
}
