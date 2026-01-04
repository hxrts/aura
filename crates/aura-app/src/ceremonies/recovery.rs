//! # Recovery Eligibility
//!
//! Type-safe recovery eligibility check ensuring threshold is configured.

use super::ThresholdConfig;
use aura_core::identifiers::AuthorityId;
use std::fmt;

/// Error when checking recovery eligibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryError {
    /// No threshold scheme is configured
    NoThresholdConfigured,
    /// Not enough guardians to meet threshold
    InsufficientGuardians {
        /// Number of guardians required (k)
        required: u8,
        /// Number of guardians available
        available: usize,
    },
    /// No guardians are currently reachable
    NoReachableGuardians,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryError::NoThresholdConfigured => {
                write!(f, "Set up guardians first before requesting recovery")
            }
            RecoveryError::InsufficientGuardians {
                required,
                available,
            } => {
                write!(
                    f,
                    "Need {required} guardians for recovery, but only {available} configured"
                )
            }
            RecoveryError::NoReachableGuardians => {
                write!(f, "No guardians are currently reachable")
            }
        }
    }
}

impl std::error::Error for RecoveryError {}

/// A validated recovery eligibility state
///
/// Invariants:
/// - Threshold scheme is configured
/// - At least k guardians are available
///
/// # Example
///
/// ```rust,ignore
/// let eligible = RecoveryEligible::check(Some(threshold), &guardians)?;
///
/// // Can now safely start recovery
/// start_recovery(eligible);
/// ```
#[derive(Debug, Clone)]
pub struct RecoveryEligible {
    threshold: ThresholdConfig,
    guardians: Vec<AuthorityId>,
}

impl RecoveryEligible {
    /// Check if recovery is possible
    ///
    /// # Arguments
    ///
    /// * `threshold` - The configured threshold scheme (if any)
    /// * `guardians` - The list of guardian authority IDs
    ///
    /// Returns an error if:
    /// - No threshold is configured
    /// - There aren't enough guardians to meet the threshold
    pub fn check(
        threshold: Option<ThresholdConfig>,
        guardians: &[AuthorityId],
    ) -> Result<Self, RecoveryError> {
        let threshold = threshold.ok_or(RecoveryError::NoThresholdConfigured)?;

        let required = threshold.k();
        let available = guardians.len();

        if available < required as usize {
            return Err(RecoveryError::InsufficientGuardians {
                required,
                available,
            });
        }

        Ok(Self {
            threshold,
            guardians: guardians.to_vec(),
        })
    }

    /// Get the threshold configuration
    pub fn threshold(&self) -> &ThresholdConfig {
        &self.threshold
    }

    /// Get the guardian list
    pub fn guardians(&self) -> &[AuthorityId] {
        &self.guardians
    }

    /// Get the number of approvals needed
    pub fn approvals_needed(&self) -> u8 {
        self.threshold.k()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_authority() -> AuthorityId {
        AuthorityId::from_uuid(Uuid::new_v4())
    }

    #[test]
    fn test_no_threshold() {
        let guardians = vec![make_authority()];
        let result = RecoveryEligible::check(None, &guardians);
        assert_eq!(result.unwrap_err(), RecoveryError::NoThresholdConfigured);
    }

    #[test]
    fn test_insufficient_guardians() {
        let threshold = ThresholdConfig::new(2, 3).unwrap();
        let guardians = vec![make_authority()]; // Only 1

        let result = RecoveryEligible::check(Some(threshold), &guardians);
        assert_eq!(
            result.unwrap_err(),
            RecoveryError::InsufficientGuardians {
                required: 2,
                available: 1
            }
        );
    }

    #[test]
    fn test_valid_recovery() {
        let threshold = ThresholdConfig::new(2, 3).unwrap();
        let guardians = vec![make_authority(), make_authority(), make_authority()];

        let eligible = RecoveryEligible::check(Some(threshold), &guardians).unwrap();
        assert_eq!(eligible.approvals_needed(), 2);
        assert_eq!(eligible.guardians().len(), 3);
    }
}
