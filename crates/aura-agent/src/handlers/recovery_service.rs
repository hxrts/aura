//! Recovery Service - Public API for Recovery Operations
//!
//! Provides a clean public interface for guardian-based recovery operations.
//! Wraps `RecoveryHandler` with ergonomic methods and proper error handling.

use super::recovery::{
    GuardianApproval, RecoveryHandler, RecoveryOperation, RecoveryRequest, RecoveryResult,
    RecoveryState,
};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::identifiers::AuthorityId;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Recovery service
///
/// Provides recovery operations through a clean public API.
pub struct RecoveryService {
    handler: RecoveryHandler,
    effects: Arc<RwLock<AuraEffectSystem>>,
}

impl RecoveryService {
    /// Create a new recovery service
    pub fn new(
        effects: Arc<RwLock<AuraEffectSystem>>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = RecoveryHandler::new(authority_context)?;
        Ok(Self { handler, effects })
    }

    /// Initiate a recovery ceremony to add a new device
    ///
    /// # Arguments
    /// * `device_public_key` - Public key of the new device
    /// * `guardians` - Guardian authorities to request approval from
    /// * `threshold` - Required number of approvals
    /// * `justification` - Reason for recovery
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn add_device(
        &self,
        device_public_key: Vec<u8>,
        guardians: Vec<AuthorityId>,
        threshold: usize,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let effects = self.effects.read().await;
        self.handler
            .initiate(
                &effects,
                RecoveryOperation::AddDevice { device_public_key },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await
    }

    /// Initiate a recovery ceremony to remove a compromised device
    ///
    /// # Arguments
    /// * `leaf_index` - Leaf index of device to remove
    /// * `guardians` - Guardian authorities to request approval from
    /// * `threshold` - Required number of approvals
    /// * `justification` - Reason for recovery
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn remove_device(
        &self,
        leaf_index: u32,
        guardians: Vec<AuthorityId>,
        threshold: usize,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let effects = self.effects.read().await;
        self.handler
            .initiate(
                &effects,
                RecoveryOperation::RemoveDevice { leaf_index },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await
    }

    /// Initiate a full tree replacement recovery
    ///
    /// # Arguments
    /// * `new_public_key` - New public key for the recovered tree
    /// * `guardians` - Guardian authorities to request approval from
    /// * `threshold` - Required number of approvals
    /// * `justification` - Reason for recovery
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn replace_tree(
        &self,
        new_public_key: Vec<u8>,
        guardians: Vec<AuthorityId>,
        threshold: usize,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let effects = self.effects.read().await;
        self.handler
            .initiate(
                &effects,
                RecoveryOperation::ReplaceTree { new_public_key },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await
    }

    /// Initiate a guardian set update
    ///
    /// # Arguments
    /// * `new_guardians` - New guardian authorities
    /// * `new_threshold` - New threshold
    /// * `guardians` - Current guardian authorities to request approval from
    /// * `threshold` - Required number of approvals from current guardians
    /// * `justification` - Reason for update
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn update_guardians(
        &self,
        new_guardians: Vec<AuthorityId>,
        new_threshold: usize,
        guardians: Vec<AuthorityId>,
        threshold: usize,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let effects = self.effects.read().await;
        self.handler
            .initiate(
                &effects,
                RecoveryOperation::UpdateGuardians {
                    new_guardians,
                    new_threshold,
                },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await
    }

    /// Submit a guardian approval for an active recovery
    ///
    /// # Arguments
    /// * `approval` - The guardian approval
    ///
    /// # Returns
    /// The updated recovery state
    pub async fn submit_approval(&self, approval: GuardianApproval) -> AgentResult<RecoveryState> {
        let effects = self.effects.read().await;
        self.handler.submit_approval(&effects, approval).await
    }

    /// Complete a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery to complete
    ///
    /// # Returns
    /// The recovery result
    pub async fn complete(&self, recovery_id: &str) -> AgentResult<RecoveryResult> {
        let effects = self.effects.read().await;
        self.handler.complete(&effects, recovery_id).await
    }

    /// Cancel a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery to cancel
    /// * `reason` - Reason for cancellation
    ///
    /// # Returns
    /// The recovery result
    pub async fn cancel(&self, recovery_id: &str, reason: String) -> AgentResult<RecoveryResult> {
        let effects = self.effects.read().await;
        self.handler.cancel(&effects, recovery_id, reason).await
    }

    /// Get the state of a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery
    ///
    /// # Returns
    /// The recovery state if found
    pub async fn get_state(&self, recovery_id: &str) -> Option<RecoveryState> {
        self.handler.get_state(recovery_id).await
    }

    /// List all active recovery ceremonies
    ///
    /// # Returns
    /// List of (recovery_id, state) pairs
    pub async fn list_active(&self) -> Vec<(String, RecoveryState)> {
        self.handler.list_active().await
    }

    /// Check if a recovery is pending (initiated but not complete)
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery
    ///
    /// # Returns
    /// True if the recovery is in Initiated or CollectingShares state
    pub async fn is_pending(&self, recovery_id: &str) -> bool {
        matches!(
            self.handler.get_state(recovery_id).await,
            Some(RecoveryState::Initiated { .. })
                | Some(RecoveryState::CollectingShares { .. })
                | Some(RecoveryState::Reconstructing { .. })
        )
    }

    /// Initiate a full guardian ceremony with FROST key generation
    ///
    /// NOTE: This method is a placeholder for future integration.
    /// Currently, ceremony initiation happens via send_guardian_invitation
    /// and CeremonyTracker tracks progress. Full FROST key generation
    /// integration requires architectural refactoring to pass
    /// Arc<AuraEffectSystem> instead of Arc<RwLock<AuraEffectSystem>>.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum number of guardians required (k-of-n)
    /// * `guardian_ids` - List of guardian authority IDs
    ///
    /// # Returns
    /// Error indicating this method is not yet fully integrated
    pub async fn initiate_guardian_ceremony(
        &self,
        _threshold_k: u16,
        _guardian_ids: Vec<AuthorityId>,
    ) -> AgentResult<String> {
        use crate::core::AgentError;

        // TODO: Implement full ceremony initiation with GuardianCeremonyExecutor
        // This requires refactoring to handle Arc<RwLock<AuraEffectSystem>> vs Arc<AuraEffectSystem>
        Err(AgentError::internal(
            "Full guardian ceremony initiation not yet integrated - use send_guardian_invitation for now".to_string()
        ))
    }

    /// Send a guardian invitation with key package
    ///
    /// This routes the guardian invitation through the proper aura-recovery
    /// protocol. The invitation includes an encrypted key package for the
    /// guardian to store securely.
    ///
    /// # Arguments
    /// * `guardian_id` - Contact ID of the guardian to invite
    /// * `ceremony_id` - Unique ceremony identifier
    /// * `threshold_k` - Minimum signers required
    /// * `total_n` - Total number of guardians
    /// * `key_package` - Encrypted FROST key package for this guardian
    ///
    /// # Returns
    /// Ok if invitation was sent successfully
    ///
    /// # Protocol Flow
    /// 1. Creates CeremonyProposal message with key package
    /// 2. Serializes and wraps in TransportEnvelope
    /// 3. Sends via TransportEffects to guardian's authority
    /// 4. Guardian receives via their effect system
    /// 5. Guardian processes through guard chain + journal
    /// 6. GuardianBinding fact committed when accepted
    pub async fn send_guardian_invitation(
        &self,
        guardian_id: &str,
        ceremony_id: &str,
        threshold_k: u16,
        total_n: u16,
        key_package: &[u8],
    ) -> AgentResult<()> {
        use crate::core::AgentError;
        use aura_core::effects::TransportEffects;
        use aura_core::{hash::hash, ContextId, Hash32};
        use aura_recovery::guardian_ceremony::CeremonyProposal;
        use aura_recovery::{CeremonyId, GuardianRotationOp};

        tracing::info!(
            guardian_id = %guardian_id,
            ceremony_id = %ceremony_id,
            threshold_k,
            total_n,
            key_package_size = key_package.len(),
            "Sending guardian invitation through transport"
        );

        // Parse ceremony ID (format: "ceremony-{epoch}-{uuid}")
        let ceremony_id_bytes = if ceremony_id.starts_with("ceremony-") {
            // Extract hash from ceremony ID
            let parts: Vec<&str> = ceremony_id.split('-').collect();
            if parts.len() >= 3 {
                // Use the UUID part to derive a ceremony hash
                let uuid_str = parts[2];
                Hash32(hash(uuid_str.as_bytes()))
            } else {
                Hash32(hash(ceremony_id.as_bytes()))
            }
        } else {
            Hash32(hash(ceremony_id.as_bytes()))
        };
        let ceremony_id_hash = CeremonyId(ceremony_id_bytes);

        // Parse guardian authority ID from string
        let guardian_authority: AuthorityId = guardian_id
            .parse()
            .map_err(|e| AgentError::invalid(format!("Invalid guardian ID: {}", e)))?;

        // Get our authority context for source
        let initiator_id = self.handler.authority_context().authority_id;

        // Create a context ID for guardian ceremonies
        // Use a deterministic derivation from initiator + ceremony ID
        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"GUARDIAN_CEREMONY_CONTEXT");
            h.update(&initiator_id.to_bytes());
            h.update(&ceremony_id_hash.0 .0);
            h.finalize()
        };
        let ceremony_context = ContextId::new_from_entropy(context_entropy);

        // Create the rotation operation
        // Note: We don't have full guardian list here, just the recipient
        // In a full implementation, this would include all guardian IDs
        let operation = GuardianRotationOp {
            threshold_k,
            total_n,
            guardian_ids: vec![guardian_authority], // Simplified for now
            new_epoch: 1,                           // Will be updated by actual ceremony state
        };

        // Create the ceremony proposal
        let proposal = CeremonyProposal {
            ceremony_id: ceremony_id_hash,
            initiator_id,
            prestate_hash: Hash32([0u8; 32]), // Will be computed from actual guardian state
            operation,
            encrypted_key_package: key_package.to_vec(),
            encryption_nonce: [0u8; 12], // Should use actual encryption nonce
            ephemeral_public_key: vec![], // Should include ephemeral key for key agreement
        };

        // Serialize the proposal
        let payload = serde_json::to_vec(&proposal)
            .map_err(|e| AgentError::internal(format!("Failed to serialize proposal: {}", e)))?;

        // Create transport envelope
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-guardian-proposal".to_string(),
        );
        metadata.insert("protocol-version".to_string(), "1".to_string());
        metadata.insert("ceremony-id".to_string(), ceremony_id.to_string());

        let envelope = aura_core::effects::TransportEnvelope {
            destination: guardian_authority,
            source: initiator_id,
            context: ceremony_context,
            payload,
            metadata,
            receipt: None, // Receipts would be added by guard chain in production
        };

        // Send via transport effects
        let effects = self.effects.read().await;
        effects
            .send_envelope(envelope)
            .await
            .map_err(|e| AgentError::effects(format!("Failed to send invitation: {}", e)))?;

        tracing::info!(
            guardian_id = %guardian_id,
            ceremony_id = %ceremony_id,
            "Guardian invitation sent successfully"
        );

        Ok(())
    }

    /// Process incoming guardian acceptance responses from transport
    ///
    /// This method should be called periodically to check for acceptance messages
    /// from guardians. The caller should update the ceremony tracker with the results.
    ///
    /// # Returns
    /// List of (ceremony_id, guardian_id) pairs for accepted guardians
    pub async fn process_guardian_acceptances(
        &self,
    ) -> AgentResult<Vec<(String, String)>> {
        use aura_core::effects::TransportEffects;

        let effects = self.effects.read().await;
        let mut acceptances = Vec::new();

        // Poll for incoming acceptance messages
        loop {
            match effects.receive_envelope().await {
                Ok(envelope) => {
                    // Check if this is a guardian acceptance response
                    if envelope.metadata.get("content-type")
                        == Some(&"application/aura-guardian-acceptance".to_string())
                    {
                        if let (Some(ceremony_id), Some(guardian_id)) = (
                            envelope.metadata.get("ceremony-id"),
                            envelope.metadata.get("guardian-id"),
                        ) {
                            tracing::info!(
                                ceremony_id = %ceremony_id,
                                guardian_id = %guardian_id,
                                "Received guardian acceptance"
                            );

                            acceptances.push((ceremony_id.clone(), guardian_id.clone()));
                        }
                    }
                }
                Err(aura_core::effects::TransportError::NoMessage) => {
                    // No more messages
                    break;
                }
                Err(e) => {
                    tracing::warn!("Error receiving acceptance response: {}", e);
                    break;
                }
            }
        }

        Ok(acceptances)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use aura_core::identifiers::ContextId;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([seed.wrapping_add(100); 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        authority_context
    }

    #[tokio::test]
    async fn test_recovery_service_creation() {
        let authority_context = create_test_authority(150);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));

        let service = RecoveryService::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_add_device_recovery() {
        let authority_context = create_test_authority(151);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = RecoveryService::new(effects, authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([152u8; 32]),
            AuthorityId::new_from_entropy([153u8; 32]),
        ];

        let request = service
            .add_device(
                vec![0u8; 32],
                guardians,
                2,
                "Adding backup device".to_string(),
                None,
            )
            .await
            .unwrap();

        assert!(request.recovery_id.starts_with("recovery-"));
        assert_eq!(request.threshold, 2);
    }

    #[tokio::test]
    async fn test_remove_device_recovery() {
        let authority_context = create_test_authority(154);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = RecoveryService::new(effects, authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([155u8; 32])];

        let request = service
            .remove_device(0, guardians, 1, "Device compromised".to_string(), None)
            .await
            .unwrap();

        assert!(request.recovery_id.starts_with("recovery-"));
    }

    #[tokio::test]
    async fn test_replace_tree_recovery() {
        let authority_context = create_test_authority(156);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = RecoveryService::new(effects, authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([157u8; 32]),
            AuthorityId::new_from_entropy([158u8; 32]),
            AuthorityId::new_from_entropy([159u8; 32]),
        ];

        let request = service
            .replace_tree(
                vec![0u8; 32],
                guardians,
                2,
                "Full recovery after device loss".to_string(),
                Some(604800000), // 1 week
            )
            .await
            .unwrap();

        assert!(request.recovery_id.starts_with("recovery-"));
        assert!(request.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_update_guardians_recovery() {
        let authority_context = create_test_authority(160);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = RecoveryService::new(effects, authority_context).unwrap();

        let current_guardians = vec![AuthorityId::new_from_entropy([161u8; 32])];
        let new_guardians = vec![
            AuthorityId::new_from_entropy([162u8; 32]),
            AuthorityId::new_from_entropy([163u8; 32]),
        ];

        let request = service
            .update_guardians(
                new_guardians,
                2, // new threshold
                current_guardians,
                1, // current threshold
                "Upgrading guardian set".to_string(),
                None,
            )
            .await
            .unwrap();

        assert!(request.recovery_id.starts_with("recovery-"));
    }

    #[tokio::test]
    async fn test_full_recovery_flow() {
        let authority_context = create_test_authority(164);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = RecoveryService::new(effects, authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([165u8; 32])];

        // Initiate
        let request = service
            .add_device(
                vec![0u8; 32],
                guardians.clone(),
                1,
                "Test".to_string(),
                None,
            )
            .await
            .unwrap();

        // Check pending
        assert!(service.is_pending(&request.recovery_id).await);

        // Submit approval
        let approval = GuardianApproval {
            recovery_id: request.recovery_id.clone(),
            guardian_id: guardians[0],
            signature: vec![0u8; 64],
            share_data: None,
            approved_at: 12345,
        };
        service.submit_approval(approval).await.unwrap();

        // Complete
        let result = service.complete(&request.recovery_id).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_list_active() {
        let authority_context = create_test_authority(166);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = RecoveryService::new(effects, authority_context).unwrap();

        // Initially empty
        let active = service.list_active().await;
        assert!(active.is_empty());

        // Create recoveries
        let guardians = vec![
            AuthorityId::new_from_entropy([167u8; 32]),
            AuthorityId::new_from_entropy([168u8; 32]),
        ];

        service
            .add_device(
                vec![0u8; 32],
                guardians.clone(),
                2,
                "Test 1".to_string(),
                None,
            )
            .await
            .unwrap();

        service
            .remove_device(0, guardians, 2, "Test 2".to_string(), None)
            .await
            .unwrap();

        // Should have 2 active
        let active = service.list_active().await;
        assert_eq!(active.len(), 2);
    }
}
