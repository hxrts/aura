//! Time utilities for timestamp generation
//!
//! Provides consistent timestamp generation across the agent crate.

/// Get current timestamp in milliseconds since UNIX epoch
pub fn timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time before UNIX epoch")
        .as_millis()
}

/// Get current timestamp in seconds since UNIX epoch
pub fn timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time before UNIX epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_millis() {
        let ts1 = timestamp_millis();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ts2 = timestamp_millis();
        assert!(ts2 > ts1);
        assert!(ts2 - ts1 >= 10);
    }

    #[test]
    fn test_timestamp_secs() {
        let ts = timestamp_secs();
        // Should be a reasonable timestamp (after 2020)
        assert!(ts > 1_600_000_000);
    }
}
