//! Time utilities with proper error handling
//!
//! Provides system time operations with proper error propagation instead of panics.

use crate::Result;

/// Get current Unix timestamp using injected effects
///
/// Returns the number of seconds since UNIX_EPOCH (1970-01-01 00:00:00 UTC).
///
/// # Example
///
/// ```
/// use aura_crypto::{time::current_timestamp_with_effects, Effects};
///
/// let effects = Effects::production();
/// let timestamp = current_timestamp_with_effects(&effects).expect("Should get valid timestamp");
/// assert!(timestamp > 0);
/// ```
pub fn current_timestamp_with_effects(effects: &crate::Effects) -> Result<u64> {
    effects.now()
}


#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    #[test]
    fn test_current_timestamp() {
        let effects = crate::Effects::production();
        let timestamp = current_timestamp_with_effects(&effects).unwrap();
        // Should be after 2020-01-01 (1577836800)
        assert!(timestamp > 1577836800);
        // Should be reasonable (before year 2100: 4102444800)
        assert!(timestamp < 4102444800);
    }

    #[test]
    fn test_timestamp_monotonic() {
        let effects = crate::Effects::production();
        let t1 = current_timestamp_with_effects(&effects).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let t2 = current_timestamp_with_effects(&effects).unwrap();
        assert!(t2 >= t1);
    }
}
