//! Threshold Signing Service
//!
//! Provides unified threshold signing operations for all scenarios:
//! - Multi-device personal signing
//! - Guardian recovery approvals
//! - Group operation approvals
//!
//! This service implements `ThresholdSigningEffects` and is the single point
//! of contact for all threshold cryptographic operations in the agent.
//!
//! ## Architecture
//!
//! The service maintains signing contexts per authority, storing:
//! - Threshold configuration (m-of-n)
//! - This device's signer index (if participating)
//! - Current epoch for key versioning
//!
//! Key material is stored via `SecureStorageEffects` (not in memory).
//! For single-device (threshold=1), signing is local without network.
//! For multi-device (threshold>1), coordination happens via choreography.

use super::state::with_state_mut_validated;
use crate::runtime::AuraEffectSystem;
use async_trait::async_trait;
use aura_consensus::dkg::recovery::recover_share_from_transcript;
use aura_consensus::dkg::{DkgTranscript, DkgTranscriptStore, StorageTranscriptStore};
use aura_core::crypto::single_signer::{SigningMode, SingleSignerPublicKeyPackage};
use aura_core::crypto::tree_signing;
use aura_core::effects::{
    crypto::KeyGenerationMethod, CryptoExtendedEffects, SecureStorageCapability,
    SecureStorageEffects, SecureStorageLocation,
};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{
    AgreementMode, ApprovalContext, ParticipantIdentity, SignableOperation, SigningContext,
    ThresholdConfig, ThresholdSignature, ThresholdState,
};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::{
    effects::{PhysicalTimeEffects, ThresholdSigningEffects},
    threshold::{ConvergenceCert, ReversionFact},
    AuraError, ContextId, Epoch, Hash32,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Threshold config metadata stored alongside keys for recovery during commit
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThresholdConfigMetadata {
    /// Minimum signers required (k-of-n)
    threshold_k: u16,
    /// Total number of participants
    total_n: u16,
    /// Participants who will hold shares (in protocol participant order)
    #[serde(default)]
    participants: Vec<ParticipantIdentity>,
    /// Signing mode (SingleSigner for 1-of-1, Threshold for k>=2)
    mode: SigningMode,
    /// Agreement mode for this epoch (A1/A2/A3)
    #[serde(default)]
    agreement_mode: AgreementMode,
}

impl ThresholdConfigMetadata {
    fn resolved_participants(&self) -> Vec<ParticipantIdentity> {
        self.participants.clone()
    }
}

/// Legacy threshold metadata stored by AuraEffectSystem rotate_keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThresholdMetadataFallback {
    /// The epoch this configuration applies to
    #[allow(dead_code)]
    epoch: u64,
    /// Minimum signers required (k-of-n)
    threshold: u16,
    /// Total number of participants
    total_participants: u16,
    /// Participants (in protocol participant order)
    #[serde(default)]
    participants: Vec<ParticipantIdentity>,
    /// Agreement mode (A1/A2/A3)
    #[serde(default)]
    agreement_mode: AgreementMode,
}

/// State for a signing context (per authority)
#[derive(Debug, Clone)]
pub struct SigningContextState {
    /// Threshold configuration
    pub config: ThresholdConfig,
    /// This device's participant index (if participating)
    pub my_signer_index: Option<u16>,
    /// Current epoch
    pub epoch: u64,
    /// Public key package (cached for verification)
    pub public_key_package: Vec<u8>,
    /// Signing mode (single-signer Ed25519 or FROST threshold)
    pub mode: SigningMode,
    /// Participants who hold shares (for threshold state queries / prestate binding)
    pub participants: Vec<ParticipantIdentity>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
}

#[derive(Debug, Clone)]
pub struct CoordinatorLease {
    pub coord_epoch: u64,
    pub issued_at_ms: u64,
}

#[derive(Debug, Default)]
struct ThresholdSigningState {
    contexts: HashMap<AuthorityId, SigningContextState>,
    leases: HashMap<AuthorityId, CoordinatorLease>,
}

impl ThresholdSigningState {
    fn validate(&self) -> Result<(), String> {
        for (authority, context) in &self.contexts {
            if context.config.threshold == 0 {
                return Err(format!("authority {:?} has zero threshold", authority));
            }
            if context.config.threshold > context.config.total_participants {
                return Err(format!(
                    "authority {:?} threshold {} exceeds total {}",
                    authority, context.config.threshold, context.config.total_participants
                ));
            }
            if context.participants.len() != context.config.total_participants as usize {
                return Err(format!(
                    "authority {:?} participant count {} does not match total {}",
                    authority,
                    context.participants.len(),
                    context.config.total_participants
                ));
            }
            if let Some(index) = context.my_signer_index {
                if index == 0 || index > context.config.total_participants {
                    return Err(format!(
                        "authority {:?} signer index {} out of bounds",
                        authority, index
                    ));
                }
            }
            if context.public_key_package.is_empty() {
                return Err(format!(
                    "authority {:?} missing public key package",
                    authority
                ));
            }
            let participant_set: HashSet<_> = context.participants.iter().collect();
            if participant_set.len() != context.participants.len() {
                return Err(format!(
                    "authority {:?} has duplicate participants",
                    authority
                ));
            }
        }
        Ok(())
    }
}

/// Unified service for all threshold signing operations
///
/// Handles:
/// - Multi-device signing (your devices)
/// - Guardian recovery (cross-authority)
/// - Group operations (shared authority)
/// - Hybrid schemes (device + guardian)
pub struct ThresholdSigningService {
    /// Effect system for crypto and secure storage operations
    effects: Arc<AuraEffectSystem>,

    /// In-memory signing state (contexts + leases)
    state: Arc<RwLock<ThresholdSigningState>>,
}

impl std::fmt::Debug for ThresholdSigningService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThresholdSigningService")
            .field("state", &"<RwLock<ThresholdSigningState>>")
            .finish()
    }
}

impl Clone for ThresholdSigningService {
    fn clone(&self) -> Self {
        Self {
            effects: self.effects.clone(),
            state: self.state.clone(),
        }
    }
}

impl ThresholdSigningService {
    /// Create a new threshold signing service
    pub fn new(effects: Arc<AuraEffectSystem>) -> Self {
        Self {
            effects,
            state: Arc::new(RwLock::new(ThresholdSigningState::default())),
        }
    }

    fn transcript_store(&self) -> StorageTranscriptStore<AuraEffectSystem> {
        StorageTranscriptStore::new_default(self.effects.clone())
    }

    /// Load a finalized DKG transcript by blob reference.
    pub async fn load_dkg_transcript(&self, reference: Hash32) -> Result<DkgTranscript, AuraError> {
        let store = self.transcript_store();
        store.get(&reference).await
    }

    /// Recover the encrypted share payload from a transcript for this authority.
    pub async fn recover_share_from_transcript(
        &self,
        transcript: &DkgTranscript,
        authority: &AuthorityId,
    ) -> Result<Vec<u8>, AuraError> {
        recover_share_from_transcript(transcript, *authority)
    }

    /// Update the agreement mode (A1/A2/A3) for an authority's signing context.
    pub async fn set_agreement_mode(
        &self,
        authority: &AuthorityId,
        mode: AgreementMode,
    ) -> Result<(), AuraError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let context = state
                    .contexts
                    .get_mut(authority)
                    .ok_or_else(|| AuraError::not_found("authority context not found"))?;
                context.agreement_mode = mode;
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Acquire or advance the coordinator lease (fencing token) for an authority.
    pub async fn acquire_coordinator_lease(
        &self,
        authority: &AuthorityId,
        coord_epoch: u64,
    ) -> Result<CoordinatorLease, AuraError> {
        let now = self.effects.physical_time().await?;
        let lease = CoordinatorLease {
            coord_epoch,
            issued_at_ms: now.ts_ms,
        };
        with_state_mut_validated(
            &self.state,
            |state| {
                if let Some(existing) = state.leases.get(authority) {
                    if coord_epoch <= existing.coord_epoch {
                        return Err(AuraError::invalid(
                            "Coordinator lease must advance monotonically",
                        ));
                    }
                }

                state.leases.insert(*authority, lease.clone());
                Ok(lease.clone())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Emit a convergence certificate for a soft-safe operation.
    pub async fn emit_convergence_cert(
        &self,
        context: ContextId,
        coordinator: &AuthorityId,
        op_id: Hash32,
        prestate_hash: Hash32,
        ack_set: Option<BTreeSet<AuthorityId>>,
        window: u64,
    ) -> Result<ConvergenceCert, AuraError> {
        let state = self.state.read().await;
        let lease = state
            .leases
            .get(coordinator)
            .ok_or_else(|| AuraError::invalid("Coordinator lease missing for convergence cert"))?;

        Ok(ConvergenceCert {
            context,
            op_id,
            prestate_hash,
            coord_epoch: lease.coord_epoch,
            ack_set,
            window,
        })
    }

    /// Emit a reversion fact for a soft-safe operation.
    pub async fn emit_reversion_fact(
        &self,
        context: ContextId,
        coordinator: &AuthorityId,
        op_id: Hash32,
        winner_op_id: Hash32,
    ) -> Result<ReversionFact, AuraError> {
        let state = self.state.read().await;
        let lease = state
            .leases
            .get(coordinator)
            .ok_or_else(|| AuraError::invalid("Coordinator lease missing for reversion fact"))?;

        Ok(ReversionFact {
            context,
            op_id,
            winner_op_id,
            coord_epoch: lease.coord_epoch,
        })
    }

    /// Sign operation for single-device using Ed25519 (SigningMode::SingleSigner)
    ///
    /// This is the fast path for 1-of-1 configurations that uses direct Ed25519
    /// signing without any FROST protocol overhead.
    async fn sign_solo_ed25519(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        tracing::debug!(?authority, "Signing with Ed25519 single-signer");

        // Load key package from secure storage
        // Location: signing_keys/<authority>/<epoch>/1
        let location = SecureStorageLocation::with_sub_key(
            "signing_keys",
            format!("{}/{}", authority, state.epoch),
            "1",
        );

        let key_package = self
            .effects
            .secure_retrieve(
                &location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to load key package: {}", e)))?;

        // Direct Ed25519 signing (no FROST overhead)
        let signature = self
            .effects
            .sign_with_key(message, &key_package, SigningMode::SingleSigner)
            .await
            .map_err(|e| AuraError::internal(format!("Ed25519 signing failed: {}", e)))?;

        tracing::info!(?authority, "Ed25519 single-signer signing complete");

        Ok(ThresholdSignature::single_signer(
            signature,
            state.public_key_package.clone(),
            state.epoch,
        ))
    }

    /// Serialize operation for signing
    fn serialize_operation(operation: &SignableOperation) -> Result<Vec<u8>, AuraError> {
        serde_json::to_vec(operation)
            .map_err(|e| AuraError::internal(format!("Failed to serialize operation: {}", e)))
    }

    /// Compute a binding message for tree operations that matches tree verification.
    fn tree_op_message(op: &TreeOp, state: &SigningContextState) -> Result<Vec<u8>, AuraError> {
        let group_public_key = Self::group_public_key_bytes(state)?;
        let attested = AttestedOp {
            op: op.clone(),
            agg_sig: Vec::new(),
            signer_count: 0,
        };

        Ok(tree_signing::tree_op_binding_message(
            &attested,
            Epoch::new(state.epoch),
            &group_public_key,
        ))
    }

    /// Extract the group public key bytes for binding messages.
    fn group_public_key_bytes(state: &SigningContextState) -> Result<[u8; 32], AuraError> {
        match state.mode {
            SigningMode::SingleSigner => {
                let package = SingleSignerPublicKeyPackage::from_bytes(&state.public_key_package)
                    .map_err(|e| {
                    AuraError::internal(format!(
                        "Failed to decode single-signer public key package: {e}"
                    ))
                })?;
                package
                    .verifying_key()
                    .try_into()
                    .map_err(|_| AuraError::internal("Single-signer public key length mismatch"))
            }
            SigningMode::Threshold => {
                let package =
                    tree_signing::public_key_package_from_bytes(&state.public_key_package)
                        .map_err(|e| {
                            AuraError::internal(format!(
                                "Failed to decode threshold public key package: {e}"
                            ))
                        })?;
                package
                    .group_public_key
                    .as_slice()
                    .try_into()
                    .map_err(|_| AuraError::internal("Threshold public key length mismatch"))
            }
        }
    }

    /// Route single-device signing.
    async fn sign_solo(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        self.sign_solo_ed25519(authority, message, state).await
    }

    /// Aggregate a threshold signature using locally available shares.
    async fn sign_threshold_local(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        use aura_core::effects::SecureStorageCapability;
        use aura_core::effects::SecureStorageLocation;

        struct SignerMaterial {
            signer_id: u16,
            key_package: Vec<u8>,
        }

        let mut signers: Vec<SignerMaterial> = Vec::new();
        let mut missing = Vec::new();

        for participant in &state.participants {
            let location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!("{}/{}", authority, state.epoch),
                participant.storage_key(),
            );

            match self
                .effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
            {
                Ok(key_package) => {
                    let share =
                        tree_signing::share_from_key_package_bytes(&key_package).map_err(|e| {
                            AuraError::internal(format!(
                                "Failed to decode key package for {}: {e}",
                                participant.debug_label()
                            ))
                        })?;
                    signers.push(SignerMaterial {
                        signer_id: share.identifier,
                        key_package,
                    });
                }
                Err(_) => missing.push(participant.debug_label()),
            }
        }

        if signers.len() < state.config.threshold as usize {
            return Err(AuraError::internal(format!(
                "Insufficient local shares for threshold signing (need {}, have {}, missing: {})",
                state.config.threshold,
                signers.len(),
                if missing.is_empty() {
                    "none".to_string()
                } else {
                    missing.join(", ")
                }
            )));
        }

        signers.sort_by_key(|s| s.signer_id);
        let participant_ids: Vec<u16> = signers.iter().map(|s| s.signer_id).collect();

        let mut nonces = Vec::with_capacity(signers.len());
        for signer in &signers {
            let nonce = self
                .effects
                .frost_generate_nonces(&signer.key_package)
                .await
                .map_err(|e| {
                    AuraError::internal(format!("Failed to generate FROST nonces: {e}"))
                })?;
            nonces.push(nonce);
        }

        let signing_package = self
            .effects
            .frost_create_signing_package(
                message,
                &nonces,
                &participant_ids,
                &state.public_key_package,
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to create signing package: {e}")))?;

        let mut partials = Vec::with_capacity(signers.len());
        for (signer, nonce) in signers.iter().zip(nonces.iter()) {
            let partial = self
                .effects
                .frost_sign_share(&signing_package, &signer.key_package, nonce)
                .await
                .map_err(|e| {
                    AuraError::internal(format!("Failed to sign share {}: {e}", signer.signer_id))
                })?;
            partials.push(partial);
        }

        let signature = self
            .effects
            .frost_aggregate_signatures(&signing_package, &partials)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to aggregate signatures: {e}")))?;

        Ok(ThresholdSignature::new(
            signature,
            participant_ids.len() as u16,
            participant_ids,
            state.public_key_package.clone(),
            state.epoch,
        ))
    }
}

#[async_trait]
impl ThresholdSigningEffects for ThresholdSigningService {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        tracing::info!(
            ?authority,
            "Bootstrapping authority with 1-of-1 Ed25519 keys"
        );

        let epoch = 0u64;
        let participant = ParticipantIdentity::guardian(*authority);
        let participants = vec![participant.clone()];

        // Generate 1-of-1 signing keys (will use Ed25519 single-signer mode)
        let key_result = self
            .effects
            .generate_signing_keys_with(KeyGenerationMethod::SingleSigner, 1, 1)
            .await
            .map_err(|e| AuraError::internal(format!("Key generation failed: {}", e)))?;

        if key_result.key_packages.is_empty() {
            return Err(AuraError::internal(
                "Key generation returned no key packages",
            ));
        }

        // Store key package in secure storage
        // Location: signing_keys/<authority>/<epoch>/1
        let location = SecureStorageLocation::with_sub_key(
            "signing_keys",
            format!("{}/{}", authority, epoch),
            "1", // signer index 1
        );

        self.effects
            .secure_store(
                &location,
                &key_result.key_packages[0],
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store key package: {}", e)))?;

        // Store participant share for consensus/DKG helpers.
        let participant_location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}/{}", authority, epoch),
            participant.storage_key(),
        );
        self.effects
            .secure_store(
                &participant_location,
                &key_result.key_packages[0],
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| {
                AuraError::internal(format!("Failed to store participant share: {}", e))
            })?;

        // Persist public key package for consensus helpers.
        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", epoch),
        );
        self.effects
            .secure_store(
                &pubkey_location,
                &key_result.public_key_package,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| {
                AuraError::internal(format!("Failed to store public key package: {}", e))
            })?;

        // Persist epoch + threshold config metadata for consensus helpers.
        let config_metadata = ThresholdConfigMetadata {
            threshold_k: 1,
            total_n: 1,
            participants,
            mode: SigningMode::SingleSigner,
            agreement_mode: AgreementMode::Provisional,
        };
        let config_bytes = serde_json::to_vec(&config_metadata).map_err(|e| {
            AuraError::internal(format!("Failed to serialize threshold config: {}", e))
        })?;
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", epoch),
        );
        self.effects
            .secure_store(
                &config_location,
                &config_bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store threshold config: {}", e)))?;

        let epoch_location = SecureStorageLocation::new("epoch_state", format!("{}", authority));
        self.effects
            .secure_store(
                &epoch_location,
                &epoch.to_le_bytes(),
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store epoch state: {}", e)))?;

        // Create context state
        let config = ThresholdConfig::new(1, 1)?;
        let state = SigningContextState {
            config,
            my_signer_index: Some(1),
            epoch,
            public_key_package: key_result.public_key_package.clone(),
            mode: key_result.mode,
            participants: vec![participant],
            agreement_mode: AgreementMode::Provisional,
        };

        // Store in memory cache
        with_state_mut_validated(
            &self.state,
            |state_map| {
                state_map.contexts.insert(*authority, state);
            },
            |state_map| state_map.validate(),
        )
        .await;

        tracing::info!(
            ?authority,
            mode = %key_result.mode,
            "Authority bootstrapped with 1-of-1 signing keys"
        );

        Ok(key_result.public_key_package)
    }

    async fn sign(&self, context: SigningContext) -> Result<ThresholdSignature, AuraError> {
        let state = self
            .state
            .read()
            .await
            .contexts
            .get(&context.authority)
            .cloned()
            .ok_or_else(|| {
                AuraError::internal(format!(
                    "No signing context for authority: {:?}",
                    context.authority
                ))
            })?;

        // Check if we're a participant
        if state.my_signer_index.is_none() {
            return Err(AuraError::internal(
                "This device is not a participant for this authority",
            ));
        }

        // Serialize or bind the operation for signing
        let message = match &context.operation {
            SignableOperation::TreeOp(op) => Self::tree_op_message(op, &state)?,
            _ => Self::serialize_operation(&context.operation)?,
        };

        // Log the approval context for audit
        match &context.approval_context {
            ApprovalContext::SelfOperation => {
                tracing::debug!(?context.authority, "Signing self operation");
            }
            ApprovalContext::RecoveryAssistance { recovering, .. } => {
                tracing::info!(
                    ?context.authority,
                    ?recovering,
                    "Signing recovery assistance"
                );
            }
            ApprovalContext::GroupDecision { group, proposal_id } => {
                tracing::info!(
                    ?context.authority,
                    ?group,
                    %proposal_id,
                    "Signing group decision"
                );
            }
            ApprovalContext::ElevatedOperation { operation_type, .. } => {
                tracing::warn!(
                    ?context.authority,
                    %operation_type,
                    "Signing elevated operation"
                );
            }
        }

        // Use single-device fast path if threshold=1
        if state.config.threshold == 1 {
            return self.sign_solo(&context.authority, &message, &state).await;
        }

        // Threshold signing via local share aggregation (demo/prototyping path).
        self.sign_threshold_local(&context.authority, &message, &state)
            .await
    }

    async fn threshold_config(&self, authority: &AuthorityId) -> Option<ThresholdConfig> {
        self.state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.config.clone())
    }

    async fn threshold_state(&self, authority: &AuthorityId) -> Option<ThresholdState> {
        self.state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|state| ThresholdState {
                epoch: state.epoch,
                threshold: state.config.threshold,
                total_participants: state.config.total_participants,
                participants: state.participants.clone(),
                agreement_mode: state.agreement_mode,
            })
    }

    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool {
        self.state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.my_signer_index.is_some())
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        self.state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.public_key_package.clone())
    }

    async fn rotate_keys(
        &self,
        authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        participants: &[ParticipantIdentity],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
        tracing::info!(
            ?authority,
            new_threshold,
            new_total_participants,
            num_participants = participants.len(),
            "Rotating threshold keys for key-rotation ceremony"
        );

        // Validate inputs
        if participants.len() != new_total_participants as usize {
            return Err(AuraError::invalid(format!(
                "Participant count ({}) must match total_participants ({})",
                participants.len(),
                new_total_participants
            )));
        }

        // Get current state to determine new epoch
        let current_epoch = self
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.epoch)
            .unwrap_or(0);

        let new_epoch = current_epoch + 1;

        // Generate new threshold keys using FROST
        // For threshold >= 2, this uses FROST DKG
        // For threshold == 1 with max_signers == 1, this uses Ed25519
        let key_result = if new_threshold >= 2 {
            // Use frost_rotate_keys for threshold configurations
            // Note: The old_shares parameter is for potential future resharing;
            // currently we do a fresh DKG which produces a new group public key
            self.effects
                .frost_rotate_keys(&[], 0, new_threshold, new_total_participants)
                .await
                .map_err(|e| AuraError::internal(format!("FROST key rotation failed: {}", e)))?
        } else {
            // Single-signer mode (shouldn't happen for guardian ceremony, but handle it)
            let result = self
                .effects
                .generate_signing_keys_with(
                    KeyGenerationMethod::DealerBased,
                    new_threshold,
                    new_total_participants,
                )
                .await
                .map_err(|e| AuraError::internal(format!("Key generation failed: {}", e)))?;

            aura_core::effects::crypto::FrostKeyGenResult {
                key_packages: result.key_packages,
                public_key_package: result.public_key_package,
            }
        };

        // Store each key package indexed by participant identity
        // Note: In a real deployment, these would be encrypted with each guardian's
        // public key before storage. For demo mode, we store them directly.
        for (i, (participant, key_package)) in participants
            .iter()
            .zip(key_result.key_packages.iter())
            .enumerate()
        {
            let signer_index = (i + 1) as u16; // 1-indexed
            let _ = signer_index; // Used for logging below

            // Store at: participant_shares/<authority>/<epoch>/<participant_key>
            let location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!("{}/{}", authority, new_epoch),
                participant.storage_key(),
            );

            self.effects
                .secure_store(
                    &location,
                    key_package,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await
                .map_err(|e| {
                    AuraError::internal(format!(
                        "Failed to store key package for participant {}: {}",
                        participant.debug_label(),
                        e
                    ))
                })?;

            tracing::debug!(
                ?authority,
                participant = %participant.debug_label(),
                signer_index,
                new_epoch,
                "Stored participant key package"
            );
        }

        // Store the public key package at the new epoch
        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", new_epoch),
        );

        self.effects
            .secure_store(
                &pubkey_location,
                &key_result.public_key_package,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| {
                AuraError::internal(format!("Failed to store public key package: {}", e))
            })?;

        // Store threshold config metadata for use in commit_key_rotation
        // This includes threshold_k, total_n, and participants
        let config_metadata = ThresholdConfigMetadata {
            threshold_k: new_threshold,
            total_n: new_total_participants,
            participants: participants.to_vec(),
            mode: if new_threshold >= 2 {
                SigningMode::Threshold
            } else {
                SigningMode::SingleSigner
            },
            agreement_mode: AgreementMode::CoordinatorSoftSafe,
        };

        let config_bytes = serde_json::to_vec(&config_metadata).map_err(|e| {
            AuraError::internal(format!("Failed to serialize threshold config: {}", e))
        })?;

        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", new_epoch),
        );

        self.effects
            .secure_store(
                &config_location,
                &config_bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store threshold config: {}", e)))?;

        tracing::debug!(
            ?authority,
            new_epoch,
            threshold_k = new_threshold,
            total_n = new_total_participants,
            "Stored threshold config metadata"
        );

        // Don't update the in-memory context yet - wait for commit
        // The old epoch remains active until commit_key_rotation is called

        tracing::info!(
            ?authority,
            new_epoch,
            new_threshold,
            new_total_participants,
            "Key rotation prepared - awaiting ceremony completion"
        );

        Ok((
            new_epoch,
            key_result.key_packages,
            key_result.public_key_package,
        ))
    }

    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), AuraError> {
        tracing::info!(
            ?authority,
            new_epoch,
            "Committing key rotation after successful ceremony"
        );

        // Load the public key package for the new epoch
        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", new_epoch),
        );

        let public_key_package = self
            .effects
            .secure_retrieve(
                &pubkey_location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| {
                AuraError::internal(format!(
                    "Failed to load public key package for epoch {}: {}",
                    new_epoch, e
                ))
            })?;

        // Load threshold config metadata stored during rotate_keys.
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", new_epoch),
        );

        let mut config_metadata: ThresholdConfigMetadata = match self
            .effects
            .secure_retrieve(
                &config_location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
        {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                AuraError::internal(format!("Failed to deserialize threshold config: {}", e))
            })?,
            Err(_) => {
                // Fallback to legacy metadata stored by AuraEffectSystem rotate_keys.
                let legacy_location = SecureStorageLocation::with_sub_key(
                    "threshold_metadata",
                    format!("{}", authority),
                    format!("{}", new_epoch),
                );
                let legacy_bytes = self
                    .effects
                    .secure_retrieve(
                        &legacy_location,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!(
                            "Failed to load threshold metadata for epoch {}: {}",
                            new_epoch, e
                        ))
                    })?;
                let legacy: ThresholdMetadataFallback = serde_json::from_slice(&legacy_bytes)
                    .map_err(|e| {
                        AuraError::internal(format!(
                            "Failed to deserialize threshold metadata: {}",
                            e
                        ))
                    })?;
                let mode = if legacy.threshold >= 2 {
                    SigningMode::Threshold
                } else {
                    SigningMode::SingleSigner
                };
                let metadata = ThresholdConfigMetadata {
                    threshold_k: legacy.threshold,
                    total_n: legacy.total_participants,
                    participants: legacy.participants,
                    mode,
                    agreement_mode: legacy.agreement_mode,
                };

                let upgraded = serde_json::to_vec(&metadata).map_err(|e| {
                    AuraError::internal(format!(
                        "Failed to serialize upgraded threshold config: {}",
                        e
                    ))
                })?;
                let _ = self
                    .effects
                    .secure_store(
                        &config_location,
                        &upgraded,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await;
                metadata
            }
        };

        if config_metadata.agreement_mode != AgreementMode::ConsensusFinalized {
            config_metadata.agreement_mode = AgreementMode::ConsensusFinalized;
            let updated_bytes = serde_json::to_vec(&config_metadata).map_err(|e| {
                AuraError::internal(format!("Failed to serialize threshold config: {}", e))
            })?;
            self.effects
                .secure_store(
                    &config_location,
                    &updated_bytes,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await
                .map_err(|e| {
                    AuraError::internal(format!(
                        "Failed to update threshold config for epoch {}: {}",
                        new_epoch, e
                    ))
                })?;
        }

        // Build the new threshold config from stored metadata
        let new_config = ThresholdConfig::new(config_metadata.threshold_k, config_metadata.total_n)
            .map_err(|e| AuraError::internal(format!("Invalid threshold config: {}", e)))?;

        // Update or create in-memory context to use the new epoch with proper config.
        let participants = config_metadata.resolved_participants();
        let agreement_mode = config_metadata.agreement_mode;
        let mode = config_metadata.mode;
        let threshold_k = config_metadata.threshold_k;
        let total_n = config_metadata.total_n;

        with_state_mut_validated(
            &self.state,
            |state| {
                if let Some(context) = state.contexts.get_mut(authority) {
                    let old_epoch = context.epoch;
                    context.epoch = new_epoch;
                    context.public_key_package = public_key_package;
                    context.config = new_config;
                    context.mode = mode;
                    context.participants = participants;
                    context.agreement_mode = agreement_mode;

                    tracing::info!(
                        ?authority,
                        old_epoch,
                        new_epoch,
                        threshold_k,
                        total_n,
                        "Key rotation committed - new epoch is now active"
                    );
                } else {
                    let device_id = self.effects.device_id();
                    let my_signer_index = participants
                        .iter()
                        .position(|p| match p {
                            ParticipantIdentity::Device(id) => *id == device_id,
                            _ => false,
                        })
                        .map(|idx| (idx + 1) as u16);

                    let context = SigningContextState {
                        config: new_config,
                        my_signer_index,
                        epoch: new_epoch,
                        public_key_package,
                        mode,
                        participants,
                        agreement_mode,
                    };

                    state.contexts.insert(*authority, context);

                    tracing::info!(
                        ?authority,
                        new_epoch,
                        threshold_k,
                        total_n,
                        "Key rotation committed - new authority context loaded"
                    );
                }
            },
            |state| state.validate(),
        )
        .await;

        Ok(())
    }

    async fn rollback_key_rotation(
        &self,
        authority: &AuthorityId,
        failed_epoch: u64,
    ) -> Result<(), AuraError> {
        tracing::warn!(
            ?authority,
            failed_epoch,
            "Rolling back key rotation after ceremony failure"
        );

        let delete_caps = &[
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        // Load the config FIRST to get guardian IDs for cleaning up their shares
        // (before we delete it)
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", failed_epoch),
        );

        let config_metadata: Option<ThresholdConfigMetadata> = {
            let config_bytes = self
                .effects
                .secure_retrieve(&config_location, delete_caps)
                .await
                .ok();

            config_bytes.and_then(|bytes| serde_json::from_slice(&bytes).ok())
        };

        // Delete the public key package for the failed epoch
        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", failed_epoch),
        );

        if let Err(e) = self
            .effects
            .secure_delete(&pubkey_location, delete_caps)
            .await
        {
            tracing::debug!(
                ?authority,
                failed_epoch,
                error = %e,
                "Failed to delete public key package (may not exist)"
            );
        }

        // Delete the threshold config metadata
        if let Err(e) = self
            .effects
            .secure_delete(&config_location, delete_caps)
            .await
        {
            tracing::debug!(
                ?authority,
                failed_epoch,
                error = %e,
                "Failed to delete threshold config (may not exist)"
            );
        }

        // Delete guardian key packages for this failed epoch
        if let Some(metadata) = config_metadata {
            for participant in &metadata.participants {
                let share_location = SecureStorageLocation::with_sub_key(
                    "participant_shares",
                    format!("{}/{}", authority, failed_epoch),
                    participant.storage_key(),
                );

                if let Err(e) = self
                    .effects
                    .secure_delete(&share_location, delete_caps)
                    .await
                {
                    tracing::debug!(
                        ?authority,
                        failed_epoch,
                        participant = %participant.debug_label(),
                        error = %e,
                        "Failed to delete participant share (may not exist)"
                    );
                }
            }
        }

        tracing::info!(
            ?authority,
            failed_epoch,
            "Key rotation rolled back - cleaned up failed epoch data"
        );

        // Note: The in-memory context was never updated (we wait for commit),
        // so no in-memory rollback is needed

        Ok(())
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

use super::traits::{RuntimeService, ServiceError, ServiceHealth};
use super::RuntimeTaskRegistry;

#[async_trait]
impl RuntimeService for ThresholdSigningService {
    fn name(&self) -> &'static str {
        "threshold_signing"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["ceremony_tracker"]
    }

    async fn start(&self, _tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError> {
        // ThresholdSigningService is in-memory and always ready
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        // Clear signing contexts + leases on shutdown
        with_state_mut_validated(
            &self.state,
            |state| {
                state.contexts.clear();
                state.leases.clear();
            },
            |state| state.validate(),
        )
        .await;
        Ok(())
    }

    fn health(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::threshold::SigningContext;
    use aura_core::tree::{TreeOp, TreeOpKind};
    use aura_core::Epoch;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_tree_op() -> TreeOp {
        TreeOp {
            parent_epoch: Epoch::initial(),
            parent_commitment: [0u8; 32],
            op: TreeOpKind::RotateEpoch { affected: vec![] },
            version: 1,
        }
    }

    #[test]
    fn test_signing_context_construction() {
        let context = SigningContext::self_tree_op(test_authority(), test_tree_op());
        assert!(matches!(
            context.approval_context,
            ApprovalContext::SelfOperation
        ));
    }

    #[test]
    fn test_serialize_operation() {
        let op = SignableOperation::TreeOp(test_tree_op());
        let result = ThresholdSigningService::serialize_operation(&op);
        assert!(result.is_ok());
    }
}
