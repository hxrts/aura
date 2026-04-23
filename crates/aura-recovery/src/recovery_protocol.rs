//! Recovery Protocol using Relational Contexts
//!
//! This module implements the recovery protocol using RelationalContexts,
//! replacing the device-centric recovery model with authority-based recovery.

use crate::facts::RecoveryFact;
use crate::recovery_approval::{
    recovery_operation_hash, verify_recovery_approval_signature, RecoveryApprovalTranscriptPayload,
};
use crate::utils::workflow::{context_id_from_operation_id, persist_recovery_fact, trace_id};
use aura_consensus::relational::run_consensus_with_commit;
use aura_consensus::types::CommitFact;
use aura_core::crypto::Ed25519Signature;
use aura_core::effects::{CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects};
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::hash;
use aura_core::key_resolution::TrustedKeyResolver;
use aura_core::relational::{ConsensusProof, RecoveryGrant, RecoveryOp};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::time::TimeStamp;
use aura_core::tree::LeafPublicKey;
use aura_core::types::identifiers::{ContextId, RecoveryId};
use aura_core::types::Epoch;
use aura_core::Prestate;
use aura_core::{AuraError, AuthorityId, Hash32, Result};
use aura_effects::random::RealRandomHandler;
use aura_effects::time::PhysicalTimeHandler;
use aura_macros::tell;
use aura_relational::RelationalContext;
use futures::lock::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Recovery protocol coordinator
#[derive(Debug, Clone)]
pub struct RecoveryProtocol {
    /// Recovery context for coordination
    pub recovery_context: Arc<RelationalContext>,
    /// Account authority being recovered
    pub account_authority: AuthorityId,
    /// Guardian authorities participating
    pub guardian_authorities: Vec<AuthorityId>,
    /// Recovery threshold
    pub threshold: u32,
    /// Collected guardian approvals
    approvals: Vec<GuardianApproval>,
}

/// Recovery request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Unique recovery ceremony ID
    pub recovery_id: RecoveryId,
    /// Account authority requesting recovery
    pub account_authority: AuthorityId,
    /// New tree commitment after recovery
    pub new_tree_commitment: Hash32,
    /// Recovery operation type
    pub operation: RecoveryOperation,
    /// Justification for recovery
    pub justification: String,
    /// Prestate hash covered by guardian approvals
    pub prestate_hash: Hash32,
}

/// Recovery operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperation {
    /// Replace the entire tree (device key recovery)
    ReplaceTree {
        /// New public key for the recovered tree
        new_public_key: PublicKeyPackage,
    },
    /// Add a new device to existing tree
    AddDevice {
        /// Public key of the new device
        device_public_key: LeafPublicKey,
    },
    /// Remove a compromised device
    RemoveDevice {
        /// Leaf index of device to remove
        leaf_index: u32,
    },
    /// Update guardian set
    UpdateGuardians {
        /// New guardian authorities
        new_guardians: Vec<AuthorityId>,
        /// New threshold
        new_threshold: u32,
    },
}

/// Guardian approval for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApproval {
    /// Guardian authority ID
    pub guardian_id: AuthorityId,
    /// Recovery request being approved
    pub recovery_id: RecoveryId,
    /// Guardian's signature over the recovery grant
    pub signature: Ed25519Signature,
    /// Timestamp
    pub timestamp: TimeStamp,
}

/// Recovery outcome from a ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryOutcome {
    /// Whether recovery succeeded
    pub success: bool,
    /// Recovery grant if successful
    pub recovery_grant: Option<RecoveryGrant>,
    /// Error message if failed
    pub error: Option<String>,
    /// Guardian approvals received
    pub approvals: Vec<GuardianApproval>,
}

impl RecoveryProtocol {
    /// Create a new recovery protocol
    pub fn new(
        recovery_context: Arc<RelationalContext>,
        account_authority: AuthorityId,
        guardian_authorities: Vec<AuthorityId>,
        threshold: u32,
    ) -> Self {
        Self {
            recovery_context,
            account_authority,
            guardian_authorities,
            threshold,
            approvals: Vec::new(),
        }
    }

    /// Get current tree commitment
    fn current_commitment(&self) -> Result<Hash32> {
        self.recovery_context.journal_commitment()
    }

    /// Get guardian commitment
    fn guardian_commitment(&self) -> Hash32 {
        let mut bytes = Vec::new();
        for guardian in &self.guardian_authorities {
            bytes.extend_from_slice(&guardian.to_bytes());
        }
        Hash32::from_bytes(&hash::hash(&bytes))
    }

    /// Run consensus protocol
    async fn run_consensus(
        &self,
        operation: &RecoveryOperation,
    ) -> Result<(ConsensusProof, CommitFact)> {
        // Create prestate
        let mut authority_commitments = std::collections::BTreeMap::new();
        authority_commitments.insert(self.account_authority, self.current_commitment()?);
        let prestate = Prestate {
            authority_commitments,
            context_commitment: self.recovery_context.journal_commitment()?,
        };

        // Run consensus using consensus adapter
        // For recovery, we use empty key packages since this is coordination, not FROST signing
        let key_packages: HashMap<AuthorityId, Share> = HashMap::new();
        let derived_group_key = hash::hash(self.account_authority.0.as_bytes()).to_vec();
        let group_public_key = PublicKeyPackage::new(
            derived_group_key,
            std::collections::BTreeMap::new(), // empty signer keys for recovery coordination
            1,
            1,
        );
        let epoch = Epoch::from(1); // Recovery uses a default epoch

        let random = RealRandomHandler;
        let time = PhysicalTimeHandler;
        run_consensus_with_commit(
            self.recovery_context.context_id,
            &prestate,
            operation,
            key_packages,
            group_public_key,
            epoch,
            &random,
            &time,
        )
        .await
    }

    /// Initiate recovery ceremony
    pub async fn initiate_recovery(&mut self, request: RecoveryRequest) -> Result<RecoveryOutcome> {
        // Validate request
        if request.account_authority != self.account_authority {
            return Err(AuraError::invalid("Account authority mismatch"));
        }
        // Reset approvals for a new ceremony
        self.approvals.clear();

        // Create recovery operation
        let recovery_op = match &request.operation {
            RecoveryOperation::ReplaceTree { .. } => RecoveryOp::ReplaceTree {
                new_tree_root: request.new_tree_commitment,
            },
            RecoveryOperation::AddDevice { device_public_key } => RecoveryOp::AddDevice {
                device_public_key: device_public_key.as_bytes().to_vec(),
            },
            RecoveryOperation::RemoveDevice { leaf_index } => RecoveryOp::RemoveDevice {
                leaf_index: *leaf_index,
            },
            RecoveryOperation::UpdateGuardians { new_threshold, .. } => RecoveryOp::UpdatePolicy {
                new_threshold: *new_threshold as u16,
            },
        };

        // Run consensus to get proof
        let (consensus_proof, commit_fact) = self.run_consensus(&request.operation).await?;

        // Create recovery grant
        let grant = RecoveryGrant {
            account_old: self.current_commitment()?,
            account_new: request.new_tree_commitment,
            guardian: self.guardian_commitment(),
            operation: recovery_op,
            consensus_proof,
        };

        // Record as a context-scoped detail fact.
        let _ = self
            .recovery_context
            .add_recovery_grant(self.account_authority, grant.clone())?;

        // Persist consensus evidence alongside the recovery grant.
        self.recovery_context
            .add_fact(commit_fact.to_relational_fact())?;

        let result = RecoveryOutcome {
            success: true,
            recovery_grant: Some(grant),
            error: None,
            approvals: self.approvals.clone(),
        };

        Ok(result)
    }

    /// Process guardian approval
    pub async fn process_guardian_approval<E, R>(
        &mut self,
        request: &RecoveryRequest,
        approval: GuardianApproval,
        crypto: &E,
        key_resolver: &R,
    ) -> Result<()>
    where
        E: CryptoEffects + Send + Sync + ?Sized,
        R: TrustedKeyResolver + ?Sized,
    {
        if request.account_authority != self.account_authority {
            return Err(AuraError::invalid("Account authority mismatch"));
        }
        if approval.recovery_id != request.recovery_id {
            return Err(AuraError::invalid("Approval is for different recovery"));
        }
        // Verify guardian is in the set
        if !self.guardian_authorities.contains(&approval.guardian_id) {
            return Err(AuraError::permission_denied("Guardian not in recovery set"));
        }
        let approved_at_ms = match &approval.timestamp {
            TimeStamp::PhysicalClock(time) => time.ts_ms,
            _ => {
                return Err(AuraError::invalid(
                    "Guardian approval requires a physical approval timestamp",
                ));
            }
        };
        let operation_hash = recovery_operation_hash(&request.operation)?;
        let verified = verify_recovery_approval_signature(
            crypto,
            RecoveryApprovalTranscriptPayload {
                recovery_id: request.recovery_id.clone(),
                account_authority: request.account_authority,
                operation_hash,
                prestate_hash: request.prestate_hash,
                approved: true,
                approved_at_ms,
                guardian_id: approval.guardian_id,
            },
            approval.signature.as_bytes(),
            key_resolver,
        )
        .await?;
        if !verified {
            return Err(AuraError::permission_denied(
                "Guardian recovery approval signature did not verify",
            ));
        }

        record_unique_approval(&mut self.approvals, approval);

        if !self.is_threshold_met(&self.approvals) {
            return Err(AuraError::permission_denied(
                "Recovery threshold not yet satisfied",
            ));
        }

        Ok(())
    }

    /// Check if recovery threshold is met
    pub fn is_threshold_met(&self, approvals: &[GuardianApproval]) -> bool {
        // Count unique guardian approvals
        let unique_guardians: std::collections::HashSet<_> =
            approvals.iter().map(|a| a.guardian_id).collect();

        (unique_guardians.len() as u32) >= self.threshold
    }
}

#[cfg(test)]
mod theorem_pack_tests {
    use aura_protocol::admission::{
        CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
        CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, THEOREM_PACK_AURA_AUTHORITY_EVIDENCE,
    };

    use super::telltale_session_types_recovery_protocol;

    #[test]
    fn recovery_proof_status_exposes_required_authority_pack() {
        assert_eq!(
            telltale_session_types_recovery_protocol::proof_status::REQUIRED_THEOREM_PACKS,
            &[THEOREM_PACK_AURA_AUTHORITY_EVIDENCE]
        );
    }

    #[test]
    fn recovery_manifest_emits_authority_evidence_metadata() {
        let manifest =
            telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest();
        let mut capabilities = manifest.required_theorem_pack_capabilities.clone();
        capabilities.sort();
        assert_eq!(
            manifest.required_theorem_packs,
            vec![THEOREM_PACK_AURA_AUTHORITY_EVIDENCE.to_string()]
        );
        assert_eq!(
            capabilities,
            vec![
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
            ]
        );
    }
}

#[cfg(test)]
mod approval_signature_tests {
    use super::*;
    use crate::recovery_approval::{
        recovery_operation_hash, RecoveryApprovalTranscript, RecoveryApprovalTranscriptPayload,
    };
    use aura_core::effects::CryptoCoreEffects;
    use aura_core::key_resolution::{KeyResolutionError, TrustedKeyDomain, TrustedPublicKey};
    use aura_core::time::PhysicalTime;
    use aura_effects::crypto::RealCryptoHandler;
    use aura_signature::sign_ed25519_transcript;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TestGuardianKeyResolver {
        keys: BTreeMap<AuthorityId, Vec<u8>>,
    }

    impl TestGuardianKeyResolver {
        fn with_guardian_key(mut self, guardian: AuthorityId, key: Vec<u8>) -> Self {
            self.keys.insert(guardian, key);
            self
        }
    }

    impl TrustedKeyResolver for TestGuardianKeyResolver {
        fn resolve_authority_threshold_key(
            &self,
            _authority: AuthorityId,
            _epoch: u64,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::AuthorityThreshold,
            })
        }

        fn resolve_device_key(
            &self,
            _device: aura_core::DeviceId,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Device,
            })
        }

        fn resolve_guardian_key(
            &self,
            guardian: AuthorityId,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            let key = self
                .keys
                .get(&guardian)
                .ok_or(KeyResolutionError::Unknown {
                    domain: TrustedKeyDomain::Guardian,
                })?;
            Ok(TrustedPublicKey::active(
                TrustedKeyDomain::Guardian,
                None,
                key.clone(),
                Hash32(hash::hash(key)),
            ))
        }

        fn resolve_release_key(
            &self,
            _authority: AuthorityId,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Release,
            })
        }
    }

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_protocol(
        account_authority: AuthorityId,
        guardians: Vec<AuthorityId>,
        threshold: u32,
    ) -> RecoveryProtocol {
        let context = Arc::new(RelationalContext::new(
            std::iter::once(account_authority)
                .chain(guardians.iter().copied())
                .collect(),
        ));
        RecoveryProtocol::new(context, account_authority, guardians, threshold)
    }

    fn test_request(account_authority: AuthorityId) -> RecoveryRequest {
        RecoveryRequest {
            recovery_id: RecoveryId::new("signed-recovery-test"),
            account_authority,
            new_tree_commitment: Hash32([7; 32]),
            operation: RecoveryOperation::RemoveDevice { leaf_index: 3 },
            justification: "test".to_string(),
            prestate_hash: Hash32([8; 32]),
        }
    }

    async fn signed_approval(
        crypto: &RealCryptoHandler,
        request: &RecoveryRequest,
        guardian_id: AuthorityId,
        private_key: &[u8],
        approved_at_ms: u64,
    ) -> GuardianApproval {
        let payload = RecoveryApprovalTranscriptPayload {
            recovery_id: request.recovery_id.clone(),
            account_authority: request.account_authority,
            operation_hash: recovery_operation_hash(&request.operation).unwrap(),
            prestate_hash: request.prestate_hash,
            approved: true,
            approved_at_ms,
            guardian_id,
        };
        let transcript = RecoveryApprovalTranscript::new(payload);
        let signature = sign_ed25519_transcript(crypto, &transcript, private_key)
            .await
            .unwrap();
        GuardianApproval {
            guardian_id,
            recovery_id: request.recovery_id.clone(),
            signature: Ed25519Signature::try_from(signature).unwrap(),
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: approved_at_ms,
                uncertainty: None,
            }),
        }
    }

    #[tokio::test]
    async fn valid_guardian_approval_signature_can_satisfy_threshold() {
        let crypto = RealCryptoHandler::for_simulation_seed([0xCA; 32]);
        let account = test_authority(1);
        let guardian = test_authority(2);
        let (private_key, public_key) = crypto.ed25519_generate_keypair().await.unwrap();
        let key_resolver =
            TestGuardianKeyResolver::default().with_guardian_key(guardian, public_key);
        let request = test_request(account);
        let approval = signed_approval(&crypto, &request, guardian, &private_key, 111).await;
        let mut protocol = test_protocol(account, vec![guardian], 1);

        protocol
            .process_guardian_approval(&request, approval, &crypto, &key_resolver)
            .await
            .unwrap();
        assert!(protocol.is_threshold_met(&protocol.approvals));
    }

    #[tokio::test]
    async fn forged_guardian_approval_does_not_advance_threshold() {
        let crypto = RealCryptoHandler::for_simulation_seed([0xCB; 32]);
        let account = test_authority(1);
        let guardian_a = test_authority(2);
        let guardian_b = test_authority(3);
        let (private_a, public_a) = crypto.ed25519_generate_keypair().await.unwrap();
        let (_private_b, public_b) = crypto.ed25519_generate_keypair().await.unwrap();
        let key_resolver = TestGuardianKeyResolver::default()
            .with_guardian_key(guardian_a, public_a)
            .with_guardian_key(guardian_b, public_b);
        let request = test_request(account);
        let valid = signed_approval(&crypto, &request, guardian_a, &private_a, 111).await;
        let forged = GuardianApproval {
            guardian_id: guardian_b,
            recovery_id: request.recovery_id.clone(),
            signature: Ed25519Signature::try_from(vec![0x42; 64]).unwrap(),
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 112,
                uncertainty: None,
            }),
        };
        let mut protocol = test_protocol(account, vec![guardian_a, guardian_b], 2);

        assert!(protocol
            .process_guardian_approval(&request, valid, &crypto, &key_resolver)
            .await
            .is_err());
        assert_eq!(protocol.approvals.len(), 1);
        assert!(protocol
            .process_guardian_approval(&request, forged, &crypto, &key_resolver)
            .await
            .is_err());
        assert_eq!(protocol.approvals.len(), 1);
        assert!(!protocol.is_threshold_met(&protocol.approvals));
    }
}

// Recovery Protocol Choreography
tell!(include_str!("src/recovery_protocol.tell"));

/// Recovery protocol handler
pub struct RecoveryProtocolHandler {
    protocol: Arc<RecoveryProtocol>,
    approvals: Mutex<HashMap<RecoveryId, Vec<GuardianApproval>>>,
}

fn record_unique_approval(
    approvals: &mut Vec<GuardianApproval>,
    approval: GuardianApproval,
) -> bool {
    if approvals
        .iter()
        .any(|existing| existing.guardian_id == approval.guardian_id)
    {
        false
    } else {
        approvals.push(approval);
        true
    }
}

fn recovery_context_id(recovery_id: &RecoveryId) -> ContextId {
    context_id_from_operation_id(recovery_id.as_str())
}

fn recovery_trace_id(recovery_id: &RecoveryId) -> Option<String> {
    trace_id(recovery_id.as_str())
}

fn recovery_request_hash(recovery_id: &RecoveryId) -> Hash32 {
    Hash32(hash::hash(recovery_id.as_str().as_bytes()))
}

fn recovery_share_hash(approval: &GuardianApproval) -> Hash32 {
    Hash32(hash::hash(approval.signature.as_bytes()))
}

impl RecoveryProtocolHandler {
    /// Create a new recovery handler
    pub fn new(protocol: Arc<RecoveryProtocol>) -> Self {
        Self {
            protocol,
            approvals: Mutex::new(HashMap::new()),
        }
    }

    /// Emit a recovery fact to the journal
    async fn emit_fact(
        &self,
        fact: RecoveryFact,
        journal_effects: &dyn JournalEffects,
    ) -> Result<()> {
        persist_recovery_fact(journal_effects, &fact).await
    }

    /// Handle recovery initiation
    pub async fn handle_recovery_initiation(
        &self,
        request: RecoveryRequest,
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<()> {
        // Initialize approval tracking
        let mut approvals = self.approvals.lock().await;
        approvals.insert(request.recovery_id.clone(), Vec::new());

        // Create context ID for this recovery ceremony
        let context_id = recovery_context_id(&request.recovery_id);

        // Emit RecoveryInitiated fact
        let timestamp = time_effects.physical_time().await?.ts_ms;
        let initiated_fact = RecoveryFact::RecoveryInitiated {
            context_id,
            account_id: request.account_authority,
            trace_id: recovery_trace_id(&request.recovery_id),
            request_hash: recovery_request_hash(&request.recovery_id),
            initiated_at: crate::utils::workflow::exact_physical_time(timestamp),
        };
        self.emit_fact(initiated_fact, journal).await?;

        // Notify guardians via effects
        self.notify_guardians_via_effects(&request, time_effects, network, journal)
            .await?;

        Ok(())
    }

    /// Handle guardian approval
    pub async fn handle_guardian_approval<E, R>(
        &self,
        request: &RecoveryRequest,
        approval: GuardianApproval,
        crypto: &E,
        key_resolver: &R,
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<bool>
    where
        E: CryptoEffects + Send + Sync + ?Sized,
        R: TrustedKeyResolver + ?Sized,
    {
        if request.account_authority != self.protocol.account_authority {
            return Err(AuraError::invalid("Account authority mismatch"));
        }
        if approval.recovery_id != request.recovery_id {
            return Err(AuraError::invalid("Approval is for different recovery"));
        }
        if !self
            .protocol
            .guardian_authorities
            .contains(&approval.guardian_id)
        {
            return Err(AuraError::permission_denied("Guardian not in recovery set"));
        }
        let approved_at_ms = match &approval.timestamp {
            TimeStamp::PhysicalClock(time) => time.ts_ms,
            _ => {
                return Err(AuraError::invalid(
                    "Guardian approval requires a physical approval timestamp",
                ));
            }
        };
        let operation_hash = recovery_operation_hash(&request.operation)?;
        let verified = verify_recovery_approval_signature(
            crypto,
            RecoveryApprovalTranscriptPayload {
                recovery_id: request.recovery_id.clone(),
                account_authority: request.account_authority,
                operation_hash,
                prestate_hash: request.prestate_hash,
                approved: true,
                approved_at_ms,
                guardian_id: approval.guardian_id,
            },
            approval.signature.as_bytes(),
            key_resolver,
        )
        .await?;
        if !verified {
            return Err(AuraError::permission_denied(
                "Guardian recovery approval signature did not verify",
            ));
        }

        // Create context ID for this recovery ceremony
        let context_id = recovery_context_id(&approval.recovery_id);

        // Emit RecoveryShareSubmitted fact
        let timestamp = time_effects.physical_time().await?.ts_ms;
        let share_fact = RecoveryFact::RecoveryShareSubmitted {
            context_id,
            guardian_id: approval.guardian_id,
            trace_id: recovery_trace_id(&approval.recovery_id),
            share_hash: recovery_share_hash(&approval),
            submitted_at: crate::utils::workflow::exact_physical_time(timestamp),
        };
        self.emit_fact(share_fact, journal).await?;

        // Add approval
        let mut approvals = self.approvals.lock().await;
        let ceremony_approvals = approvals
            .entry(approval.recovery_id.clone())
            .or_insert_with(Vec::new);

        record_unique_approval(ceremony_approvals, approval.clone());

        // Check if threshold met
        let threshold_met = self.protocol.is_threshold_met(ceremony_approvals);

        if threshold_met {
            let approvals_hash = Self::hash_approvals(ceremony_approvals);

            let approved_fact = RecoveryFact::RecoveryApproved {
                context_id,
                account_id: self.protocol.account_authority,
                trace_id: recovery_trace_id(&approval.recovery_id),
                approvals_hash,
                approved_at: crate::utils::workflow::exact_physical_time(timestamp),
            };
            self.emit_fact(approved_fact, journal).await?;

            self.update_journal_recovery_state_via_effects(
                &approval.recovery_id,
                "approved",
                ceremony_approvals,
                time_effects,
                journal,
            )
            .await?;

            // Finalize recovery via effects
            self.finalize_recovery_via_effects(
                &approval.recovery_id,
                ceremony_approvals,
                time_effects,
                network,
                journal,
            )
            .await?;

            // Remove cached approvals for this recovery once finalized.
            approvals.remove(&approval.recovery_id);
        }

        Ok(threshold_met)
    }

    fn hash_approvals(approvals: &[GuardianApproval]) -> Hash32 {
        let mut sorted = approvals.to_vec();
        sorted.sort_by_key(|approval| approval.guardian_id.to_bytes());

        let mut bytes = Vec::new();
        for approval in sorted {
            bytes.extend_from_slice(&approval.guardian_id.to_bytes());
            bytes.extend_from_slice(approval.signature.as_bytes());
        }

        Hash32(hash::hash(&bytes))
    }

    /// Cleanup stale approval caches by TTL (best-effort).
    ///
    /// Entries are removed if all approvals are older than `ttl_ms`
    /// relative to `now_ms`. Approvals with non-physical timestamps are retained.
    pub async fn cleanup_stale_approvals(&self, now_ms: u64, ttl_ms: u64) -> usize {
        let mut approvals = self.approvals.lock().await;
        let before = approvals.len();
        approvals.retain(|_, approvals_for_recovery| {
            let newest = approvals_for_recovery
                .iter()
                .filter_map(|a| match &a.timestamp {
                    aura_core::time::TimeStamp::PhysicalClock(t) => Some(t.ts_ms),
                    _ => None,
                })
                .max();
            match newest {
                Some(ts) => now_ms.saturating_sub(ts) <= ttl_ms,
                None => true,
            }
        });
        before.saturating_sub(approvals.len())
    }

    /// Notify guardians about recovery request via NetworkEffects
    async fn notify_guardians_via_effects(
        &self,
        request: &RecoveryRequest,
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<()> {
        // Serialize the recovery request
        let message_data =
            serde_json::to_vec(request).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Send recovery request to each guardian via network effects
        for guardian_id in &self.protocol.guardian_authorities {
            network
                .send_to_peer(guardian_id.0, message_data.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to notify guardian: {e}")))?;
        }

        // Update journal state with recovery initiation
        self.update_journal_recovery_state_via_effects(
            &request.recovery_id,
            "initiated",
            &[],
            time_effects,
            journal,
        )
        .await?;

        Ok(())
    }

    /// Finalize recovery via effects
    async fn finalize_recovery_via_effects(
        &self,
        recovery_id: &RecoveryId,
        approvals: &[GuardianApproval],
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<()> {
        let policy = policy_for(CeremonyFlow::RecoveryExecution);
        if !policy.allows_mode(AgreementMode::ConsensusFinalized) {
            return Err(AuraError::invalid(
                "Recovery execution does not permit consensus finalization",
            ));
        }

        // Create context ID for this recovery ceremony
        let context_id = recovery_context_id(recovery_id);

        // Emit RecoveryCompleted fact
        let timestamp = time_effects.physical_time().await?.ts_ms;
        let completed_fact = RecoveryFact::RecoveryCompleted {
            context_id,
            account_id: self.protocol.account_authority,
            trace_id: recovery_trace_id(recovery_id),
            evidence_hash: recovery_request_hash(recovery_id),
            completed_at: crate::utils::workflow::exact_physical_time(timestamp),
        };
        self.emit_fact(completed_fact, journal).await?;

        // Create recovery outcome
        let result = RecoveryOutcome {
            success: true,
            recovery_grant: None, // Would be populated from actual consensus
            error: None,
            approvals: approvals.to_vec(),
        };

        // Serialize the recovery result
        let result_data =
            serde_json::to_vec(&result).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Notify account of recovery completion via network effects
        network
            .send_to_peer(self.protocol.account_authority.0, result_data)
            .await
            .map_err(|e| AuraError::network(format!("Failed to notify account: {e}")))?;

        // Update journal state with recovery completion
        self.update_journal_recovery_state_via_effects(
            recovery_id,
            "completed",
            approvals,
            time_effects,
            journal,
        )
        .await?;

        Ok(())
    }

    /// Update recovery state in journal via JournalEffects
    async fn update_journal_recovery_state_via_effects(
        &self,
        recovery_id: &RecoveryId,
        state: &str,
        approvals: &[GuardianApproval],
        time_effects: &dyn PhysicalTimeEffects,
        journal_effects: &dyn JournalEffects,
    ) -> Result<()> {
        // Create a fact representing the recovery state change
        let timestamp = time_effects.physical_time().await?.ts_ms / 1000; // Convert milliseconds to seconds

        let state_data = serde_json::json!({
            "recovery_id": recovery_id.as_str(),
            "state": state,
            "approvals_count": approvals.len(),
            "timestamp": timestamp,
        });

        let mut journal = journal_effects.get_journal().await?;
        journal.facts.insert_with_context(
            format!("recovery_state:{}", recovery_id.as_str()),
            aura_core::journal::FactValue::String(state_data.to_string()),
            aura_core::ActorId::authority(self.protocol.account_authority),
            aura_core::FactTimestamp::new(timestamp),
            None,
        )?;
        journal_effects.persist_journal(&journal).await?;

        Ok(())
    }
}
