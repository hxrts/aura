//! Authority Effects Trait
//!
//! This trait defines the effect interface for authority management operations
//! in the authority-centric model.

use crate::identifiers::ContextId;
use crate::relationships::RelationalContext;
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
#[async_trait]
pub trait RelationalEffects: Send + Sync {
    /// Create a new relational context
    async fn create_context(&self, participants: Vec<AuthorityId>) -> Result<ContextId>;

    /// Get a relational context by ID
    async fn get_context(&self, id: ContextId) -> Result<Arc<RelationalContext>>;

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
