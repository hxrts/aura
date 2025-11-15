//! Authorization Effects
//!
//! Provides capability-based authorization primitives for access control across
//! the Aura system. These effects enable verification of permissions, delegation
//! of authority, and enforcement of security policies.

use crate::{AuraError, DeviceId, Cap};
use async_trait::async_trait;

/// Authorization operations for capability-based access control
///
/// This trait provides pure authorization primitives that can be composed
/// into complex permission systems. All operations are stateless and work
/// with explicit capability tokens and access policies.
#[async_trait]
pub trait AuthorizationEffects {
    /// Verify that capabilities grant permission for a specific operation
    ///
    /// # Arguments
    /// * `capabilities` - The capability lattice to check
    /// * `operation` - The operation requiring authorization
    /// * `resource` - The resource being accessed
    ///
    /// # Returns
    /// * `Ok(true)` if authorization is granted
    /// * `Ok(false)` if authorization is denied
    /// * `Err(_)` if verification failed due to system error
    async fn verify_capability(
        &self,
        capabilities: &Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, AuthorizationError>;

    /// Delegate a subset of capabilities to another entity
    ///
    /// Creates a new capability set that contains only the permissions
    /// that are both requested and present in the source capabilities.
    /// This implements the principle of least privilege using meet-semilattice operations.
    ///
    /// # Arguments
    /// * `source_capabilities` - The capabilities to delegate from
    /// * `requested_capabilities` - The capabilities being requested
    /// * `target_device` - The device receiving the delegation
    ///
    /// # Returns
    /// The intersection of source and requested capabilities (source ⊓ requested)
    async fn delegate_capabilities(
        &self,
        source_capabilities: &Cap,
        requested_capabilities: &Cap,
        target_device: &DeviceId,
    ) -> Result<Cap, AuthorizationError>;
}

/// Errors that can occur during authorization operations
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    /// The requested operation is not permitted
    #[error("Access denied: {operation} on {resource}")]
    AccessDenied {
        operation: String,
        resource: String,
    },

    /// The capability set is invalid or malformed
    #[error("Invalid capability set: {reason}")]
    InvalidCapabilities { reason: String },

    /// The access token is invalid, expired, or revoked
    #[error("Invalid access token: {reason}")]
    InvalidToken { reason: String },

    /// Cryptographic verification failed
    #[error("Signature verification failed")]
    SignatureError,

    /// System error during authorization
    #[error("Authorization system error: {0}")]
    SystemError(#[from] AuraError),
}