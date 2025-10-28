// Convergent capabilities for local-first authorization

pub mod authority_graph;
pub mod events;
pub mod group_capabilities;
pub mod identity;
pub mod keyhive_manager;
pub mod manager;
pub mod resource_allocation;
pub mod testing;
pub mod threshold_capabilities;
pub mod types;
pub mod unified;
pub mod unified_manager;
pub mod visibility;

pub use authority_graph::AuthorityGraph;
pub use events::{CapabilityDelegation, CapabilityRevocation};
pub use group_capabilities::{
    BeeKEM, CgkaOperationType, CgkaState, EligibilityView, EligibleMember, GroupCapabilityManager,
    GroupCapabilityScope, GroupMessage, GroupOperation, GroupRoster, KeyhiveCgkaOperation,
    MemberId, MemberRole, OperationId,
};
// Re-export Epoch from aura-types for convenience
pub use aura_types::Epoch;
pub use keyhive_manager::{
    GroupMembershipProvider, InMemoryGroupProvider, KeyhiveCapabilityManager, KeyhiveConfig,
    VerificationResult,
};
pub use manager::{CapabilityGrant, CapabilityManager};
pub use resource_allocation::*;
pub use testing::MockGroupProvider;
pub use threshold_capabilities::ThresholdCapability;
pub use types::{CapabilityId, CapabilityScope, Subject};
pub use unified::{
    CapabilityToken, CommunicationOperation, DeviceAuthentication, Permission, RelayOperation,
    StorageOperation,
};
pub use visibility::VisibilityIndex;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CapabilityError {
    #[error("Invalid capability chain: {0}")]
    InvalidChain(String),

    #[error("Authority not found: {0}")]
    AuthorityNotFound(String),

    #[error("Revocation not authorized: {0}")]
    RevocationNotAuthorized(String),

    #[error("Capability expired at {0}")]
    CapabilityExpired(u64),

    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    #[error("Authorization error: {0}")]
    AuthorizationError(String),

    #[error("Cryptographic error: {0}")]
    CryptographicError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type Result<T> = std::result::Result<T, CapabilityError>;

impl From<crate::LedgerError> for CapabilityError {
    fn from(err: crate::LedgerError) -> Self {
        CapabilityError::SerializationError(err.to_string())
    }
}

impl From<CapabilityError> for aura_types::AuraError {
    fn from(err: CapabilityError) -> Self {
        match err {
            CapabilityError::InvalidChain(msg) => aura_types::AuraError::invalid_chain(&msg),
            CapabilityError::AuthorityNotFound(msg) => {
                aura_types::AuraError::authority_not_found(&msg)
            }
            CapabilityError::RevocationNotAuthorized(msg) => {
                aura_types::AuraError::revocation_not_authorized(&msg)
            }
            CapabilityError::CapabilityExpired(ts) => {
                aura_types::AuraError::capability_expired("Capability expired", ts)
            }
            CapabilityError::CryptoError(msg) => {
                aura_types::AuraError::capability_cryptographic_error(&msg)
            }
            CapabilityError::AuthorizationError(msg) => {
                aura_types::AuraError::capability_authorization_error(&msg)
            }
            CapabilityError::CryptographicError(msg) => {
                aura_types::AuraError::capability_cryptographic_error(&msg)
            }
            CapabilityError::SerializationError(msg) => {
                aura_types::AuraError::capability_serialization_error(&msg)
            }
        }
    }
}
