//! Time utilities with proper error handling
//!
//! Provides system time operations with proper error propagation instead of panics.

use crate::{CryptoError, Result};
use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp with proper error handling
/// 
/// Returns the number of seconds since UNIX_EPOCH (1970-01-01 00:00:00 UTC).
/// 
/// # Errors
/// 
/// Returns error if system time is before UNIX_EPOCH (should never happen on
/// properly configured systems, but we handle it gracefully).
/// 
/// # Example
/// 
/// ```
/// use aura_crypto::time::current_timestamp;
/// 
/// let timestamp = current_timestamp().expect("System time should be valid");
/// assert!(timestamp > 0);
/// ```
pub fn current_timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| CryptoError::SystemTimeError(format!(
            "System time is before UNIX epoch: {}",
            e
        )))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_current_timestamp() {
        let timestamp = current_timestamp().unwrap();
        // Should be after 2020-01-01 (1577836800)
        assert!(timestamp > 1577836800);
        // Should be reasonable (before year 2100: 4102444800)
        assert!(timestamp < 4102444800);
    }
    
    #[test]
    fn test_timestamp_monotonic() {
        let t1 = current_timestamp().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let t2 = current_timestamp().unwrap();
        assert!(t2 >= t1);
    }
}

