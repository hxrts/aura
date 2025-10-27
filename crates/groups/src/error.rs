//! Groups errors - using unified error system

// Re-export unified error system
pub use aura_errors::{AuraError, ErrorCode, ErrorSeverity, Result};

// Type aliases for backward compatibility
pub type GroupsError = AuraError;
pub type GroupError = AuraError;
pub type BeekemError = AuraError;
pub type CgkaError = AuraError;

// Groups-specific error constructors as standalone functions
pub fn group_creation_failed(reason: impl Into<String>) -> AuraError {
    AuraError::coordination_failed(format!("Group creation failed: {}", reason.into()))
}

pub fn group_join_failed(reason: impl Into<String>) -> AuraError {
    AuraError::coordination_failed(format!("Group join failed: {}", reason.into()))
}

pub fn group_leave_failed(reason: impl Into<String>) -> AuraError {
    AuraError::coordination_failed(format!("Group leave failed: {}", reason.into()))
}

pub fn key_agreement_failed(reason: impl Into<String>) -> AuraError {
    AuraError::key_derivation_failed(format!("Group key agreement failed: {}", reason.into()))
}

pub fn group_message_encryption_failed(reason: impl Into<String>) -> AuraError {
    AuraError::encryption_failed(format!("Group message encryption failed: {}", reason.into()))
}

pub fn group_message_decryption_failed(reason: impl Into<String>) -> AuraError {
    AuraError::decryption_failed(format!("Group message decryption failed: {}", reason.into()))
}

pub fn beekem_failed(reason: impl Into<String>) -> AuraError {
    AuraError::coordination_failed(format!("BeeKEM protocol failed: {}", reason.into()))
}

pub fn group_membership_error(reason: impl Into<String>) -> AuraError {
    AuraError::insufficient_capability(format!("Group membership error: {}", reason.into()))
}

pub fn group_not_found(group_id: impl Into<String>) -> AuraError {
    AuraError::storage_read_failed(format!("Group not found: {}", group_id.into()))
}

pub fn invalid_group_state(reason: impl Into<String>) -> AuraError {
    AuraError::coordination_failed(format!("Invalid group state: {}", reason.into()))
}