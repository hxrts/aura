//! Capability guard middleware for effect handlers
//!
//! This module provides middleware that wraps existing effect handlers with
//! capability checking based on the non-interference property from the formal
//! specification.

use super::capability::{
    CapabilityGuard, CapabilityResult, EffectRequirement, GuardedContext, GuardedEffect,
};
use crate::effects::{CryptoEffects, JournalEffects, StorageEffects, SystemEffects, TimeEffects};
use async_trait::async_trait;
use aura_core::{AuraError, Cap, Fact, Journal, MessageContext};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Middleware that wraps effect handlers with capability checking
pub struct CapabilityMiddleware<T> {
    /// The underlying effect handler
    inner: T,
    /// The capability guard for authorization checking
    guard: Arc<Mutex<CapabilityGuard>>,
    /// Current execution context
    context: GuardedContext,
}

impl<T> CapabilityMiddleware<T> {
    /// Create new capability middleware wrapping an effect handler
    pub fn new(inner: T, context: GuardedContext) -> Self {
        Self {
            inner,
            guard: Arc::new(Mutex::new(CapabilityGuard::new())),
            context,
        }
    }

    /// Create capability middleware for testing (bypasses enforcement)
    pub fn for_testing(inner: T, context: GuardedContext) -> Self {
        Self {
            inner,
            guard: Arc::new(Mutex::new(CapabilityGuard::for_testing())),
            context,
        }
    }

    /// Update the execution context
    pub fn with_context(mut self, context: GuardedContext) -> Self {
        self.context = context;
        self
    }

    /// Execute an effect with capability checking
    async fn execute_guarded<E>(&self, effect: E) -> CapabilityResult<()>
    where
        E: GuardedEffect,
    {
        let mut guard = self.guard.lock().await;
        guard.execute_guarded_effect(&effect, &self.context).await
    }

    /// Get a reference to the inner handler
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get audit log from the capability guard
    pub async fn audit_log(&self) -> Vec<super::capability::CapabilityAuditEntry> {
        let guard = self.guard.lock().await;
        guard.audit_log().to_vec()
    }
}

/// Guarded effect for reading facts
struct ReadFactsEffect;

#[async_trait]
impl GuardedEffect for ReadFactsEffect {
    fn capability_requirements(&self) -> EffectRequirement {
        super::capability::JournalRequirements::read_facts()
    }

    fn operation_name(&self) -> &'static str {
        "read_facts"
    }

    async fn execute_guarded(&self, _context: &GuardedContext) -> CapabilityResult<()> {
        Ok(())
    }
}

/// Guarded effect for merging facts
struct MergeFactsEffect {
    delta: Fact,
}

#[async_trait]
impl GuardedEffect for MergeFactsEffect {
    fn capability_requirements(&self) -> EffectRequirement {
        super::capability::JournalRequirements::merge_facts()
    }

    fn operation_name(&self) -> &'static str {
        "merge_facts"
    }

    async fn execute_guarded(&self, _context: &GuardedContext) -> CapabilityResult<()> {
        Ok(())
    }
}

/// Guarded effect for reading capabilities
struct ReadCapsEffect;

#[async_trait]
impl GuardedEffect for ReadCapsEffect {
    fn capability_requirements(&self) -> EffectRequirement {
        super::capability::JournalRequirements::read_caps()
    }

    fn operation_name(&self) -> &'static str {
        "read_caps"
    }

    async fn execute_guarded(&self, _context: &GuardedContext) -> CapabilityResult<()> {
        Ok(())
    }
}

/// Guarded effect for refining capabilities
struct RefineCapsEffect {
    constraint: Cap,
}

#[async_trait]
impl GuardedEffect for RefineCapsEffect {
    fn capability_requirements(&self) -> EffectRequirement {
        super::capability::JournalRequirements::refine_caps()
    }

    fn operation_name(&self) -> &'static str {
        "refine_caps"
    }

    async fn execute_guarded(&self, _context: &GuardedContext) -> CapabilityResult<()> {
        Ok(())
    }
}

// Implement JournalEffects for the middleware
#[async_trait]
impl<T: JournalEffects + Send + Sync> JournalEffects for CapabilityMiddleware<T> {
    async fn get_journal_state(&self) -> Result<crate::effects::journal::JournalMap, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_journal_state().await
    }

    async fn get_current_tree(&self) -> Result<crate::effects::journal::RatchetTree, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_current_tree().await
    }

    async fn get_tree_at_epoch(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<crate::effects::journal::RatchetTree, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_tree_at_epoch(epoch).await
    }

    async fn get_current_commitment(
        &self,
    ) -> Result<crate::effects::journal::Commitment, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_current_commitment().await
    }

    async fn get_latest_epoch(&self) -> Result<Option<crate::effects::journal::Epoch>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_latest_epoch().await
    }

    async fn append_tree_op(
        &self,
        op: crate::effects::journal::TreeOpRecord,
    ) -> Result<(), AuraError> {
        // Generate proper delta from tree op
        let tree_delta = create_tree_op_delta(&op);
        let effect = MergeFactsEffect {
            delta: tree_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.append_tree_op(op).await
    }

    async fn get_tree_op(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<Option<crate::effects::journal::TreeOpRecord>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_tree_op(epoch).await
    }

    async fn list_tree_ops(&self) -> Result<Vec<crate::effects::journal::TreeOpRecord>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.list_tree_ops().await
    }

    async fn submit_intent(
        &self,
        intent: crate::effects::journal::Intent,
    ) -> Result<crate::effects::journal::IntentId, AuraError> {
        // Generate proper delta from intent submission
        let intent_delta = create_intent_delta(&intent);
        let effect = MergeFactsEffect {
            delta: intent_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.submit_intent(intent).await
    }

    async fn get_intent(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<Option<crate::effects::journal::Intent>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_intent(intent_id).await
    }

    async fn get_intent_status(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<crate::effects::journal::IntentStatus, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_intent_status(intent_id).await
    }

    async fn list_pending_intents(
        &self,
    ) -> Result<Vec<crate::effects::journal::Intent>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.list_pending_intents().await
    }

    async fn tombstone_intent(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<(), AuraError> {
        // Generate proper delta from intent tombstone
        let tombstone_delta = create_tombstone_delta(&intent_id);
        let effect = MergeFactsEffect {
            delta: tombstone_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.tombstone_intent(intent_id).await
    }

    async fn prune_stale_intents(
        &self,
        current_commitment: crate::effects::journal::Commitment,
    ) -> Result<usize, AuraError> {
        // Generate proper delta from intent pruning
        let pruning_delta = create_pruning_delta(&current_commitment);
        let effect = MergeFactsEffect {
            delta: pruning_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.prune_stale_intents(current_commitment).await
    }

    async fn validate_capability(
        &self,
        capability: &crate::effects::journal::CapabilityRef,
    ) -> Result<bool, AuraError> {
        self.execute_guarded(ReadCapsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.validate_capability(capability).await
    }

    async fn is_capability_revoked(
        &self,
        capability_id: &crate::effects::journal::CapabilityId,
    ) -> Result<bool, AuraError> {
        self.execute_guarded(ReadCapsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.is_capability_revoked(capability_id).await
    }

    async fn list_capabilities_in_op(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<Vec<crate::effects::journal::CapabilityRef>, AuraError> {
        self.execute_guarded(ReadCapsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.list_capabilities_in_op(epoch).await
    }

    async fn merge_journal_state(
        &self,
        other: crate::effects::journal::JournalMap,
    ) -> Result<(), AuraError> {
        // Generate proper delta from journal state merge
        let merge_delta = create_journal_merge_delta(&other);
        let effect = MergeFactsEffect {
            delta: merge_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.merge_journal_state(other).await
    }

    async fn get_journal_stats(&self) -> Result<crate::effects::journal::JournalStats, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_journal_stats().await
    }

    async fn is_device_member(
        &self,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<bool, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.is_device_member(device_id).await
    }

    async fn get_device_leaf_index(
        &self,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<Option<crate::effects::journal::LeafIndex>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_device_leaf_index(device_id).await
    }

    async fn list_devices(&self) -> Result<Vec<aura_core::identifiers::DeviceId>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.list_devices().await
    }

    async fn list_guardians(&self) -> Result<Vec<aura_core::identifiers::GuardianId>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.list_guardians().await
    }

    async fn append_attested_tree_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError> {
        // Generate proper delta from attested operation
        let attested_delta = create_attested_op_delta(&op);
        let effect = MergeFactsEffect {
            delta: attested_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.append_attested_tree_op(op).await
    }

    async fn get_tree_state(&self) -> Result<aura_journal::ratchet_tree::TreeState, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_tree_state().await
    }

    async fn get_op_log(&self) -> Result<aura_journal::semilattice::OpLog, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_op_log().await
    }

    async fn merge_op_log(
        &self,
        remote: aura_journal::semilattice::OpLog,
    ) -> Result<(), AuraError> {
        // Generate proper delta from op log merge
        let oplog_delta = create_oplog_merge_delta(&remote);
        let effect = MergeFactsEffect {
            delta: oplog_delta,
        };
        self.execute_guarded(effect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.merge_op_log(remote).await
    }

    async fn get_attested_op(
        &self,
        cid: &aura_core::Hash32,
    ) -> Result<Option<aura_core::AttestedOp>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.get_attested_op(cid).await
    }

    async fn list_attested_ops(&self) -> Result<Vec<aura_core::AttestedOp>, AuraError> {
        self.execute_guarded(ReadFactsEffect)
            .await
            .map_err(|e| AuraError::internal(&format!("Capability check failed: {}", e)))?;

        self.inner.list_attested_ops().await
    }
}

/// Builder for creating capability middleware
pub struct CapabilityMiddlewareBuilder {
    journal: Option<Journal>,
    message_context: Option<MessageContext>,
    timestamp: Option<u64>,
    auth_level: Option<super::capability::AuthLevel>,
    enforcement_enabled: bool,
}

impl CapabilityMiddlewareBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            journal: None,
            message_context: None,
            timestamp: None,
            auth_level: None,
            enforcement_enabled: true,
        }
    }

    /// Set the journal for the context
    pub fn with_journal(mut self, journal: Journal) -> Self {
        self.journal = Some(journal);
        self
    }

    /// Set the message context
    pub fn with_message_context(mut self, context: MessageContext) -> Self {
        self.message_context = Some(context);
        self
    }

    /// Set the timestamp
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Set the authentication level
    pub fn with_auth_level(mut self, level: super::capability::AuthLevel) -> Self {
        self.auth_level = Some(level);
        self
    }

    /// Disable enforcement (for testing)
    pub fn disable_enforcement(mut self) -> Self {
        self.enforcement_enabled = false;
        self
    }

    /// Build the middleware around an effect handler
    pub fn build<T>(self, inner: T) -> Result<CapabilityMiddleware<T>, String> {
        let journal = self.journal.ok_or("Journal is required")?;
        let message_context = self.message_context.ok_or("Message context is required")?;
        let timestamp = self
            .timestamp
            .unwrap_or(aura_core::current_unix_timestamp());
        let auth_level = self
            .auth_level
            .unwrap_or(super::capability::AuthLevel::Device);

        let context = GuardedContext::new(journal, message_context, timestamp, auth_level);

        if self.enforcement_enabled {
            Ok(CapabilityMiddleware::new(inner, context))
        } else {
            Ok(CapabilityMiddleware::for_testing(inner, context))
        }
    }
}

impl Default for CapabilityMiddlewareBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate fact delta from tree operation
fn create_tree_op_delta(op: &crate::effects::journal::TreeOpRecord) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add tree operation fact
    delta.insert(
        "tree_op".to_string(),
        FactValue::String(format!("op:{}", op.0))
    );
    
    // Add timestamp
    delta.insert(
        "tree_op_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

/// Generate fact delta from intent submission
fn create_intent_delta(intent: &crate::effects::journal::Intent) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add intent submission fact
    delta.insert(
        "intent_submitted".to_string(),
        FactValue::String(format!("intent:{}", intent.0))
    );
    
    // Add timestamp
    delta.insert(
        "intent_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

/// Generate fact delta from intent tombstone
fn create_tombstone_delta(intent_id: &crate::effects::journal::IntentId) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add tombstone fact
    delta.insert(
        "intent_tombstoned".to_string(),
        FactValue::String(format!("tombstone:{}", intent_id.0))
    );
    
    // Add timestamp
    delta.insert(
        "tombstone_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

/// Generate fact delta from intent pruning
fn create_pruning_delta(commitment: &crate::effects::journal::Commitment) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add pruning fact
    delta.insert(
        "intents_pruned".to_string(),
        FactValue::String(format!("commitment:{:02x?}", &commitment.0[..8]))
    );
    
    // Add timestamp
    delta.insert(
        "pruning_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

/// Generate fact delta from journal state merge
fn create_journal_merge_delta(other: &crate::effects::journal::JournalMap) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add merge fact
    delta.insert(
        "journal_merged".to_string(),
        FactValue::String(format!("entries:{}", other.0.len()))
    );
    
    // Add timestamp
    delta.insert(
        "merge_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

/// Generate fact delta from attested operation
fn create_attested_op_delta(op: &aura_core::AttestedOp) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add attested operation fact
    delta.insert(
        "attested_op".to_string(),
        FactValue::String(format!("op_type:{:?}", op.operation_type()))
    );
    
    // Add commitment binding
    delta.insert(
        "op_commitment".to_string(),
        FactValue::String(format!("commitment:{:02x?}", &op.commitment_hash()[..8]))
    );
    
    // Add timestamp
    delta.insert(
        "attested_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

/// Generate fact delta from op log merge
fn create_oplog_merge_delta(remote: &aura_journal::semilattice::OpLog) -> Fact {
    use aura_core::FactValue;
    let mut delta = Fact::new();
    
    // Add op log merge fact
    delta.insert(
        "oplog_merged".to_string(),
        FactValue::String(format!("operations:{}", remote.len()))
    );
    
    // Add timestamp
    delta.insert(
        "oplog_merge_timestamp".to_string(),
        FactValue::Number(aura_core::current_unix_timestamp() as i64)
    );
    
    delta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::journal::JournalEffects;
    use crate::handlers::journal::memory::MemoryJournalHandler;
    use aura_core::{identifiers::DeviceId, FactValue};

    #[tokio::test]
    async fn test_capability_middleware_allows_sufficient_caps() {
        let caps = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
        ]);
        let journal = Journal::with_caps(caps);

        let inner_handler = InMemoryJournalHandler::new();
        let middleware = CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(MessageContext::dkd_context("test", [0u8; 32]))
            .with_auth_level(crate::guards::capability::AuthLevel::Device)
            .build(inner_handler)
            .unwrap();

        // Should allow reading facts
        let result = middleware.read_facts().await;
        assert!(result.is_ok());

        // Should allow merging facts
        let facts = Fact::with_value("test", FactValue::String("value".to_string()));
        let result = middleware.merge_facts(facts).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_capability_middleware_denies_insufficient_caps() {
        let caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let journal = Journal::with_caps(caps);

        let inner_handler = InMemoryJournalHandler::new();
        let middleware = CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(MessageContext::dkd_context("test", [0u8; 32]))
            .with_auth_level(crate::guards::capability::AuthLevel::Device)
            .build(inner_handler)
            .unwrap();

        // Should allow reading facts
        let result = middleware.read_facts().await;
        assert!(result.is_ok());

        // Should deny merging facts (insufficient permissions)
        let facts = Fact::with_value("test", FactValue::String("value".to_string()));
        let result = middleware.merge_facts(facts).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Capability check failed"));
    }

    #[tokio::test]
    async fn test_capability_middleware_testing_mode() {
        let caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let journal = Journal::with_caps(caps);

        let inner_handler = InMemoryJournalHandler::new();
        let middleware = CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(MessageContext::dkd_context("test", [0u8; 32]))
            .with_auth_level(crate::guards::capability::AuthLevel::Device)
            .disable_enforcement()
            .build(inner_handler)
            .unwrap();

        // Should allow both read and write in testing mode
        let result = middleware.read_facts().await;
        assert!(result.is_ok());

        let facts = Fact::with_value("test", FactValue::String("value".to_string()));
        let result = middleware.merge_facts(facts).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let journal = Journal::with_caps(caps);

        let inner_handler = InMemoryJournalHandler::new();
        let middleware = CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(MessageContext::dkd_context("test", [0u8; 32]))
            .with_auth_level(crate::guards::capability::AuthLevel::Device)
            .build(inner_handler)
            .unwrap();

        // Perform some operations
        let _ = middleware.read_facts().await;

        let facts = Fact::with_value("test", FactValue::String("value".to_string()));
        let _ = middleware.merge_facts(facts).await;

        // Check audit log
        let audit_log = middleware.audit_log().await;
        assert!(!audit_log.is_empty());
        assert!(audit_log.len() >= 2); // At least two operations logged
    }
}
