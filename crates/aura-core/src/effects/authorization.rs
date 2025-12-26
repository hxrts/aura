//! Authorization Effects
//!
//! Provides capability-based authorization primitives for access control across
//! the Aura system. These effects enable verification of permissions, delegation
//! of authority, and enforcement of security policies.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-authorization` / `aura-protocol` (Layer 2/4)
//! - **Usage**: Biscuit token evaluation and capability-based authorization
//!
//! This is an application effect implemented in domain crates by composing
//! infrastructure effects with authorization-specific logic.

use crate::types::identifiers::AuthorityId;
use crate::types::scope::ResourceScope;
use crate::{AuraError, Cap};
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
    /// * `target_authority` - The authority receiving the delegation
    ///
    /// # Returns
    /// The intersection of source and requested capabilities (source ⊓ requested)
    async fn delegate_capabilities(
        &self,
        source_capabilities: &Cap,
        requested_capabilities: &Cap,
        target_authority: &AuthorityId,
    ) -> Result<Cap, AuthorizationError>;
}

/// Errors that can occur during authorization operations
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    /// The requested operation is not permitted
    #[error("Access denied: {operation} on {resource}")]
    AccessDenied { operation: String, resource: String },

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

/// Authorization decision result
#[derive(Debug, Clone)]
pub struct AuthorizationDecision {
    /// Whether the operation is authorized
    pub authorized: bool,
    /// Optional reason for the decision
    pub reason: Option<String>,
}

/// Biscuit token-based authorization effects
///
/// This trait enables Biscuit authorization checks without creating domain dependencies.
/// The journal domain can use this trait while the implementation lives in aura-authorization.
#[async_trait]
pub trait BiscuitAuthorizationEffects {
    /// Authorize an operation against a Biscuit token and resource scope
    ///
    /// # Arguments
    /// * `token_data` - Raw Biscuit token bytes
    /// * `operation` - The operation being attempted
    /// * `scope` - The resource scope for the operation
    ///
    /// # Returns
    /// Authorization decision with detailed reasoning
    async fn authorize_biscuit(
        &self,
        token_data: &[u8],
        operation: &str,
        scope: &ResourceScope,
    ) -> Result<AuthorizationDecision, AuthorizationError>;

    /// Verify a fact authorization using Biscuit tokens
    ///
    /// # Arguments
    /// * `token_data` - Raw Biscuit token bytes
    /// * `fact_type` - Type of fact being authorized
    /// * `scope` - Resource scope for the fact
    ///
    /// # Returns
    /// Whether the fact is authorized
    async fn authorize_fact(
        &self,
        token_data: &[u8],
        fact_type: &str,
        scope: &ResourceScope,
    ) -> Result<bool, AuthorizationError>;
}

/// Blanket implementation for Arc<T> where T: AuthorizationEffects
#[async_trait]
impl<T> AuthorizationEffects for std::sync::Arc<T>
where
    T: AuthorizationEffects + ?Sized + Send + Sync,
{
    async fn verify_capability(
        &self,
        capabilities: &Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, AuthorizationError> {
        (**self)
            .verify_capability(capabilities, operation, resource)
            .await
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &Cap,
        requested_capabilities: &Cap,
        target_authority: &AuthorityId,
    ) -> Result<Cap, AuthorizationError> {
        (**self)
            .delegate_capabilities(
                source_capabilities,
                requested_capabilities,
                target_authority,
            )
            .await
    }
}

/// Blanket implementation for Arc<T> where T: BiscuitAuthorizationEffects
#[async_trait]
impl<T> BiscuitAuthorizationEffects for std::sync::Arc<T>
where
    T: BiscuitAuthorizationEffects + ?Sized + Send + Sync,
{
    async fn authorize_biscuit(
        &self,
        token_data: &[u8],
        operation: &str,
        scope: &ResourceScope,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        (**self)
            .authorize_biscuit(token_data, operation, scope)
            .await
    }

    async fn authorize_fact(
        &self,
        token_data: &[u8],
        fact_type: &str,
        scope: &ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        (**self).authorize_fact(token_data, fact_type, scope).await
    }
}
