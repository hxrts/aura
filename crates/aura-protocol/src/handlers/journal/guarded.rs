//! Capability-guarded journal effect handlers
//!
//! This module provides journal effect handlers that enforce the non-interference
//! property from the formal specification through capability-based access control.

use crate::effects::journal::JournalEffects;
use crate::guards::{CapabilityMiddleware, CapabilityMiddlewareBuilder, GuardedContext};
use crate::handlers::journal::memory::MemoryJournalHandler;
use async_trait::async_trait;
use aura_core::{AuraError, Journal, MessageContext};

/// Factory for creating capability-guarded journal handlers
pub struct GuardedJournalHandlerFactory;

impl GuardedJournalHandlerFactory {
    /// Create a new guarded journal handler with the given context
    pub fn create_with_context(
        journal: Journal,
        message_context: MessageContext,
        auth_level: crate::guards::capability::AuthLevel,
    ) -> Result<CapabilityMiddleware<MemoryJournalHandler>, String> {
        let inner_handler = MemoryJournalHandler::new();

        CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(message_context)
            .with_auth_level(auth_level)
            .build(inner_handler)
    }

    /// Create a guarded handler for testing (enforcement disabled)
    pub fn create_for_testing(
        journal: Journal,
        message_context: MessageContext,
    ) -> Result<CapabilityMiddleware<MemoryJournalHandler>, String> {
        let inner_handler = MemoryJournalHandler::new();

        CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(message_context)
            .with_auth_level(crate::guards::capability::AuthLevel::Device)
            .disable_enforcement()
            .build(inner_handler)
    }

    /// Create a guarded handler with admin privileges
    pub fn create_admin_handler(
        journal: Journal,
        message_context: MessageContext,
    ) -> Result<CapabilityMiddleware<MemoryJournalHandler>, String> {
        let inner_handler = MemoryJournalHandler::new();

        CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(message_context)
            .with_auth_level(crate::guards::capability::AuthLevel::Threshold)
            .build(inner_handler)
    }

    /// Create a guarded handler with device-level authentication
    pub fn create_device_handler(
        journal: Journal,
        message_context: MessageContext,
    ) -> Result<CapabilityMiddleware<MemoryJournalHandler>, String> {
        let inner_handler = MemoryJournalHandler::new();

        CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(message_context)
            .with_auth_level(crate::guards::capability::AuthLevel::Device)
            .build(inner_handler)
    }

    /// Create a guarded handler with multi-factor authentication
    pub fn create_multifactor_handler(
        journal: Journal,
        message_context: MessageContext,
    ) -> Result<CapabilityMiddleware<MemoryJournalHandler>, String> {
        let inner_handler = MemoryJournalHandler::new();

        CapabilityMiddlewareBuilder::new()
            .with_journal(journal)
            .with_message_context(message_context)
            .with_auth_level(crate::guards::capability::AuthLevel::MultiFactor)
            .build(inner_handler)
    }
}

/// Specialized guarded journal handler for protocol contexts
///
/// This handler enforces context-specific capability requirements and provides
/// protocol-aware access control for different types of operations.
pub struct ProtocolJournalHandler {
    /// The underlying guarded handler
    handler: CapabilityMiddleware<MemoryJournalHandler>,
    /// Protocol context information
    protocol_context: ProtocolContext,
}

/// Context information for protocol execution
#[derive(Debug, Clone)]
pub struct ProtocolContext {
    /// The protocol being executed
    pub protocol_name: String,
    /// The role of this device in the protocol
    pub device_role: String,
    /// Phase of protocol execution
    pub phase: String,
    /// Required capabilities for this context
    pub required_capabilities: Vec<String>,
}

impl ProtocolJournalHandler {
    /// Create a new protocol-specific journal handler
    pub fn new(
        journal: Journal,
        message_context: MessageContext,
        auth_level: crate::guards::capability::AuthLevel,
        protocol_context: ProtocolContext,
    ) -> Result<Self, String> {
        let handler = GuardedJournalHandlerFactory::create_with_context(
            journal,
            message_context,
            auth_level,
        )?;

        Ok(Self {
            handler,
            protocol_context,
        })
    }

    /// Get the protocol context
    pub fn protocol_context(&self) -> &ProtocolContext {
        &self.protocol_context
    }

    /// Check if the current context allows a specific operation
    pub async fn check_operation_allowed(&self, operation: &str) -> bool {
        self.protocol_context
            .required_capabilities
            .iter()
            .any(|cap| cap.contains(operation))
    }

    /// Get audit log from the underlying handler
    pub async fn audit_log(&self) -> Vec<crate::guards::capability::CapabilityAuditEntry> {
        self.handler.audit_log().await
    }
}

#[async_trait]
impl JournalEffects for ProtocolJournalHandler {
    async fn get_journal_state(&self) -> Result<crate::effects::journal::JournalMap, AuraError> {
        self.handler.get_journal_state().await
    }

    async fn get_current_tree(&self) -> Result<crate::effects::journal::RatchetTree, AuraError> {
        self.handler.get_current_tree().await
    }

    async fn get_tree_at_epoch(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<crate::effects::journal::RatchetTree, AuraError> {
        self.handler.get_tree_at_epoch(epoch).await
    }

    async fn get_current_commitment(
        &self,
    ) -> Result<crate::effects::journal::Commitment, AuraError> {
        self.handler.get_current_commitment().await
    }

    async fn get_latest_epoch(&self) -> Result<Option<crate::effects::journal::Epoch>, AuraError> {
        self.handler.get_latest_epoch().await
    }

    async fn append_tree_op(
        &self,
        op: crate::effects::journal::TreeOpRecord,
    ) -> Result<(), AuraError> {
        self.handler.append_tree_op(op).await
    }

    async fn get_tree_op(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<Option<crate::effects::journal::TreeOpRecord>, AuraError> {
        self.handler.get_tree_op(epoch).await
    }

    async fn list_tree_ops(&self) -> Result<Vec<crate::effects::journal::TreeOpRecord>, AuraError> {
        self.handler.list_tree_ops().await
    }

    async fn submit_intent(
        &self,
        intent: crate::effects::journal::Intent,
    ) -> Result<crate::effects::journal::IntentId, AuraError> {
        self.handler.submit_intent(intent).await
    }

    async fn get_intent(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<Option<crate::effects::journal::Intent>, AuraError> {
        self.handler.get_intent(intent_id).await
    }

    async fn get_intent_status(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<crate::effects::journal::IntentStatus, AuraError> {
        self.handler.get_intent_status(intent_id).await
    }

    async fn list_pending_intents(
        &self,
    ) -> Result<Vec<crate::effects::journal::Intent>, AuraError> {
        self.handler.list_pending_intents().await
    }

    async fn tombstone_intent(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<(), AuraError> {
        self.handler.tombstone_intent(intent_id).await
    }

    async fn prune_stale_intents(
        &self,
        current_commitment: crate::effects::journal::Commitment,
    ) -> Result<usize, AuraError> {
        self.handler.prune_stale_intents(current_commitment).await
    }

    async fn validate_capability(
        &self,
        capability: &crate::effects::journal::CapabilityRef,
    ) -> Result<bool, AuraError> {
        self.handler.validate_capability(capability).await
    }

    async fn is_capability_revoked(
        &self,
        capability_id: &crate::effects::journal::CapabilityId,
    ) -> Result<bool, AuraError> {
        self.handler.is_capability_revoked(capability_id).await
    }

    async fn list_capabilities_in_op(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<Vec<crate::effects::journal::CapabilityRef>, AuraError> {
        self.handler.list_capabilities_in_op(epoch).await
    }

    async fn merge_journal_state(
        &self,
        other: crate::effects::journal::JournalMap,
    ) -> Result<(), AuraError> {
        self.handler.merge_journal_state(other).await
    }

    async fn get_journal_stats(&self) -> Result<crate::effects::journal::JournalStats, AuraError> {
        self.handler.get_journal_stats().await
    }

    async fn is_device_member(
        &self,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<bool, AuraError> {
        self.handler.is_device_member(device_id).await
    }

    async fn get_device_leaf_index(
        &self,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<Option<crate::effects::journal::LeafIndex>, AuraError> {
        self.handler.get_device_leaf_index(device_id).await
    }

    async fn list_devices(&self) -> Result<Vec<aura_core::identifiers::DeviceId>, AuraError> {
        self.handler.list_devices().await
    }

    async fn list_guardians(&self) -> Result<Vec<aura_core::identifiers::GuardianId>, AuraError> {
        self.handler.list_guardians().await
    }

    async fn append_attested_tree_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError> {
        self.handler.append_attested_tree_op(op).await
    }

    async fn get_tree_state(&self) -> Result<aura_journal::ratchet_tree::TreeState, AuraError> {
        self.handler.get_tree_state().await
    }

    async fn get_op_log(&self) -> Result<aura_journal::semilattice::OpLog, AuraError> {
        self.handler.get_op_log().await
    }

    async fn merge_op_log(
        &self,
        remote: aura_journal::semilattice::OpLog,
    ) -> Result<(), AuraError> {
        self.handler.merge_op_log(remote).await
    }

    async fn get_attested_op(
        &self,
        cid: &aura_core::Hash32,
    ) -> Result<Option<aura_core::AttestedOp>, AuraError> {
        self.handler.get_attested_op(cid).await
    }

    async fn list_attested_ops(&self) -> Result<Vec<aura_core::AttestedOp>, AuraError> {
        self.handler.list_attested_ops().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{identifiers::DeviceId, Cap, FactValue};

    #[tokio::test]
    async fn test_guarded_journal_handler_creation() {
        let caps = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
        ]);
        let journal = Journal::with_caps(caps);
        let context = MessageContext::dkd_context("test", [0u8; 32]);

        let handler = GuardedJournalHandlerFactory::create_with_context(
            journal,
            context,
            crate::guards::capability::AuthLevel::Device,
        );

        assert!(handler.is_ok());
    }

    #[tokio::test]
    async fn test_protocol_journal_handler() {
        let caps = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
            "protocol:dkd".to_string(),
        ]);
        let journal = Journal::with_caps(caps);
        let context = MessageContext::dkd_context("test", [0u8; 32]);

        let protocol_context = ProtocolContext {
            protocol_name: "DKD".to_string(),
            device_role: "participant".to_string(),
            phase: "key_generation".to_string(),
            required_capabilities: vec!["journal:read".to_string(), "protocol:dkd".to_string()],
        };

        let handler = ProtocolJournalHandler::new(
            journal,
            context,
            crate::guards::capability::AuthLevel::Device,
            protocol_context,
        );

        assert!(handler.is_ok());
        let handler = handler.unwrap();

        // Should allow operations specified in the protocol context
        assert!(handler.check_operation_allowed("read").await);
        assert!(handler.check_operation_allowed("dkd").await);
        assert!(!handler.check_operation_allowed("admin").await);
    }

    #[tokio::test]
    async fn test_different_auth_levels() {
        let caps = Cap::with_permissions(vec!["journal:admin".to_string()]);
        let journal = Journal::with_caps(caps);
        let context = MessageContext::dkd_context("test", [0u8; 32]);

        // Test device level
        let device_handler =
            GuardedJournalHandlerFactory::create_device_handler(journal.clone(), context.clone());
        assert!(device_handler.is_ok());

        // Test multifactor level
        let mf_handler = GuardedJournalHandlerFactory::create_multifactor_handler(
            journal.clone(),
            context.clone(),
        );
        assert!(mf_handler.is_ok());

        // Test admin level
        let admin_handler = GuardedJournalHandlerFactory::create_admin_handler(journal, context);
        assert!(admin_handler.is_ok());
    }

    #[tokio::test]
    async fn test_testing_handler_bypasses_enforcement() {
        let caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let journal = Journal::with_caps(caps);
        let context = MessageContext::dkd_context("test", [0u8; 32]);

        let handler = GuardedJournalHandlerFactory::create_for_testing(journal, context).unwrap();

        // Should work even with insufficient permissions in testing mode
        let journal_map = crate::effects::journal::JournalMap(std::collections::HashMap::new());
        let result = handler.merge_journal_state(journal_map).await;
        assert!(result.is_ok());
    }
}
