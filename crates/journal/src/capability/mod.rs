// Convergent capabilities for local-first authorization

pub mod authority_graph;
pub mod events;
pub mod identity;
pub mod manager;
pub mod resource_allocation;
pub mod types;
pub mod unified;
pub mod visibility;

pub use authority_graph::AuthorityGraph;
pub use events::{CapabilityDelegation, CapabilityRevocation};
pub use manager::{CapabilityGrant, CapabilityManager};
pub use resource_allocation::*;
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
