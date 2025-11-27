//! Effect System Trait for Guards
//!
//! Minimal interface guards need from an effect system without depending on the
//! concrete AuraEffectSystem type.

use crate::effects::AuraEffects;
use crate::effects::JournalEffects;
use async_trait::async_trait;
use aura_core::effects::ExecutionMode;
use aura_core::effects::{
    AuthorizationEffects, FlowBudgetEffects, LeakageEffects, PhysicalTimeEffects, RandomEffects,
    StorageEffects, TimeError,
};
use aura_core::identifiers::AuthorityId;

/// Minimal interface that guards need from an effect system
pub trait GuardEffectSystem:
    JournalEffects
    + StorageEffects
    + FlowBudgetEffects
    + PhysicalTimeEffects
    + RandomEffects
    + AuthorizationEffects
    + LeakageEffects
    + Send
    + Sync
{
    /// Get the authority ID for this effect system
    fn authority_id(&self) -> AuthorityId;

    /// Get the execution mode
    fn execution_mode(&self) -> ExecutionMode;

    /// Query metadata from the effect system
    fn get_metadata(&self, key: &str) -> Option<String>;

    /// Check if this effect system can perform a specific operation
    fn can_perform_operation(&self, operation: &str) -> bool;
}

/// Compatibility shim for migrating to pure guard execution.
/// Remove once all guard callers use GuardChainExecutor with GuardSnapshot/EffectCommand.
pub trait GuardContextProvider {
    fn authority_id(&self) -> AuthorityId;
    fn get_metadata(&self, key: &str) -> Option<String>;
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

/// GuardEffectSystem for boxed AuraEffects
impl GuardEffectSystem for Box<dyn AuraEffects> {
    fn authority_id(&self) -> AuthorityId {
        // Fallback authority ID for boxed trait objects; production systems should pass concrete implementors.
        AuthorityId::new()
    }

    fn execution_mode(&self) -> ExecutionMode {
        AuraEffects::execution_mode(self.as_ref())
    }

    fn get_metadata(&self, _key: &str) -> Option<String> {
        None
    }

    fn can_perform_operation(&self, operation: &str) -> bool {
        if let Some(allowed_ops) = GuardContextProvider::get_metadata(self, "allowed_operations") {
            allowed_ops.split(',').any(|op| op.trim() == operation)
        } else {
            true
        }
    }
}

impl GuardContextProvider for Box<dyn AuraEffects> {
    fn authority_id(&self) -> AuthorityId {
        GuardEffectSystem::authority_id(self)
    }

    fn get_metadata(&self, key: &str) -> Option<String> {
        GuardEffectSystem::get_metadata(self, key)
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
            AuthorityId::new(),
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
        since_timestamp: Option<u64>,
    ) -> aura_core::Result<Vec<aura_core::effects::LeakageEvent>> {
        (**self)
            .get_leakage_history(context_id, since_timestamp)
            .await
    }
}
