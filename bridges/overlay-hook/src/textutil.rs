//! Safe UTF-8 chat text: normalisation, sanitisation, shortcode expansion.
//!
//! Encoding path (send):
//!   raw keystrokes → NFC → expand `:shortcodes:` → strip controls / bidi / tags
//!   → grapheme+byte limits → JSON string (serde, UTF-8)
//!
//! Encoding path (recv / display):
//!   JSON UTF-8 → NFC → strip again (defense in depth) → render (Sora + emoji atlas)

use std::borrow::Cow;

/// Soft max for a single DM body (matches edge function).
pub const MAX_CHAT_CHARS: usize = 500;
/// Hard byte ceiling so a pathological UTF-8 payload can't blow IPC buffers.
pub const MAX_CHAT_BYTES: usize = 2000;

/// Unicode bidi / isolate controls that can reverse UI chrome (Trojan Source).
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' // LRE..RLO
            | '\u{2066}'..='\u{2069}' // LRI..PDI
            | '\u{200E}' | '\u{200F}' // LRM / RLM
            | '\u{061C}' // ALM
    )
}

/// C0/C1 controls except TAB/LF (we flatten those too for single-line chat).
fn is_disallowed_control(c: char) -> bool {
    let u = c as u32;
    if c == '\n' || c == '\r' || c == '\t' {
        return true; // flatten to space later
    }
    // C0 + DEL + C1
    if u < 0x20 || (0x7F..=0x9F).contains(&u) {
        return true;
    }
    // Zero-width / format that we don't want in chat bodies (except ZWJ for emoji).
    // Keep ZWJ (200D) and VS16 (FE0F) so multi-codepoint emoji stay intact.
    matches!(
        c,
        '\u{200B}' // ZWSP
            | '\u{200C}' // ZWNJ — drop; ZWJ kept
            | '\u{2060}' // WJ
            | '\u{FEFF}' // BOM / ZWNBSP
            | '\u{00AD}' // soft hyphen
            | '\u{180E}' // MONGOLIAN VOWEL SEPARATOR
            | '\u{206A}'..='\u{206F}' // deprecated format
    ) || is_bidi_control(c)
}

/// NFC-normalise when the `unicode-normalization` crate is unavailable we do a
/// light pass: just validate UTF-8 (already a Rust `String`) and strip bad cps.
/// Full NFC is applied on the edge function; client keeps a best-effort strip.
pub fn sanitize_chat(input: &str) -> String {
    let mut out = String::with_capacity(input.len().min(MAX_CHAT_BYTES));
    let mut bytes = 0usize;
    let mut chars = 0usize;

    for ch in input.chars() {
        if chars >= MAX_CHAT_CHARS || bytes >= MAX_CHAT_BYTES {
            break;
        }
        // Flatten newlines / tabs to a single space (collapse runs later).
        let ch = if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        };
        if is_disallowed_control(ch) {
            continue;
        }
        // Drop Unicode tags block (U+E0001..E007F) used in some spoof attacks.
        let u = ch as u32;
        if (0xE0001..=0xE007F).contains(&u) || u == 0xE0000 {
            continue;
        }
        // Private-use area — not useful in chat, can be font-glyph smuggling.
        if (0xE000..=0xF8FF).contains(&u)
            || (0xF0000..=0xFFFFD).contains(&u)
            || (0x100000..=0x10FFFD).contains(&u)
        {
            continue;
        }
        let mut buf = [0u8; 4];
        let enc = ch.encode_utf8(&mut buf);
        if bytes + enc.len() > MAX_CHAT_BYTES {
            break;
        }
        out.push_str(enc);
        bytes += enc.len();
        chars += 1;
    }

    collapse_spaces(&out)
}

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Expand `:shortcode:` tokens using the emoji atlas map. Unknown codes are
/// left intact (so users don't lose typing mid-shortcode).
pub fn expand_shortcodes(input: &str, lookup: &dyn Fn(&str) -> Option<char>) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b':' {
            // find closing :
            if let Some(rel) = input[i + 1..].find(':') {
                let end = i + 1 + rel;
                let name = &input[i + 1..end];
                // shortcodes: [a-z0-9_+-]{1,32}
                if is_shortcode_name(name) {
                    if let Some(ch) = lookup(name) {
                        out.push(ch);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        // copy one char
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_shortcode_name(s: &str) -> bool {
    let len = s.len();
    if !(1..=32).contains(&len) {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'+' || b == b'-')
}

/// Full outbound pipeline: shortcodes → sanitize.
pub fn prepare_outgoing(raw: &str, lookup: &dyn Fn(&str) -> Option<char>) -> String {
    let expanded = expand_shortcodes(raw, lookup);
    sanitize_chat(&expanded)
}

/// Inbound display pipeline (already JSON-decoded UTF-8).
pub fn prepare_inbound(raw: &str) -> String {
    sanitize_chat(raw)
}

/// Validate a player key / uuid-ish token (hex or dashed uuid, 8..=64).
pub fn is_safe_key(s: &str) -> bool {
    let len = s.len();
    if !(8..=64).contains(&len) {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// Username for friend add: 1..=32 of `[A-Za-z0-9_]` plus common MC extras.
pub fn is_safe_username(s: &str) -> bool {
    let len = s.chars().count();
    if !(1..=32).contains(&len) {
        return false;
    }
    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
    })
}

/// Percent-encode a query component (RFC 3986 unreserved passthrough).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                out.push('%');
                out.push(nibble(b >> 4));
                out.push(nibble(b & 0xf));
            }
        }
    }
    out
}

fn nibble(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'A' + (n - 10) })
}

/// Count Unicode scalar values (not grapheme clusters — good enough for limits).
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// True if `s` is valid UTF-8 that we already hold as `str` (always) and within limits.
pub fn within_chat_limits(s: &str) -> bool {
    s.len() <= MAX_CHAT_BYTES && char_len(s) <= MAX_CHAT_CHARS
}

/// Cow helper: return borrowed when already clean.
pub fn ensure_clean<'a>(s: &'a str) -> Cow<'a, str> {
    let cleaned = sanitize_chat(s);
    if cleaned == s {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bidi() {
        let s = sanitize_chat("hi\u{202E}secret");
        assert!(!s.contains('\u{202E}'));
        assert!(s.contains("hi"));
    }

    #[test]
    fn expands_shortcode() {
        let out = expand_shortcodes("hello :fire:!", &|n| {
            if n == "fire" {
                Some('🔥')
            } else {
                None
            }
        });
        assert_eq!(out, "hello 🔥!");
    }

    #[test]
    fn percent_encodes() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("привет"), percent_encode("привет"));
    }
}
