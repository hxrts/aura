//! Recovery-state error types.

use aura_core::types::identifiers::AuthorityId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryError {
    NoActiveRecovery,
    NoSuchRecovery(String),
    NoSuchGuardian(AuthorityId),
    AlreadyApproved(AuthorityId),
    GuardianAlreadyExists(AuthorityId),
    GuardianNotFound(AuthorityId),
    InvalidThreshold { threshold: u32, guardian_count: u32 },
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoActiveRecovery => write!(f, "No active recovery process"),
            Self::NoSuchRecovery(id) => write!(f, "Recovery not found: {id}"),
            Self::NoSuchGuardian(id) => write!(f, "Guardian not found: {id:?}"),
            Self::AlreadyApproved(id) => write!(f, "Guardian already approved: {id:?}"),
            Self::GuardianAlreadyExists(id) => write!(f, "Guardian already exists: {id:?}"),
            Self::GuardianNotFound(id) => write!(f, "Guardian not found: {id:?}"),
            Self::InvalidThreshold {
                threshold,
                guardian_count,
            } => write!(
                f,
                "Invalid threshold: {threshold} of {guardian_count} guardians"
            ),
        }
    }
}

impl std::error::Error for RecoveryError {}
