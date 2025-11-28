//! Authority Effects Trait
//!
//! This trait defines the effect interface for authority management operations
//! in the authority-centric model.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-protocol` or `aura-relational` (Layer 4 or Layer 5)
//! - **Usage**: Authority management, relational context operations
//!
//! This is an application effect core to Aura's identity model. Manages opaque
//! authorities (AuthorityId) and relational contexts (guardian bindings, recovery
//! grants, rendezvous receipts). Handlers implement authority-centric operations
//! in `aura-protocol` or `aura-relational`.

use crate::types::identifiers::ContextId;
use crate::{Authority, AuthorityId, Result};
use async_trait::async_trait;
use std::sync::Arc;

/// Effect trait for authority management operations
#[async_trait]
pub trait AuthorityEffects: Send + Sync {
    /// Get an authority by ID
    async fn get_authority(&self, id: AuthorityId) -> Result<Arc<dyn Authority>>;

    /// List all known authorities
    async fn list_authorities(&self) -> Result<Vec<AuthorityId>>;

    /// Create a new authority
    async fn create_authority(&self) -> Result<AuthorityId>;

    /// Add a device to an authority
    async fn add_device_to_authority(
        &self,
        authority_id: AuthorityId,
        device_public_key: Vec<u8>,
    ) -> Result<()>;

    /// Remove a device from an authority
    async fn remove_device_from_authority(
        &self,
        authority_id: AuthorityId,
        device_index: u32,
    ) -> Result<()>;
}

/// Effect trait for relational context operations
///
/// Note: get_context returns Box<dyn Any> to avoid circular dependencies.
/// Implementations should return their concrete RelationalContext type wrapped in Arc<dyn Any>.
/// Callers can downcast to the specific RelationalContext type they expect.
#[async_trait]
pub trait RelationalEffects: Send + Sync {
    /// Create a new relational context
    async fn create_context(&self, participants: Vec<AuthorityId>) -> Result<ContextId>;

    /// Get a relational context by ID
    /// Returns an opaque Arc<dyn Any> that can be downcast to the concrete context type
    async fn get_context(&self, id: ContextId) -> Result<Arc<dyn std::any::Any + Send + Sync>>;

    /// List contexts for an authority
    async fn list_contexts_for_authority(
        &self,
        authority_id: AuthorityId,
    ) -> Result<Vec<ContextId>>;

    /// Add participant to context
    async fn add_participant_to_context(
        &self,
        context_id: ContextId,
        participant: AuthorityId,
    ) -> Result<()>;

    /// Remove participant from context
    async fn remove_participant_from_context(
        &self,
        context_id: ContextId,
        participant: AuthorityId,
    ) -> Result<()>;
}

/// Combined authority and relational effects
#[async_trait]
pub trait AuthorityRelationalEffects: AuthorityEffects + RelationalEffects + Send + Sync {}

// Blanket implementation
impl<T> AuthorityRelationalEffects for T where T: AuthorityEffects + RelationalEffects + Send + Sync {}
