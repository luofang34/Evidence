//! UTC time helpers used by the bundle builder.
//!
//! These wrap `chrono::Utc::now()`; formatting is infallible so the
//! return type is `String`, not `Result<String, _>`.

/// Get current UTC time in RFC3339 format.
pub fn utc_now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Get current UTC time as compact timestamp (YYYYMMDD-HHMMSSZ).
pub fn utc_compact_stamp() -> String {
    chrono::Utc::now().format("%Y%m%d-%H%M%SZ").to_string()
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
    fn test_utc_compact_stamp_format() {
        let stamp = utc_compact_stamp();
        // Format: YYYYMMDD-HHMMSSZ
        assert!(stamp.ends_with('Z'));
        assert!(stamp.contains('-'));
        assert_eq!(stamp.len(), 16);
    }
}
