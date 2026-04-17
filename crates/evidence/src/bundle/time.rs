//! UTC time helpers used by the bundle builder.

use anyhow::Result;

/// Get current UTC time in RFC3339 format.
pub fn utc_now_rfc3339() -> Result<String> {
    let now = chrono::Utc::now();
    Ok(now.to_rfc3339())
}

/// Get current UTC time as compact timestamp (YYYYMMDD-HHMMSSZ).
pub fn utc_compact_stamp() -> Result<String> {
    let now = chrono::Utc::now();
    Ok(now.format("%Y%m%d-%H%M%SZ").to_string())
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
        let stamp = utc_compact_stamp().unwrap();
        // Format: YYYYMMDD-HHMMSSZ
        assert!(stamp.ends_with('Z'));
        assert!(stamp.contains('-'));
        assert_eq!(stamp.len(), 16);
    }
}
