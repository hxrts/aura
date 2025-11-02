//! Utility functions for the journal system

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current timestamp in seconds since Unix epoch
pub fn current_timestamp() -> u64 {
    #[allow(clippy::disallowed_methods)]
    let now = SystemTime::now();
    now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
