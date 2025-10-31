//! Utility functions for the journal system

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current timestamp in seconds since Unix epoch
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}
