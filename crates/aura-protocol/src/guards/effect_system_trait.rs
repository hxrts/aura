//! Effect System Trait for Guards
//!
//! This module defines the minimal trait that guards need from an effect system.
//! This avoids circular dependencies by not depending on the concrete AuraEffectSystem type.

use crate::effects::JournalEffects;
use crate::guards::flow::FlowBudgetEffects;
use async_trait::async_trait;
use aura_core::effects::StorageEffects;
use aura_core::{DeviceId, TimeEffects};

/// Minimal interface that guards need from an effect system
///
/// This trait abstracts over the concrete AuraEffectSystem to avoid circular dependencies.
/// Guards work with this trait, and the actual runtime implements it.
///
/// Note: This trait extends JournalEffects because guards need access to journal operations
/// for coupling protocol execution with distributed state updates.
pub trait GuardEffectSystem:
    JournalEffects + StorageEffects + FlowBudgetEffects + TimeEffects + Send + Sync
{
    /// Get the device ID for this effect system
    fn device_id(&self) -> DeviceId;

    /// Get the execution mode
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode;

    /// Query metadata from the effect system
    fn get_metadata(&self, key: &str) -> Option<String>;

    /// Check if this effect system can perform a specific operation
    fn can_perform_operation(&self, operation: &str) -> bool;
}

/// Security context for guard operations
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Device performing the operation
    pub device_id: DeviceId,

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
            device_id: DeviceId::default(),
            security_level: SecurityLevel::Normal,
            hardware_secure: false,
        }
    }
}

// TEMPORARY: Implementation for type alias until refactoring in Phase 4
use crate::effects::AuraEffects;
use aura_core::effects::ExecutionMode;

impl GuardEffectSystem for Box<dyn AuraEffects> {
    fn device_id(&self) -> DeviceId {
        // TODO: This should come from the actual effect system
        // For now, return a dummy device ID to make compilation work
        DeviceId::new()
    }

    fn execution_mode(&self) -> ExecutionMode {
        // TODO: This should come from the actual effect system
        // For now, return a default mode
        ExecutionMode::Production
    }

    fn get_metadata(&self, _key: &str) -> Option<String> {
        // TODO: This should delegate to the actual effect system
        // For now, return None to indicate no metadata available
        None
    }

    fn can_perform_operation(&self, _operation: &str) -> bool {
        // TODO: This should check the actual effect system capabilities
        // For now, return true to allow all operations
        true
    }
}

#[async_trait::async_trait]
impl FlowBudgetEffects for Box<dyn AuraEffects> {
    async fn charge_flow(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> aura_core::AuraResult<aura_core::Receipt> {
        // Use the journal-backed flow budget charge to honor charge-before-send
        let updated_budget =
            crate::effects::JournalEffects::charge_flow_budget(&**self, context, peer, cost)
                .await?;

        // Build a receipt chained by spent value as a monotone nonce; signatures are left empty here
        let nonce = updated_budget.spent;
        let epoch = updated_budget.epoch;
        Ok(aura_core::Receipt::new(
            context.clone(),
            DeviceId::new(), // source is unknown from the boxed trait object
            *peer,
            epoch,
            cost,
            nonce,
            aura_core::Hash32::default(),
            Vec::new(),
        ))
    }
}

// Explicit trait implementations to resolve trait bound issues
#[async_trait]
impl aura_core::TimeEffects for Box<dyn AuraEffects> {
    async fn current_epoch(&self) -> u64 {
        aura_core::TimeEffects::current_epoch(&**self).await
    }

    async fn current_timestamp(&self) -> u64 {
        aura_core::TimeEffects::current_timestamp(&**self).await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        (**self).current_timestamp_millis().await
    }

    async fn sleep_ms(&self, ms: u64) {
        (**self).sleep_ms(ms).await
    }

    async fn sleep_until(&self, epoch: u64) {
        (**self).sleep_until(epoch).await
    }

    async fn delay(&self, duration: std::time::Duration) {
        (**self).delay(duration).await
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), aura_core::AuraError> {
        (**self).sleep(duration_ms).await
    }

    async fn yield_until(
        &self,
        condition: aura_core::effects::time::WakeCondition,
    ) -> Result<(), aura_core::effects::time::TimeError> {
        (**self).yield_until(condition).await
    }

    async fn wait_until(
        &self,
        condition: aura_core::effects::time::WakeCondition,
    ) -> Result<(), aura_core::AuraError> {
        (**self).wait_until(condition).await
    }

    async fn set_timeout(&self, timeout_ms: u64) -> aura_core::effects::time::TimeoutHandle {
        aura_core::TimeEffects::set_timeout(&**self, timeout_ms).await
    }

    async fn cancel_timeout(
        &self,
        handle: aura_core::effects::time::TimeoutHandle,
    ) -> Result<(), aura_core::effects::time::TimeError> {
        (**self).cancel_timeout(handle).await
    }

    fn is_simulated(&self) -> bool {
        aura_core::TimeEffects::is_simulated(&**self)
    }

    fn register_context(&self, context_id: uuid::Uuid) {
        (**self).register_context(context_id)
    }

    fn unregister_context(&self, context_id: uuid::Uuid) {
        (**self).unregister_context(context_id)
    }

    async fn notify_events_available(&self) {
        (**self).notify_events_available().await
    }

    fn resolution_ms(&self) -> u64 {
        (**self).resolution_ms()
    }

    async fn now_instant(&self) -> std::time::Instant {
        (**self).now_instant().await
    }
}

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
impl crate::effects::JournalEffects for Box<dyn AuraEffects> {
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
        peer: &DeviceId,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        (**self).get_flow_budget(context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &DeviceId,
        budget: &aura_core::FlowBudget,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        (**self).update_flow_budget(context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        (**self).charge_flow_budget(context, peer, cost).await
    }
}
