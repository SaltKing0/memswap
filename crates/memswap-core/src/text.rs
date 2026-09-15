//! Platform-independent text handling.
//!
//! Memory files are plain text and are hashed byte-for-byte, so the same
//! memory saved on Windows (CRLF) and on Linux/macOS (LF) must produce the
//! same content hash. Otherwise a `.memfile` exported on one platform cannot
//! be verified on the other, which defeats the point of an interchange
//! format.
//!
//! Canonical form: LF only. Reads normalize, so the store never holds a
//! `\r\n` pair. Lone `\r` (classic Mac line endings) is normalized too.

use std::fs;
use std::path::Path;

/// Collapse `\r\n` and lone `\r` to `\n`. Idempotent.
pub fn normalize_newlines(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            // `\r\n` collapses to a single `\n`; a lone `\r` becomes `\n`.
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

/// Read a text file in canonical (LF-normalized) form.
pub fn read_text(path: impl AsRef<Path>) -> std::io::Result<String> {
    Ok(normalize_newlines(&fs::read_to_string(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_becomes_lf() {
        assert_eq!(normalize_newlines("a\r\nb\r\n"), "a\nb\n");
    }

    #[test]
    fn lone_cr_becomes_lf() {
        assert_eq!(normalize_newlines("a\rb"), "a\nb");
    }

    #[test]
    fn lf_is_untouched() {
        assert_eq!(normalize_newlines("a\nb\n"), "a\nb\n");
    }

    #[test]
    fn mixed_and_idempotent() {
        let once = normalize_newlines("a\r\nb\rc\n");
        assert_eq!(once, "a\nb\nc\n");
        assert_eq!(normalize_newlines(&once), once);
    }
}
