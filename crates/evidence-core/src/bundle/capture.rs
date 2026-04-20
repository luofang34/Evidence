//! Captured subprocess output normalization.
//!
//! Every file written by [`crate::bundle::EvidenceBuilder::run_capture`]
//! into the bundle's capture directory flows through
//! [`normalize_captured_text`]. Recording raw platform-native line
//! endings would make the same logical test run on Windows and Linux
//! produce different `content_hash` values — a cross-platform
//! determinism leak that defeats the evidence chain.

/// Normalize captured subprocess text output to LF line endings.
///
/// Collapses every `\r\n` pair to a single `\n`. Lone `\r` bytes (e.g.
/// `cargo`'s progress spinners `Compiling …\r`) are deliberately
/// preserved — stripping them would corrupt legitimate carriage-return
/// usage. Lone `\n` bytes pass through unchanged.
///
/// This is a **schema-level tool invariant**: it is documented as
/// "Captured Output Normalization" in the README and is not opt-out.
pub(crate) fn normalize_captured_text(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\r' && raw.get(i + 1) == Some(&b'\n') {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_captured_text_converts_crlf_to_lf() {
        let input = b"line 1\r\nline 2\r\nline 3\r\n";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"line 1\nline 2\nline 3\n");
    }

    #[test]
    fn test_normalize_captured_text_preserves_lone_cr() {
        // cargo emits `Compiling foo\r` to rewrite a progress line.
        // Stripping lone \r would corrupt that. Only strict CRLF pairs
        // collapse.
        let input = b"Compiling foo\rCompiling bar\r\nok\r\n";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"Compiling foo\rCompiling bar\nok\n");
    }

    #[test]
    fn test_normalize_captured_text_passes_lone_lf_through() {
        let input = b"line 1\nline 2\n";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"line 1\nline 2\n");
    }

    #[test]
    fn test_normalize_captured_text_empty_input() {
        assert_eq!(normalize_captured_text(b""), b"");
    }

    #[test]
    fn test_normalize_captured_text_trailing_cr_without_lf() {
        // A trailing \r with no following \n is kept — there is no CRLF
        // pair to collapse. Matches "lone \r preserved".
        let input = b"abc\r";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"abc\r");
    }

    #[test]
    fn test_normalize_captured_text_mixed_content() {
        let input = b"header\r\n\rspinner\rdone\r\nfooter";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"header\n\rspinner\rdone\nfooter");
    }
}
