//! # Threshold Configuration
//!
//! Type-safe threshold configuration ensuring k-of-n validity.

use std::fmt;
use std::num::NonZeroU8;

/// Error when constructing a threshold configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThresholdError {
    /// k (threshold) cannot be zero
    KIsZero,
    /// n (total) cannot be zero
    NIsZero,
    /// k cannot exceed n
    KExceedsN {
        /// The threshold value that was too large
        k: u8,
        /// The total value that k exceeded
        n: u8,
    },
}

impl fmt::Display for ThresholdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThresholdError::KIsZero => write!(f, "Threshold (k) must be at least 1"),
            ThresholdError::NIsZero => write!(f, "Total guardians (n) must be at least 1"),
            ThresholdError::KExceedsN { k, n } => {
                write!(f, "Threshold ({k}) cannot exceed total guardians ({n})")
            }
        }
    }
}

impl std::error::Error for ThresholdError {}

/// A valid k-of-n threshold configuration
///
/// Invariants:
/// - k > 0
/// - n > 0
/// - k <= n
///
/// # Example
///
/// ```rust,ignore
/// // 2-of-3 threshold
/// let config = ThresholdConfig::new(2, 3)?;
/// assert_eq!(config.k(), 2);
/// assert_eq!(config.n(), 3);
///
/// // Invalid: k > n
/// assert!(ThresholdConfig::new(3, 2).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThresholdConfig {
    k: NonZeroU8,
    n: NonZeroU8,
}

impl ThresholdConfig {
    /// Create a new threshold configuration
    ///
    /// Returns an error if:
    /// - k is zero
    /// - n is zero
    /// - k exceeds n
    pub fn new(k: u8, n: u8) -> Result<Self, ThresholdError> {
        let k_nz = NonZeroU8::new(k).ok_or(ThresholdError::KIsZero)?;
        let n_nz = NonZeroU8::new(n).ok_or(ThresholdError::NIsZero)?;

        if k > n {
            return Err(ThresholdError::KExceedsN { k, n });
        }

        Ok(Self { k: k_nz, n: n_nz })
    }

    /// Get the threshold value (k)
    pub fn k(&self) -> u8 {
        self.k.get()
    }

    /// Get the total value (n)
    pub fn n(&self) -> u8 {
        self.n.get()
    }

    /// Check if this is a majority threshold (k > n/2)
    pub fn is_majority(&self) -> bool {
        self.k() > self.n() / 2
    }

    /// Check if this requires all participants (k == n)
    pub fn is_unanimous(&self) -> bool {
        self.k() == self.n()
    }

    /// Create a 1-of-1 configuration (single guardian)
    #[allow(clippy::unwrap_used)] // SAFETY: 1 is always non-zero
    pub fn single() -> Self {
        Self {
            k: NonZeroU8::new(1).unwrap(),
            n: NonZeroU8::new(1).unwrap(),
        }
    }

    /// Create a majority threshold (ceil((n+1)/2) of n)
    #[allow(clippy::unwrap_used)] // SAFETY: k >= 1 when n >= 1
    pub fn majority(n: u8) -> Result<Self, ThresholdError> {
        let n_nz = NonZeroU8::new(n).ok_or(ThresholdError::NIsZero)?;
        // Ceiling of (n+1)/2 for majority
        let k = (n / 2) + 1;
        Ok(Self {
            k: NonZeroU8::new(k).unwrap(),
            n: n_nz,
        })
    }
}

impl fmt::Display for ThresholdConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-of-{}", self.k(), self.n())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_thresholds() {
        assert!(ThresholdConfig::new(1, 1).is_ok());
        assert!(ThresholdConfig::new(1, 3).is_ok());
        assert!(ThresholdConfig::new(2, 3).is_ok());
        assert!(ThresholdConfig::new(3, 3).is_ok());
    }

    #[test]
    fn test_invalid_thresholds() {
        assert_eq!(ThresholdConfig::new(0, 3), Err(ThresholdError::KIsZero));
        assert_eq!(ThresholdConfig::new(2, 0), Err(ThresholdError::NIsZero));
        assert_eq!(
            ThresholdConfig::new(4, 3),
            Err(ThresholdError::KExceedsN { k: 4, n: 3 })
        );
    }

    #[test]
    fn test_majority() {
        let m3 = ThresholdConfig::majority(3).unwrap();
        assert_eq!(m3.k(), 2);
        assert_eq!(m3.n(), 3);
        assert!(m3.is_majority());

        let m5 = ThresholdConfig::majority(5).unwrap();
        assert_eq!(m5.k(), 3);
        assert_eq!(m5.n(), 5);
    }

    #[test]
    fn test_single() {
        let single = ThresholdConfig::single();
        assert_eq!(single.k(), 1);
        assert_eq!(single.n(), 1);
        assert!(single.is_unanimous());
    }
}
