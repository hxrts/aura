//! Effect System Trait for Guards
//!
//! Compatibility shims for guard callers while migrating fully to the pure
//! guard interpreter path (ADR-014). This module intentionally limits the
//! surface area to authority/metadata access and trait object adapters.

use crate::effects::AuraEffects;
use crate::effects::JournalEffects;
use async_trait::async_trait;
use aura_core::effects::ExecutionMode;
use aura_core::effects::{
    AuthorizationEffects, FlowBudgetEffects, LeakageEffects, PhysicalTimeEffects, RandomEffects,
    StorageEffects, TimeError,
};
use aura_core::identifiers::AuthorityId;

/// Minimal context provider for guards (authority + metadata).
pub trait GuardContextProvider {
    fn authority_id(&self) -> AuthorityId;
    fn get_metadata(&self, key: &str) -> Option<String>;
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
    fn can_perform_operation(&self, _operation: &str) -> bool {
        true
    }
}

pub const META_BISCUIT_TOKEN: &str = "biscuit_token";
pub const META_BISCUIT_ROOT_PK: &str = "biscuit_root_pk";

pub fn require_biscuit_metadata(
    provider: &impl GuardContextProvider,
) -> aura_core::AuraResult<(String, String)> {
    let token = provider.get_metadata(META_BISCUIT_TOKEN).ok_or_else(|| {
        aura_core::AuraError::invalid("missing biscuit_token metadata".to_string())
    })?;
    let root_pk = provider.get_metadata(META_BISCUIT_ROOT_PK).ok_or_else(|| {
        aura_core::AuraError::invalid("missing biscuit_root_pk metadata".to_string())
    })?;
    Ok((token, root_pk))
}

/// Security context for guard operations
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Authority performing the operation
    pub authority_id: AuthorityId,
    /// Current security level
    pub security_level: SecurityLevel,
    /// Whether hardware security is available
    pub hardware_secure: bool,
}

/// Security level enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Low security - testing/development
    Low,
    /// Normal security - production
    Normal,
    /// High security - sensitive operations
    High,
}

impl Default for SecurityContext {
    fn default() -> Self {
        Self {
            authority_id: AuthorityId::default(),
            security_level: SecurityLevel::Normal,
            hardware_secure: false,
        }
    }
}

impl GuardContextProvider for Box<dyn AuraEffects> {
    fn authority_id(&self) -> AuthorityId {
        // Fallback authority ID for boxed trait objects; production systems should pass concrete implementors.
        AuthorityId::default()
    }

    fn get_metadata(&self, _key: &str) -> Option<String> {
        None
    }

    fn execution_mode(&self) -> ExecutionMode {
        AuraEffects::execution_mode(self.as_ref())
    }
}

// PhysicalTimeEffects is automatically provided by AuraEffects trait bounds
#[async_trait]
impl PhysicalTimeEffects for Box<dyn AuraEffects> {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        PhysicalTimeEffects::physical_time(&**self).await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        PhysicalTimeEffects::sleep_ms(&**self, ms).await
    }
}

// FlowBudgetEffects implementation for boxed AuraEffects
#[async_trait::async_trait]
impl FlowBudgetEffects for Box<dyn AuraEffects> {
    async fn charge_flow(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> aura_core::AuraResult<aura_core::Receipt> {
        // Use journal-backed flow budget charge to honor charge-before-send
        let updated_budget =
            crate::effects::JournalEffects::charge_flow_budget(&**self, context, peer, cost)
                .await?;

        let nonce = updated_budget.spent;
        let epoch = updated_budget.epoch;
        Ok(aura_core::Receipt::new(
            *context,
            AuthorityId::default(),
            *peer,
            epoch,
            cost,
            nonce,
            aura_core::Hash32::default(),
            Vec::new(),
        ))
    }
}

// RandomEffects implementation
#[async_trait]
impl RandomEffects for Box<dyn AuraEffects> {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        RandomEffects::random_bytes(&**self, len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        RandomEffects::random_bytes_32(&**self).await
    }

    async fn random_u64(&self) -> u64 {
        RandomEffects::random_u64(&**self).await
    }

    async fn random_range(&self, low: u64, high: u64) -> u64 {
        RandomEffects::random_range(&**self, low, high).await
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        RandomEffects::random_uuid(&**self).await
    }
}

// TimeEffects implementation
// StorageEffects implementation
#[async_trait]
impl StorageEffects for Box<dyn AuraEffects> {
    async fn store(
        &self,
        key: &str,
        data: Vec<u8>,
    ) -> Result<(), aura_core::effects::StorageError> {
        (**self).store(key, data).await
    }

    async fn retrieve(
        &self,
        key: &str,
    ) -> Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
        (**self).retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
        (**self).remove(key).await
    }

    async fn list_keys(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, aura_core::effects::StorageError> {
        (**self).list_keys(prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
        (**self).exists(key).await
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), aura_core::effects::StorageError> {
        (**self).store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, aura_core::effects::StorageError> {
        (**self).retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), aura_core::effects::StorageError> {
        (**self).clear_all().await
    }

    async fn stats(
        &self,
    ) -> Result<aura_core::effects::StorageStats, aura_core::effects::StorageError> {
        (**self).stats().await
    }
}

// JournalEffects implementation
#[async_trait]
impl JournalEffects for Box<dyn AuraEffects> {
    async fn merge_facts(
        &self,
        target: &aura_core::Journal,
        delta: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        (**self).merge_facts(target, delta).await
    }

    async fn refine_caps(
        &self,
        target: &aura_core::Journal,
        refinement: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        (**self).refine_caps(target, refinement).await
    }

    async fn get_journal(&self) -> Result<aura_core::Journal, aura_core::AuraError> {
        (**self).get_journal().await
    }

    async fn persist_journal(
        &self,
        journal: &aura_core::Journal,
    ) -> Result<(), aura_core::AuraError> {
        (**self).persist_journal(journal).await
    }

    async fn get_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &AuthorityId,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        (**self).get_flow_budget(context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &AuthorityId,
        budget: &aura_core::FlowBudget,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        (**self).update_flow_budget(context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        (**self).charge_flow_budget(context, peer, cost).await
    }
}

// AuthorizationEffects implementation
#[async_trait]
impl AuthorizationEffects for Box<dyn AuraEffects> {
    async fn verify_capability(
        &self,
        capabilities: &aura_core::Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, aura_core::effects::AuthorizationError> {
        AuthorizationEffects::verify_capability(&**self, capabilities, operation, resource).await
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &aura_core::Cap,
        requested_capabilities: &aura_core::Cap,
        target_authority: &AuthorityId,
    ) -> Result<aura_core::Cap, aura_core::effects::AuthorizationError> {
        AuthorizationEffects::delegate_capabilities(
            &**self,
            source_capabilities,
            requested_capabilities,
            target_authority,
        )
        .await
    }
}

// LeakageEffects implementation
#[async_trait]
impl LeakageEffects for Box<dyn AuraEffects> {
    async fn record_leakage(
        &self,
        event: aura_core::effects::LeakageEvent,
    ) -> aura_core::Result<()> {
        (**self).record_leakage(event).await
    }

    async fn get_leakage_budget(
        &self,
        context_id: aura_core::identifiers::ContextId,
    ) -> aura_core::Result<aura_core::effects::LeakageBudget> {
        (**self).get_leakage_budget(context_id).await
    }

    async fn check_leakage_budget(
        &self,
        context_id: aura_core::identifiers::ContextId,
        observer: aura_core::effects::ObserverClass,
        amount: u64,
    ) -> aura_core::Result<bool> {
        (**self)
            .check_leakage_budget(context_id, observer, amount)
            .await
    }

    async fn get_leakage_history(
        &self,
        context_id: aura_core::identifiers::ContextId,
        since_timestamp: Option<&aura_core::time::PhysicalTime>,
    ) -> aura_core::Result<Vec<aura_core::effects::LeakageEvent>> {
        (**self)
            .get_leakage_history(context_id, since_timestamp)
            .await
    }
}
