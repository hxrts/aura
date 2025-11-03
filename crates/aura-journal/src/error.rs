//! Error types for the journal

use thiserror::Error;

/// Error types for journal operations.
///
/// This enum covers all failure modes in the journal subsystem, from low-level
/// storage errors to high-level coordination and permission failures.
#[derive(Debug, Error)]
pub enum Error {
    /// Generic storage operation failure.
    ///
    /// Indicates a failure in the underlying storage layer, such as
    /// filesystem errors or database connection issues.
    #[error("Storage failed: {0}")]
    Storage(String),

    /// Coordination or synchronization failure between devices.
    ///
    /// Occurs when devices cannot agree on state or when CRDT merge
    /// operations fail to converge properly.
    #[error("Coordination failed: {0}")]
    Coordination(String),

    /// Invalid operation attempted on the ledger.
    ///
    /// Raised when an operation violates ledger invariants or is
    /// malformed (e.g., adding a device that already exists).
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Specified device does not exist in the account.
    ///
    /// Raised when attempting to access or modify a device that
    /// has not been registered or has been removed.
    #[error("Device not found: {0}")]
    DeviceNotFound(aura_types::DeviceId),

    /// Specified guardian does not exist in the account.
    ///
    /// Raised when attempting to access or modify a guardian that
    /// has not been added to the recovery configuration.
    #[error("Guardian not found: {0}")]
    GuardianNotFound(aura_types::GuardianId),

    /// Automerge CRDT library error.
    ///
    /// Wraps errors from the underlying Automerge library, such as
    /// invalid document operations or merge conflicts.
    #[error("Automerge error: {0}")]
    Automerge(String),

    /// Operation exceeded its allowed time limit.
    ///
    /// Raised when a ledger operation takes longer than expected,
    /// which may indicate network issues or deadlocks.
    #[error("Operation timed out: {message}")]
    Timeout {
        /// Description of which operation timed out and context
        message: String,
    },

    /// Critical infrastructure component failure.
    ///
    /// Indicates failure of essential services like network connectivity,
    /// cryptographic modules, or persistent storage systems.
    #[error("Infrastructure failed: {reason}")]
    InfrastructureFailed {
        /// Description of what infrastructure component failed
        reason: String,
    },

    /// Distributed consensus protocol failure.
    ///
    /// Raised when devices cannot reach agreement on a state transition,
    /// typically due to insufficient participants or Byzantine behavior.
    #[error("Consensus failed: {reason}")]
    ConsensusFailed {
        /// Description of why consensus could not be reached
        reason: String,
    },

    /// Permission check failed for the requested operation.
    ///
    /// Raised when a device or user lacks the required permissions to
    /// perform an operation (e.g., only admin can add guardians).
    #[error("Permission denied: {reason}")]
    PermissionDenied {
        /// Description of which permission was missing
        reason: String,
    },

    /// Authentication verification failed.
    ///
    /// Raised when a device cannot prove its identity or when
    /// session credentials are invalid or expired.
    #[error("Authentication failed: {reason}")]
    AuthenticationFailed {
        /// Description of why authentication failed
        reason: String,
    },

    /// Required capability was not present or has been revoked.
    ///
    /// Raised when a capability-based access control check fails,
    /// preventing the operation from proceeding.
    #[error("Capability denied: {capability}")]
    CapabilityDenied {
        /// The capability that was required but not available
        capability: String,
    },

    /// Specific storage operation failed with details.
    ///
    /// More specific than the generic `Storage` variant, providing
    /// detailed context about what storage operation failed.
    #[error("Storage operation failed: {reason}")]
    StorageFailed {
        /// Detailed description of the storage failure
        reason: String,
    },

    /// Communication between devices failed.
    ///
    /// Raised when network operations fail, messages cannot be delivered,
    /// or transport-layer errors occur.
    #[error("Communication failed: {reason}")]
    CommunicationFailed {
        /// Description of the communication failure
        reason: String,
    },

    /// Invalid input provided to a journal operation.
    ///
    /// Raised when parameters fail validation checks, such as
    /// malformed IDs, empty required fields, or out-of-range values.
    #[error("Invalid input: {message}")]
    InvalidInput {
        /// Description of what input was invalid and why
        message: String,
    },
}

/// Result type for journal operations
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create a storage error
    pub fn storage_failed(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// Create a coordination error
    pub fn coordination_failed(msg: impl Into<String>) -> Self {
        Self::Coordination(msg.into())
    }

    /// Create an invalid operation error
    pub fn invalid_operation(msg: impl Into<String>) -> Self {
        Self::InvalidOperation(msg.into())
    }

    /// Create an invalid input error
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: msg.into(),
        }
    }
}
