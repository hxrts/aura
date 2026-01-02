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
use aura_core::effects::RandomCoreEffects;
use aura_core::identifiers::{AuthorityId, RecoveryId};
use std::sync::Arc;

/// Recovery service API
///
/// Provides recovery operations through a clean public API.
#[derive(Clone)]
pub struct RecoveryServiceApi {
    handler: RecoveryHandler,
    effects: Arc<AuraEffectSystem>,
}

impl std::fmt::Debug for RecoveryServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryServiceApi").finish_non_exhaustive()
    }
}

impl RecoveryServiceApi {
    /// Create a new recovery service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
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
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        self.handler
            .initiate(
                &self.effects,
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
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        self.handler
            .initiate(
                &self.effects,
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
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        self.handler
            .initiate(
                &self.effects,
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
        new_threshold: u32,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        self.handler
            .initiate(
                &self.effects,
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
        self.handler.submit_approval(&self.effects, approval).await
    }

    /// Complete a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery to complete
    ///
    /// # Returns
    /// The recovery result
    pub async fn complete(&self, recovery_id: &RecoveryId) -> AgentResult<RecoveryResult> {
        self.handler.complete(&self.effects, recovery_id).await
    }

    /// Cancel a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery to cancel
    /// * `reason` - Reason for cancellation
    ///
    /// # Returns
    /// The recovery result
    pub async fn cancel(
        &self,
        recovery_id: &RecoveryId,
        reason: String,
    ) -> AgentResult<RecoveryResult> {
        self.handler
            .cancel(&self.effects, recovery_id, reason)
            .await
    }

    /// Get the state of a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery
    ///
    /// # Returns
    /// The recovery state if found
    pub async fn get_state(&self, recovery_id: &RecoveryId) -> Option<RecoveryState> {
        self.handler.get_state(recovery_id).await
    }

    /// List all active recovery ceremonies
    ///
    /// # Returns
    /// List of (recovery_id, state) pairs
    pub async fn list_active(&self) -> Vec<(RecoveryId, RecoveryState)> {
        self.handler.list_active().await
    }

    /// Check if a recovery is pending (initiated but not complete)
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery
    ///
    /// # Returns
    /// True if the recovery is in Initiated or CollectingShares state
    pub async fn is_pending(&self, recovery_id: &RecoveryId) -> bool {
        matches!(
            self.handler.get_state(recovery_id).await,
            Some(RecoveryState::Initiated { .. })
                | Some(RecoveryState::CollectingShares { .. })
                | Some(RecoveryState::Reconstructing { .. })
        )
    }

    /// Prepare guardian ceremony by generating FROST threshold keys
    ///
    /// This method generates new threshold keys for the guardian configuration
    /// at a new epoch. The keys are stored but not activated until the ceremony
    /// completes successfully.
    ///
    /// # Full Ceremony Flow
    ///
    /// For complete ceremony orchestration, use `RuntimeBridge.initiate_guardian_ceremony()`
    /// via the AppCore workflow layer. That method:
    /// 1. Calls this method to generate keys
    /// 2. Creates a ceremony ID and registers with CeremonyTracker
    /// 3. Sends guardian invitations via `send_guardian_invitation`
    /// 4. Tracks responses and commits/rollbacks the key rotation
    ///
    /// This method is exposed for advanced use cases where you need direct
    /// control over key generation separate from the invitation flow.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum number of guardians required (k-of-n)
    /// * `guardian_ids` - List of guardian authority IDs
    ///
    /// # Returns
    /// A tuple of (new_epoch, key_packages, public_key_package) on success.
    /// - `new_epoch`: The epoch for the new keys (call commit/rollback with this)
    /// - `key_packages`: Encrypted key packages for each guardian
    /// - `public_key_package`: The group public key for the new configuration
    pub async fn prepare_guardian_keys(
        &self,
        threshold_k: u16,
        guardian_ids: Vec<AuthorityId>,
    ) -> AgentResult<(u64, Vec<Vec<u8>>, Vec<u8>)> {
        use crate::core::AgentError;
        use aura_core::effects::ThresholdSigningEffects;

        let total_n = guardian_ids.len() as u16;

        if threshold_k == 0 {
            return Err(AgentError::invalid(
                "Threshold must be at least 1".to_string(),
            ));
        }

        if total_n < threshold_k {
            return Err(AgentError::invalid(format!(
                "Need at least {} guardians for {}-of-{} threshold",
                threshold_k, threshold_k, total_n
            )));
        }

        // Get effect system read lock
        let authority_id = self.handler.authority_context().authority_id();

        let participants: Vec<aura_core::threshold::ParticipantIdentity> = guardian_ids
            .iter()
            .copied()
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect();

        // Generate new threshold keys
        let (new_epoch, key_packages, public_key) = self
            .effects
            .rotate_keys(&authority_id, threshold_k, total_n, &participants)
            .await
            .map_err(|e| {
                AgentError::internal(format!("Failed to generate threshold keys: {}", e))
            })?;

        tracing::info!(
            authority_id = %authority_id,
            new_epoch,
            threshold_k,
            total_n,
            num_key_packages = key_packages.len(),
            public_key_size = public_key.len(),
            "Generated new guardian threshold keys"
        );

        Ok((new_epoch, key_packages, public_key))
    }

    /// Commit a guardian key rotation after successful ceremony
    ///
    /// Call this after all guardians have accepted and stored their key shares.
    /// This makes the new epoch authoritative.
    pub async fn commit_guardian_keys(&self, new_epoch: u64) -> AgentResult<()> {
        use crate::core::{default_context_id_for_authority, AgentError};
        use aura_core::effects::ThresholdSigningEffects;
        use aura_core::threshold::{policy_for, CeremonyFlow, KeyGenerationPolicy};

        let authority_id = self.handler.authority_context().authority_id();
        let policy = policy_for(CeremonyFlow::GuardianSetupRotation);
        if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg {
            let context_id = default_context_id_for_authority(authority_id);
            let has_commit = self
                .effects
                .has_dkg_transcript_commit(authority_id, context_id, new_epoch)
                .await
                .map_err(|e| {
                    AgentError::internal(format!("Failed to verify DKG transcript commit: {e}"))
                })?;
            if !has_commit {
                return Err(AgentError::invalid(
                    "Missing consensus DKG transcript".to_string(),
                ));
            }
        }

        self.effects
            .commit_key_rotation(&authority_id, new_epoch)
            .await
            .map_err(|e| AgentError::internal(format!("Failed to commit key rotation: {}", e)))?;

        tracing::info!(
            authority_id = %authority_id,
            epoch = new_epoch,
            "Committed guardian key rotation"
        );

        Ok(())
    }

    /// Rollback a guardian key rotation after ceremony failure
    ///
    /// Call this when the ceremony fails (guardian declined, user cancelled, or timeout).
    /// This discards the new epoch's keys and keeps the previous configuration active.
    pub async fn rollback_guardian_keys(&self, failed_epoch: u64) -> AgentResult<()> {
        use crate::core::AgentError;
        use aura_core::effects::ThresholdSigningEffects;

        let authority_id = self.handler.authority_context().authority_id();

        self.effects
            .rollback_key_rotation(&authority_id, failed_epoch)
            .await
            .map_err(|e| AgentError::internal(format!("Failed to rollback key rotation: {}", e)))?;

        tracing::info!(
            authority_id = %authority_id,
            epoch = failed_epoch,
            "Rolled back guardian key rotation"
        );

        Ok(())
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
    ///
    /// # Parameters
    /// * `guardian_id` - The specific guardian to send this invitation to
    /// * `ceremony_id` - The ceremony identifier string
    /// * `threshold_k` - Minimum signers required
    /// * `total_n` - Total number of guardians
    /// * `all_guardian_ids` - All guardian authority IDs participating in the ceremony
    /// * `new_epoch` - The epoch for the new key rotation
    /// * `key_package` - Encrypted key package for this guardian
    pub async fn send_guardian_invitation(
        &self,
        guardian_authority: AuthorityId,
        ceremony_id: aura_recovery::CeremonyId,
        prestate_hash: aura_core::Hash32,
        operation: aura_recovery::GuardianRotationOp,
        key_package: &[u8],
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        use aura_core::effects::TransportEffects;
        use aura_core::ContextId;
        use aura_recovery::guardian_ceremony::CeremonyProposal;

        tracing::info!(
            guardian_id = %guardian_authority,
            ceremony_id = %ceremony_id,
            threshold_k = operation.threshold_k,
            total_n = operation.total_n,
            key_package_size = key_package.len(),
            "Sending guardian invitation through transport"
        );

        // Get our authority context for source
        let initiator_id = self.handler.authority_context().authority_id();

        // Create a context ID for guardian ceremonies
        // Use a deterministic derivation from initiator + ceremony ID
        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"GUARDIAN_CEREMONY_CONTEXT");
            h.update(&initiator_id.to_bytes());
            h.update(&ceremony_id.0 .0);
            h.finalize()
        };
        let ceremony_context = ContextId::new_from_entropy(context_entropy);

        // Best-effort encryption envelope fields (actual key agreement is wired separately).
        let nonce_bytes = self.effects.random_bytes(12).await;
        let mut encryption_nonce = [0u8; 12];
        encryption_nonce.copy_from_slice(&nonce_bytes[..12]);
        let ephemeral_public_key = self.effects.random_bytes(32).await;

        // Create the ceremony proposal
        let proposal = CeremonyProposal {
            ceremony_id,
            initiator_id,
            prestate_hash,
            operation,
            encrypted_key_package: key_package.to_vec(),
            encryption_nonce,
            ephemeral_public_key,
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
        self.effects
            .send_envelope(envelope)
            .await
            .map_err(|e| AgentError::effects(format!("Failed to send invitation: {}", e)))?;

        tracing::info!(
            guardian_id = %guardian_authority,
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
    pub async fn process_guardian_acceptances(&self) -> AgentResult<Vec<(String, String)>> {
        use aura_core::effects::TransportEffects;

        let mut acceptances = Vec::new();

        // Poll for incoming acceptance messages
        loop {
            match self.effects.receive_envelope().await {
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
    use crate::core::AgentConfig;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    #[tokio::test]
    async fn test_recovery_service_creation() {
        let authority_context = create_test_authority(150);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RecoveryServiceApi::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_add_device_recovery() {
        let authority_context = create_test_authority(151);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

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

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert_eq!(request.threshold, 2);
    }

    #[tokio::test]
    async fn test_remove_device_recovery() {
        let authority_context = create_test_authority(154);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([155u8; 32])];

        let request = service
            .remove_device(0, guardians, 1, "Device compromised".to_string(), None)
            .await
            .unwrap();

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
    }

    #[tokio::test]
    async fn test_replace_tree_recovery() {
        let authority_context = create_test_authority(156);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

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

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert!(request.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_update_guardians_recovery() {
        let authority_context = create_test_authority(160);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

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

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
    }

    #[tokio::test]
    async fn test_full_recovery_flow() {
        let authority_context = create_test_authority(164);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

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
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

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
