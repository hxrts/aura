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

#[derive(Debug, thiserror::Error)]
#[error("cached threshold context contradicts protected current signing mode")]
struct UnownedThresholdContextModeError;

use super::state::with_state_mut_validated;
mod device_quorum;
pub(crate) use device_quorum::DevicePossessionProof;
mod enrollment_quorum_registry;
mod enrollment_transcript_proxy;
mod enrollment_transcript_signing;
mod enrollment_transcript_wire;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::runtime::effects::ThresholdConfigMetadata;
use crate::runtime::AuraEffectSystem;
use async_trait::async_trait;
use aura_consensus::dkg::recovery::recover_share_from_transcript;
use aura_consensus::dkg::{DkgTranscript, DkgTranscriptStore, StorageTranscriptStore};
use aura_core::crypto::single_signer::{
    SigningMode, SingleSignerKeyPackage, SingleSignerPublicKeyPackage,
};
use aura_core::crypto::tree_signing;
use aura_core::effects::RandomCoreEffects;
use aura_core::effects::{
    crypto::KeyGenerationMethod, CryptoExtendedEffects, SecureStorageCapability,
    SecureStorageEffects, SecureStorageLocation, StorageCoreEffects,
};
use aura_core::effects::{CapabilityKey, RuntimeCapabilityEffects};
use aura_core::threshold::{
    AgreementMode, ApprovalContext, ParticipantIdentity, SignableOperation, SigningContext,
    ThresholdConfig, ThresholdSignature, ThresholdState,
};
use aura_core::tree::metadata::DeviceLeafMetadata;
use aura_core::tree::{AttestedOp, LeafId, LeafNode, LeafRole, NodeIndex, TreeOp};
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_core::{
    effects::{PhysicalTimeEffects, ThresholdSigningEffects},
    secrets::SecretExportContext,
    threshold::{ConvergenceCert, ReversionFact},
    AuraError, ContextId, Epoch, Hash32,
};
use aura_effects::RuntimeCapabilityHandler;
use aura_invitation::enrollment_setup::{
    DeviceEnrollmentSetupRequest, DeviceEnrollmentSetupStatement, EnrollmentSetupExportError,
};
use aura_protocol::effects::TreeEffects;
use aura_signature::threshold_signing_context_transcript_bytes;
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBootstrapMigrationDecision {
    version: u16,
    authority: AuthorityId,
    device: aura_core::DeviceId,
    original_policy_digest: [u8; 32],
    original_envelope_digest: [u8; 32],
    public_package_digest: [u8; 32],
    creation_op_digest: [u8; 32],
}

/// Validate an explicit migration origin before using converted active policy.
/// Protected original records, not a metadata string, prove this origin.
pub(crate) async fn validate_bootstrap_migration_origin(
    effects: &AuraEffectSystem,
    authority: &AuthorityId,
    epoch: u64,
    origin: [u8; 32],
) -> Result<(), AuraError> {
    if epoch != 0 || *authority != effects.runtime_authority_id() {
        return Err(BootstrapGenesisError::RecordMismatch.into());
    }
    let location = SecureStorageLocation::new(
        "bootstrap_physical_participant_migration_v1",
        authority.to_string(),
    );
    let bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > 4096 || aura_core::hash::hash(&bytes) != origin {
        return Err(BootstrapGenesisError::RecordMismatch.into());
    }
    let decision: LegacyBootstrapMigrationDecision = serde_json::from_slice(&bytes)?;
    if serde_json::to_vec(&decision)? != bytes
        || decision.version != 1
        || decision.authority != *authority
        || decision.device != effects.device_id()
    {
        return Err(BootstrapGenesisError::RecordMismatch.into());
    }
    let genesis_bytes = effects
        .secure_retrieve(
            &ThresholdSigningService::bootstrap_genesis_location(authority),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if genesis_bytes.len() > 2048 {
        return Err(BootstrapGenesisError::RecordSize.into());
    }
    let genesis: BootstrapGenesisRecord = serde_json::from_slice(&genesis_bytes)?;
    if genesis.version != 1
        || genesis.authority != *authority
        || genesis.device != effects.device_id()
        || genesis.epoch != 0
        || genesis.public_package_digest != decision.public_package_digest
        || !matches!(genesis.state, BootstrapGenesisState::Complete { creation_op_digest }
            if creation_op_digest == decision.creation_op_digest)
    {
        return Err(BootstrapGenesisError::RecordMismatch.into());
    }
    let package = effects
        .secure_retrieve(
            &SecureStorageLocation::with_sub_key("threshold_pubkey", authority.to_string(), "0"),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if package.is_empty() || package.len() > 65_536 {
        return Err(BootstrapGenesisError::RecordSize.into());
    }
    if aura_core::hash::hash(&package) != decision.public_package_digest {
        return Err(BootstrapGenesisError::RecordMismatch.into());
    }
    Ok(())
}

const PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION: u8 = 1;
const PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN: &str = "aura:participant-key-package-envelope:v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapGenesisRecord {
    version: u16,
    authority: AuthorityId,
    device: aura_core::DeviceId,
    epoch: u64,
    public_package_digest: [u8; 32],
    state: BootstrapGenesisState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum BootstrapGenesisState {
    Pending,
    Complete { creation_op_digest: [u8; 32] },
}

#[derive(Debug, thiserror::Error)]
enum BootstrapGenesisError {
    #[error("bootstrap genesis record does not match this authority/device/key")]
    RecordMismatch,
    #[error("bootstrap genesis record exceeds its size bound")]
    RecordSize,
    #[error("bootstrap device leaf conflicts with the retained signing key")]
    DeviceKeyMismatch,
    #[error("bootstrap has no authenticated device creation witness")]
    MissingCreation,
    #[error("bootstrap device creation signature is invalid")]
    InvalidCreationSignature,
    #[error("bootstrap creation witness differs from its completion record")]
    CompletionMismatch,
    #[error("bootstrap creation witness is not durably indexed in tree storage")]
    MissingDurableCreation,
    #[error(
        "completed bootstrap is missing its active signing epoch; explicit recovery is required"
    )]
    MissingActiveEpoch,
}

impl From<BootstrapGenesisError> for AuraError {
    fn from(error: BootstrapGenesisError) -> Self {
        Self::Storage {
            message: error.to_string(),
            source: Some(Arc::new(error)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParticipantKeyPackageEnvelope {
    version: u8,
    authority: AuthorityId,
    epoch: u64,
    recipient: ParticipantIdentity,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[allow(dead_code)] // Declaration-layer ingress inventory; runtime actor wiring lands incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThresholdSigningCommand {
    BootstrapAuthority,
    LoadDkgTranscript,
    RecoverShareFromTranscript,
    SetAgreementMode,
    AcquireCoordinatorLease,
    EmitConvergenceCert,
    EmitReversionFact,
    Sign,
    ExportEnrollmentSetup,
    PrepareApprovedEnrollment,
    ApproveEnrollmentTranscript,
    ResumeApprovedEnrollment,
    RotateKeys,
    CommitKeyRotation,
    RollbackKeyRotation,
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

#[derive(Debug, thiserror::Error)]
#[error("participant wrapping key length is {actual}, expected 32")]
struct InvalidParticipantWrappingKeyLength {
    actual: usize,
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
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for (authority, context) in &self.contexts {
            if context.config.threshold == 0 {
                return Err(super::invariant::InvariantViolation::new(
                    "ThresholdSigning",
                    format!("authority {:?} has zero threshold", authority),
                ));
            }
            if context.config.threshold > context.config.total_participants {
                return Err(super::invariant::InvariantViolation::new(
                    "ThresholdSigning",
                    format!(
                        "authority {:?} threshold {} exceeds total {}",
                        authority, context.config.threshold, context.config.total_participants
                    ),
                ));
            }
            if context.participants.len() != context.config.total_participants as usize {
                return Err(super::invariant::InvariantViolation::new(
                    "ThresholdSigning",
                    format!(
                        "authority {:?} participant count {} does not match total {}",
                        authority,
                        context.participants.len(),
                        context.config.total_participants
                    ),
                ));
            }
            if let Some(index) = context.my_signer_index {
                if index == 0 || index > context.config.total_participants {
                    return Err(super::invariant::InvariantViolation::new(
                        "ThresholdSigning",
                        format!(
                            "authority {:?} signer index {} out of bounds",
                            authority, index
                        ),
                    ));
                }
            }
            if context.public_key_package.is_empty() {
                return Err(super::invariant::InvariantViolation::new(
                    "ThresholdSigning",
                    format!("authority {:?} missing public key package", authority),
                ));
            }
            let participant_set: HashSet<_> = context.participants.iter().collect();
            if participant_set.len() != context.participants.len() {
                return Err(super::invariant::InvariantViolation::new(
                    "ThresholdSigning",
                    format!("authority {:?} has duplicate participants", authority),
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
#[aura_macros::actor_owned(
    owner = "threshold_signing_service",
    domain = "threshold_signing",
    gate = "threshold_signing_command_ingress",
    command = ThresholdSigningCommand,
    capacity = 64,
    category = "actor_owned"
)]
pub struct ThresholdSigningService {
    /// Effect system for crypto and secure storage operations
    effects: Arc<AuraEffectSystem>,
    shared: Arc<ThresholdSigningShared>,
    /// Runtime capability admission snapshot for threshold-signing gates.
    runtime_capabilities: RuntimeCapabilityHandler,
}

struct ThresholdSigningShared {
    quorum: enrollment_quorum_registry::EnrollmentQuorumRegistry,
    /// Co-signer rounds of device quorum signing (Task 163).
    device_quorum: device_quorum::DeviceQuorumSlots,
    /// Serializes signing-material lifecycle changes across cloned handles.
    transitions: Mutex<()>,
    /// In-memory signing state (contexts + leases)
    state: RwLock<ThresholdSigningState>,
    /// Authoritative lifecycle state for runtime health.
    lifecycle: RwLock<ServiceHealth>,
}

impl std::fmt::Debug for ThresholdSigningService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThresholdSigningService")
            .field("shared", &"<ThresholdSigningShared>")
            .finish()
    }
}

impl Clone for ThresholdSigningService {
    fn clone(&self) -> Self {
        Self {
            effects: self.effects.clone(),
            shared: Arc::clone(&self.shared),
            runtime_capabilities: self.runtime_capabilities.clone(),
        }
    }
}

/// Exact retained pending generation. Only the signing service validates and mints it.
///
/// ```compile_fail
/// use aura_agent::runtime::services::threshold_signing::VerifiedPendingSigningGeneration;
/// let forged: VerifiedPendingSigningGeneration = serde_json::from_str("{}").unwrap();
/// ```

#[derive(Debug, Clone)]
pub(crate) struct VerifiedPendingSigningGeneration {
    authority: AuthorityId,
    epoch: u64,
    package_digest: [u8; 32],
    config_digest: [u8; 32],
}

impl VerifiedPendingSigningGeneration {
    pub(crate) fn authority(&self) -> AuthorityId {
        self.authority
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(crate) fn package_digest(&self) -> [u8; 32] {
        self.package_digest
    }
    pub(crate) fn config_digest(&self) -> [u8; 32] {
        self.config_digest
    }
}

impl ThresholdSigningService {
    /// Export the actual runtime device and one coherent signing-context
    /// snapshot. The read guard prevents context replacement until the signed
    /// request is retained; signing uses that snapshot without reacquiring the
    /// context lock. The proof itself is checked before any code escapes.
    pub(crate) async fn export_device_enrollment_setup_request(
        &self,
        authority: AuthorityId,
    ) -> Result<String, EnrollmentSetupExportError> {
        let _transition = self.shared.transitions.lock().await;
        let now = self.effects.physical_time().await?.ts_ms;
        let expires_at_ms = now
            .checked_add(15 * 60 * 1000)
            .ok_or(EnrollmentSetupExportError::TimeOverflow)?;
        let nonce = self.effects.random_bytes_32().await;
        let guard = self.shared.state.read().await;
        let state = guard
            .contexts
            .get(&authority)
            .ok_or(EnrollmentSetupExportError::MissingSigningContext(authority))?;
        if state.my_signer_index.is_none() {
            return Err(EnrollmentSetupExportError::NotParticipant);
        }
        let statement = DeviceEnrollmentSetupStatement {
            version: DeviceEnrollmentSetupRequest::VERSION,
            authority,
            device: self.effects.device_id(),
            nonce,
            issued_at_ms: now,
            expires_at_ms,
            signing_epoch: state.epoch,
            signing_mode: state.mode,
            threshold: state.config.threshold,
            participants: state.config.total_participants,
            public_key_package: state.public_key_package.clone(),
        };
        let context = DeviceEnrollmentSetupRequest::signing_context(&statement)?;
        let message = Self::serialize_signing_context(&context, state.epoch)?;
        let proof = match state.mode {
            SigningMode::SingleSigner => self.sign_solo(&authority, &message, state).await?,
            SigningMode::Threshold => {
                self.runtime_capabilities
                    .require_capabilities(&[CapabilityKey::new("byzantine_envelope")])
                    .await?;
                return Err(self
                    .unowned_threshold_route_error(&authority, state.epoch)
                    .await
                    .into());
            }
        };
        let request = DeviceEnrollmentSetupRequest { statement, proof };
        // Detect mismatched secure key material as well as epoch/package drift.
        request
            .clone()
            .verify_possession(self.effects.as_ref(), now)
            .await?;
        let code = request.encode()?;
        let location = SecureStorageLocation::with_sub_key(
            "device_enrollment_setup",
            format!("{authority}:{}", hex::encode(nonce)),
            "request",
        );
        self.effects
            .secure_store(
                &location,
                code.as_bytes(),
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(EnrollmentSetupExportError::Storage)?;
        drop(guard);
        Ok(code)
    }

    /// Create a new threshold signing service
    pub fn new(effects: Arc<AuraEffectSystem>) -> Self {
        let runtime_capabilities =
            RuntimeCapabilityHandler::from_pairs([("byzantine_envelope", true)]);
        Self {
            effects,
            shared: Arc::new(ThresholdSigningShared {
                quorum: enrollment_quorum_registry::EnrollmentQuorumRegistry::new(),
                device_quorum: device_quorum::DeviceQuorumSlots::default(),
                transitions: Mutex::new(()),
                state: RwLock::new(ThresholdSigningState::default()),
                lifecycle: RwLock::new(ServiceHealth::NotStarted),
            }),
            runtime_capabilities,
        }
    }

    /// Create a threshold signing service with explicit runtime capabilities.
    pub fn with_runtime_capabilities(
        effects: Arc<AuraEffectSystem>,
        runtime_capabilities: RuntimeCapabilityHandler,
    ) -> Self {
        Self {
            effects,
            shared: Arc::new(ThresholdSigningShared {
                quorum: enrollment_quorum_registry::EnrollmentQuorumRegistry::new(),
                device_quorum: device_quorum::DeviceQuorumSlots::default(),
                transitions: Mutex::new(()),
                state: RwLock::new(ThresholdSigningState::default()),
                lifecycle: RwLock::new(ServiceHealth::NotStarted),
            }),
            runtime_capabilities,
        }
    }

    /// Read effective agreement only from the original reverified committed
    /// profile receipt. Signed imported configuration bytes remain immutable.
    async fn read_original_effective_signing_policy(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        protected_config: &[u8],
    ) -> Result<ThresholdConfigMetadata, AuraError> {
        if protected_config.len() > 131_072 {
            return Err(AuraError::invalid(
                "original signing configuration exceeds bounds",
            ));
        }
        let confirmation = super::enrollment_profile::load_original_committed_profile_confirmation(
            self.effects.as_ref(),
        )
        .await?;
        if let Some(confirmed) = confirmation.as_ref().filter(|confirmed| {
            confirmed.confirmation().manifest().subject == *authority
                && confirmed.confirmation().manifest().pending_epoch == epoch
        }) {
            let manifest = confirmed.confirmation().manifest();
            if manifest.invitee_device != self.effects.device_id()
                || aura_core::hash::hash(protected_config)
                    != *manifest.pending_threshold_config_digest.as_bytes()
            {
                return Err(crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                ));
            }
            let public = self
                .effects
                .secure_retrieve(
                    &SecureStorageLocation::with_sub_key(
                        "threshold_pubkey",
                        authority.to_string(),
                        epoch.to_string(),
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await?;
            if aura_core::hash::hash(&public) != manifest.pending_public_key_package_digest {
                return Err(crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                ));
            }
            // Missing or corrupt original envelope is a required read failure,
            // never permission to finalize raw metadata or allocate new custody.
            let owner = self
                .effects
                .load_confirmed_activation_envelope(confirmed)
                .await?;
            return self
                .effects
                .confirmed_activation_finalized_config(&owner)
                .await;
        }
        serde_json::from_slice(protected_config).map_err(|source| AuraError::Serialization {
            message: "decode original retained signing policy".into(),
            source: Some(Arc::new(source)),
        })
    }

    async fn load_retained_signing_candidate(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<
        (
            SigningContextState,
            ThresholdConfigMetadata,
            SecureStorageLocation,
        ),
        AuraError,
    > {
        let confirmation = super::enrollment_profile::load_original_committed_profile_confirmation(
            self.effects.as_ref(),
        )
        .await?;
        let confirmed = confirmation.as_ref().filter(|confirmed| {
            confirmed.confirmation().manifest().subject == *authority
                && confirmed.confirmation().manifest().pending_epoch == new_epoch
        });
        self.load_retained_signing_candidate_with_confirmation(authority, new_epoch, confirmed)
            .await
    }

    async fn load_retained_signing_candidate_with_confirmation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
        confirmed: Option<&crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability>,
    ) -> Result<
        (
            SigningContextState,
            ThresholdConfigMetadata,
            SecureStorageLocation,
        ),
        AuraError,
    > {
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
            .map_err(|error| AuraError::Storage {
                message: "load retained pending public package".into(),
                source: Some(Arc::new(error)),
            })?;

        // Load threshold config metadata stored during rotate_keys.
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", new_epoch),
        );

        let config_metadata: ThresholdConfigMetadata = self
            .effects
            .secure_retrieve(
                &config_location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|error| AuraError::Storage {
                message: "load retained pending config".into(),
                source: Some(Arc::new(error)),
            })
            .and_then(|bytes| {
                if let Some(confirmed) = confirmed {
                    let manifest = confirmed.confirmation().manifest();
                    if manifest.subject != *authority
                        || manifest.pending_epoch != new_epoch
                        || aura_core::hash::hash(&bytes)
                            != *manifest.pending_threshold_config_digest.as_bytes()
                        || aura_core::hash::hash(&public_key_package)
                            != manifest.pending_public_key_package_digest
                    {
                        return Err(crate::runtime::effects::held_registration_error(
                            crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                        ));
                    }
                }
                serde_json::from_slice(&bytes).map_err(|e| AuraError::Internal {
                    message: "decode retained pending config".into(),
                    source: Some(Arc::new(e)),
                })
            })?;

        let new_config =
            ThresholdConfig::new(config_metadata.threshold_k, config_metadata.total_n)?;
        let participants = config_metadata.resolved_participants();
        if participants.len() != usize::from(config_metadata.total_n)
            || participants.iter().collect::<HashSet<_>>().len() != participants.len()
            || (config_metadata.mode == SigningMode::SingleSigner
                && (config_metadata.threshold_k != 1 || config_metadata.total_n != 1))
            || (config_metadata.mode == SigningMode::Threshold && config_metadata.threshold_k < 2)
        {
            return Err(AuraError::storage("pending signing policy is inconsistent"));
        }
        let device_id = self.effects.device_id();
        let my_signer_index = participants
            .iter()
            .position(|participant| match participant {
                ParticipantIdentity::Device(id) => *id == device_id,
                ParticipantIdentity::Guardian(id) => id == authority,
                ParticipantIdentity::GroupMember { .. } => false,
            })
            .map(|index| (index + 1) as u16);
        let candidate = SigningContextState {
            config: new_config,
            my_signer_index,
            epoch: new_epoch,
            public_key_package,
            mode: config_metadata.mode,
            participants,
            agreement_mode: AgreementMode::ConsensusFinalized,
        };
        Self::group_public_key_bytes(&candidate)?;
        self.validate_retained_threshold_signer_with_confirmation(authority, &candidate, confirmed)
            .await?;
        if candidate.mode == SigningMode::SingleSigner && candidate.my_signer_index.is_some() {
            let context = SigningContext::message(
                *authority,
                "aura.pending-generation.restore".to_owned(),
                Vec::new(),
            );
            let message = Self::serialize_signing_context(&context, candidate.epoch)?;
            let proof = self.sign_solo(authority, &message, &candidate).await?;
            if !self
                .effects
                .verify_signature(
                    &message,
                    &proof.signature,
                    &candidate.public_key_package,
                    candidate.mode,
                )
                .await?
            {
                return Err(AuraError::crypto(
                    "pending local signer does not match its public package",
                ));
            }
        }
        Ok((candidate, config_metadata, config_location))
    }

    pub(crate) async fn capture_retained_pending_generation(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<VerifiedPendingSigningGeneration, AuraError> {
        let _transition = self.shared.transitions.lock().await;
        let (candidate, mut config, _) = self
            .load_retained_signing_candidate(authority, epoch)
            .await?;
        config.agreement_mode = AgreementMode::Provisional;
        let config_bytes = serde_json::to_vec(&config).map_err(|error| AuraError::Internal {
            message: "encode pending signing generation".into(),
            source: Some(Arc::new(error)),
        })?;
        Ok(VerifiedPendingSigningGeneration {
            authority: *authority,
            epoch,
            package_digest: aura_core::hash::hash(&candidate.public_key_package),
            config_digest: aura_core::hash::hash(&config_bytes),
        })
    }

    /// Restores eligibility only; it neither changes the active epoch nor publishes readiness.
    pub(crate) async fn verify_retained_pending_generation(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        expected_package_digest: [u8; 32],
        expected_config_digest: [u8; 32],
    ) -> Result<VerifiedPendingSigningGeneration, AuraError> {
        let _transition = self.shared.transitions.lock().await;
        let (candidate, mut config, _) = self
            .load_retained_signing_candidate(authority, epoch)
            .await?;
        if let Some(active) = self.shared.state.read().await.contexts.get(authority) {
            if active.epoch > epoch
                || (active.epoch == epoch
                    && (active.public_key_package != candidate.public_key_package
                        || active.config.threshold != candidate.config.threshold
                        || active.config.total_participants != candidate.config.total_participants
                        || active.participants != candidate.participants
                        || active.mode != candidate.mode))
            {
                return Err(AuraError::invalid(
                    "restored signing generation conflicts with active epoch",
                ));
            }
        } else {
            return Err(AuraError::invalid(
                "pending generation restore requires active signing bootstrap",
            ));
        }
        // Agreement is monotone activation metadata, not signing-generation identity.
        config.agreement_mode = AgreementMode::Provisional;
        let package_digest = aura_core::hash::hash(&candidate.public_key_package);
        let config_digest =
            aura_core::hash::hash(&serde_json::to_vec(&config).map_err(|error| {
                AuraError::Internal {
                    message: "encode pending signing generation".into(),
                    source: Some(Arc::new(error)),
                }
            })?);
        if package_digest != expected_package_digest || config_digest != expected_config_digest {
            return Err(AuraError::invalid(
                "retained pending signing generation was substituted",
            ));
        }
        Ok(VerifiedPendingSigningGeneration {
            authority: *authority,
            epoch,
            package_digest,
            config_digest,
        })
    }

    async fn activate_retained_generation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
        expected: Option<&VerifiedPendingSigningGeneration>,
        held: Option<&crate::runtime::effects::EnrollmentGenerationCustodyCapability<'_>>,
        confirmed: Option<&crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability>,
    ) -> Result<(), AuraError> {
        match (expected, held) {
            (Some(_), Some(owner)) => owner.require_effects(self.effects.as_ref())?,
            (None, None) => {}
            _ => {
                return Err(crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
                ))
            }
        }
        let _generic_generation = if expected.is_none() {
            Some(self.effects.acquire_enrollment_generation_custody().await)
        } else {
            None
        };
        let _transition = self.shared.transitions.lock().await;
        if expected.is_none() && self.effects.secure_exists(
            &crate::handlers::invitation::enrollment_manifest_admission::imported_generation_location(authority,new_epoch),
        ).await? {
            return Err(AuraError::invalid("generic signing activation cannot consume an imported enrollment generation"));
        }
        if expected.is_none()
            && self
                .effects
                .has_live_enrollment_generation(*authority, new_epoch)
                .await?
        {
            return Err(AuraError::invalid(
                "generic signing activation cannot consume an enrollment generation",
            ));
        }
        tracing::info!(
            ?authority,
            new_epoch,
            "Committing key rotation after successful ceremony"
        );

        let (candidate, mut config_metadata, config_location) = self
            .load_retained_signing_candidate_with_confirmation(authority, new_epoch, confirmed)
            .await?;
        if let Some(expected) = expected {
            let mut canonical = config_metadata.clone();
            canonical.agreement_mode = AgreementMode::Provisional;
            let bytes = serde_json::to_vec(&canonical).map_err(|error| AuraError::Internal {
                message: "encode activation generation".into(),
                source: Some(Arc::new(error)),
            })?;
            if expected.authority != *authority
                || expected.epoch != new_epoch
                || expected.package_digest != aura_core::hash::hash(&candidate.public_key_package)
                || expected.config_digest != aura_core::hash::hash(&bytes)
            {
                return Err(AuraError::invalid(
                    "activation generation changed before commit",
                ));
            }
        }
        let existing = self
            .shared
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .cloned();
        if let Some(existing) = &existing {
            if new_epoch < existing.epoch
                || (new_epoch == existing.epoch
                    && (candidate.public_key_package != existing.public_key_package
                        || candidate.config.threshold != existing.config.threshold
                        || candidate.config.total_participants
                            != existing.config.total_participants
                        || candidate.participants != existing.participants
                        || candidate.mode != existing.mode))
            {
                return Err(AuraError::invalid(
                    "stale or substituted signing epoch commit",
                ));
            }
        }
        let epoch_location = SecureStorageLocation::new("epoch_state", authority.to_string());
        if self.effects.secure_exists(&epoch_location).await? {
            let bytes = self
                .effects
                .secure_retrieve(&epoch_location, &[SecureStorageCapability::Read])
                .await?;
            let active =
                u64::from_le_bytes(bytes.try_into().map_err(|_| {
                    AuraError::storage("persisted signing epoch has invalid length")
                })?);
            if new_epoch < active {
                return Err(AuraError::invalid(
                    "cannot commit an older persisted signing epoch",
                ));
            }
        }
        if confirmed.is_none()
            && config_metadata.agreement_mode != AgreementMode::ConsensusFinalized
        {
            config_metadata.agreement_mode = AgreementMode::ConsensusFinalized;
            let updated_bytes =
                serde_json::to_vec(&config_metadata).map_err(|e| AuraError::Internal {
                    message: "encode activated signing config".into(),
                    source: Some(Arc::new(e)),
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
                .map_err(|e| AuraError::Storage {
                    message: "persist activated signing config".into(),
                    source: Some(Arc::new(e)),
                })?;
        }

        // Persist activation before exposing the new in-memory context.
        self.effects
            .secure_store(
                &epoch_location,
                &new_epoch.to_le_bytes(),
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;

        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.contexts.insert(*authority, candidate);
            },
            |state| state.validate(),
        )
        .await;

        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_pending_signing_generation",
        capability_type = EnrollmentActivationCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn commit_verified_pending_generation(
        &self,
        expected: &VerifiedPendingSigningGeneration,
        activation: &crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> Result<(), AuraError> {
        activation
            .generation()
            .require_effects(self.effects.as_ref())?;
        activation
            .require_generation(expected.authority, expected.epoch)
            .await?;
        self.effects
            .seal_owned_enrollment_wrapping_allocations(
                activation,
                expected.authority,
                expected.epoch,
            )
            .await?;
        self.activate_retained_generation(
            &expected.authority,
            expected.epoch,
            Some(expected),
            Some(activation.generation()),
            None,
        )
        .await
    }

    fn transcript_store(&self) -> StorageTranscriptStore<AuraEffectSystem> {
        StorageTranscriptStore::new_default(self.effects.clone())
    }

    async fn validate_retained_threshold_signer(
        &self,
        authority: &AuthorityId,
        state: &SigningContextState,
    ) -> Result<(), AuraError> {
        self.validate_retained_threshold_signer_with_confirmation(authority, state, None)
            .await
    }

    async fn validate_retained_threshold_signer_with_confirmation(
        &self,
        authority: &AuthorityId,
        state: &SigningContextState,
        confirmed: Option<&crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability>,
    ) -> Result<(), AuraError> {
        if state.mode != SigningMode::Threshold {
            return Ok(());
        }
        let Some(index) = state.my_signer_index else {
            return Ok(());
        };
        let participant = state
            .participants
            .get(usize::from(index - 1))
            .ok_or_else(|| {
                AuraError::invalid("retained signer index is outside its participant inventory")
            })?;
        let location = Self::participant_share_location(authority, state.epoch, participant);
        let key = zeroize::Zeroizing::new(match confirmed {
            Some(confirmed) => {
                self.confirmed_participant_key_package(
                    confirmed,
                    authority,
                    state.epoch,
                    participant,
                )
                .await?
            }
            None => {
                self.retrieve_participant_key_package(
                    authority,
                    state.epoch,
                    participant,
                    &location,
                )
                .await?
            }
        });
        tree_signing::validate_retained_threshold_key_package(
            &key,
            &state.public_key_package,
            index,
            state.config.threshold,
            state.config.total_participants,
        )
        .map_err(|error| {
            AuraError::crypto_with_source(
                "retained local threshold share does not match its signing context",
                Arc::new(error),
            )
        })
    }

    async fn ensure_bootstrap_device_leaf(
        &self,
        authority: &AuthorityId,
        signing: &SigningContextState,
        allow_creation: bool,
    ) -> Result<[u8; 32], AuraError> {
        let public_key_package = &signing.public_key_package;
        let current_device_id = self.effects.device_id();
        let tree_state = self.effects.get_current_state().await?;
        let verifying_key =
            SingleSignerPublicKeyPackage::from_bytes(public_key_package)?.verifying_key;
        let current_leaf = tree_state
            .leaves
            .values()
            .find(|leaf| leaf.role == LeafRole::Device && leaf.device_id == current_device_id);
        let operations = self
            .effects
            .export_tree_ops()
            .await
            .map_err(|error| match error {
                crate::core::AgentError::Aura(source) => source,
                source => AuraError::Storage {
                    message: "bootstrap tree export failed".to_owned(),
                    source: Some(Arc::new(source)),
                },
            })?;
        if let Some(leaf) = current_leaf {
            if leaf.public_key.as_ref() != verifying_key.as_slice() {
                return Err(BootstrapGenesisError::DeviceKeyMismatch.into());
            }
            for operation in &operations {
                if operation.op.parent_epoch != Epoch::initial()
                    || operation.op.parent_commitment != [0; 32]
                {
                    continue;
                }
                if let aura_core::tree::TreeOpKind::AddLeaf {
                    leaf: created,
                    under,
                } = &operation.op.op
                {
                    if *under == NodeIndex(0)
                        && created.leaf_id == leaf.leaf_id
                        && created.device_id == current_device_id
                        && created.role == LeafRole::Device
                        && created.public_key.as_ref() == verifying_key.as_slice()
                    {
                        let message = Self::tree_op_message(&operation.op, signing)?;
                        if operation.signer_count != 1
                            || !self
                                .effects
                                .verify_signature(
                                    &message,
                                    &operation.agg_sig,
                                    public_key_package,
                                    SigningMode::SingleSigner,
                                )
                                .await?
                        {
                            return Err(BootstrapGenesisError::InvalidCreationSignature.into());
                        }
                        return Ok(aura_core::hash::hash(
                            &aura_core::util::serialization::to_vec(operation)?,
                        ));
                    }
                }
            }
            return Err(BootstrapGenesisError::MissingCreation.into());
        }
        if !allow_creation
            || !operations.is_empty()
            || !tree_state.leaves.is_empty()
            || tree_state.epoch != Epoch::initial()
            || tree_state.root_commitment != [0; 32]
        {
            return Err(BootstrapGenesisError::MissingCreation.into());
        }

        let next_leaf_id = tree_state
            .leaves
            .keys()
            .map(|leaf_id| leaf_id.0)
            .max()
            .unwrap_or(0)
            + 1;

        let leaf_metadata = DeviceLeafMetadata::new()
            .with_enrolled_at_ms(self.effects.physical_time().await?.ts_ms)
            .encode()?;

        let leaf = LeafNode::new(
            LeafId(next_leaf_id),
            current_device_id,
            LeafRole::Device,
            verifying_key,
            leaf_metadata,
        )?;

        let op_kind = self.effects.add_leaf(leaf, NodeIndex(0)).await?;
        let op = TreeOp {
            parent_epoch: tree_state.epoch,
            parent_commitment: tree_state.root_commitment,
            op: op_kind,
            version: 1,
        };

        let signature = self
            .sign_solo(authority, &Self::tree_op_message(&op, signing)?, signing)
            .await?;
        let attested = AttestedOp {
            op,
            agg_sig: signature.signature,
            signer_count: signature.signer_count,
        };

        let digest = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&attested)?);
        self.effects.apply_attested_op(attested).await?;
        Ok(digest)
    }

    fn bootstrap_genesis_location(authority: &AuthorityId) -> SecureStorageLocation {
        SecureStorageLocation::new("bootstrap_genesis", authority.to_string())
    }

    async fn complete_bootstrap_genesis(
        &self,
        authority: &AuthorityId,
        signing: &SigningContextState,
    ) -> Result<(), AuraError> {
        let location = Self::bootstrap_genesis_location(authority);
        let record: Option<BootstrapGenesisRecord> =
            if self.effects.secure_exists(&location).await? {
                let bytes = self
                    .effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await?;
                if bytes.len() > 2048 {
                    return Err(BootstrapGenesisError::RecordSize.into());
                }
                Some(serde_json::from_slice(&bytes)?)
            } else {
                None
            };
        let package_digest = aura_core::hash::hash(&signing.public_key_package);
        if let Some(record) = &record {
            if record.version != 1
                || record.authority != *authority
                || record.device != self.effects.device_id()
                || record.epoch != 0
                || signing.epoch != 0
                || record.public_package_digest != package_digest
            {
                return Err(BootstrapGenesisError::RecordMismatch.into());
            }
        }
        // Only a persisted pending initialization authorizes creation. Legacy
        // keys and completed records require an existing authenticated witness.
        let allow_creation = record
            .as_ref()
            .is_some_and(|record| matches!(record.state, BootstrapGenesisState::Pending));
        let creation_op_digest = self
            .ensure_bootstrap_device_leaf(authority, signing, allow_creation)
            .await?;
        // A cache entry is not a commit witness after a failed tree write.
        let index = self
            .effects
            .retrieve(aura_journal::commitment_tree::storage::TREE_OPS_INDEX_KEY)
            .await?
            .ok_or(BootstrapGenesisError::MissingDurableCreation)?;
        if !aura_journal::commitment_tree::storage::deserialize_op_index(&index)?
            .contains(&creation_op_digest)
        {
            return Err(BootstrapGenesisError::MissingDurableCreation.into());
        }
        let persisted = self
            .effects
            .retrieve(&aura_journal::commitment_tree::storage::op_key(
                creation_op_digest,
            ))
            .await?
            .ok_or(BootstrapGenesisError::MissingDurableCreation)?;
        let operation = aura_journal::commitment_tree::storage::deserialize_op(&persisted)?;
        if aura_journal::commitment_tree::storage::op_hash(&operation)? != creation_op_digest {
            return Err(BootstrapGenesisError::MissingDurableCreation.into());
        }
        if let Some(BootstrapGenesisRecord {
            state:
                BootstrapGenesisState::Complete {
                    creation_op_digest: expected,
                },
            ..
        }) = &record
        {
            if *expected != creation_op_digest {
                return Err(BootstrapGenesisError::CompletionMismatch.into());
            }
            return Ok(());
        }
        let complete = BootstrapGenesisRecord {
            version: 1,
            authority: *authority,
            device: self.effects.device_id(),
            epoch: 0,
            public_package_digest: package_digest,
            state: BootstrapGenesisState::Complete { creation_op_digest },
        };
        self.effects
            .secure_store(
                &location,
                &serde_json::to_vec(&complete)?,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        Ok(())
    }

    /// Convert only a proved original physical-device bootstrap representation.
    /// Caller already holds generation and signing-transition custody.
    async fn migrate_proved_legacy_bootstrap(
        &self,
        authority: &AuthorityId,
        mut restored: SigningContextState,
    ) -> Result<SigningContextState, AuraError> {
        let location = SecureStorageLocation::new(
            "bootstrap_physical_participant_migration_v1",
            authority.to_string(),
        );
        let legacy = ParticipantIdentity::guardian(*authority);
        let physical = ParticipantIdentity::device(self.effects.device_id());
        let prior = self.effects.secure_exists(&location).await?;
        let config_location =
            SecureStorageLocation::with_sub_key("threshold_config", authority.to_string(), "0");
        let current = self
            .effects
            .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
            .await?;
        if current.len() > 131_072 {
            return Err(BootstrapGenesisError::RecordSize.into());
        }
        let metadata: ThresholdConfigMetadata = serde_json::from_slice(&current)?;
        if let Some(origin) = metadata.bootstrap_migration_origin {
            validate_bootstrap_migration_origin(self.effects.as_ref(), authority, 0, origin)
                .await?;
        }
        if restored.participants != vec![legacy.clone()]
            && !prior
            && metadata.bootstrap_migration_origin.is_none()
        {
            return Ok(restored);
        }
        if restored.epoch != 0
            || restored.mode != SigningMode::SingleSigner
            || restored.config.threshold != 1
            || restored.config.total_participants != 1
            || restored.my_signer_index != Some(1)
            || (restored.participants != vec![legacy.clone()]
                && restored.participants != vec![physical.clone()])
        {
            return Err(BootstrapGenesisError::RecordMismatch.into());
        }
        let _tree = self.effects.lock_tree_decision().await;
        let operations =
            self.effects
                .export_tree_ops()
                .await
                .map_err(|source| AuraError::Internal {
                    message: "read original bootstrap migration history".into(),
                    source: Some(Arc::new(source)),
                })?;
        self.effects
            .collect_enrollment_parent_inventory(&operations)
            .await?;
        let state = aura_journal::commitment_tree::reduce(&operations).map_err(|source| {
            AuraError::crypto_with_source(
                "validate original bootstrap migration history",
                Arc::new(source),
            )
        })?;
        if state.epoch.value() != 0
            || state.leaves.len() != 1
            || state.leaves.values().any(|leaf| {
                leaf.role != LeafRole::Device || leaf.device_id != self.effects.device_id()
            })
        {
            return Err(BootstrapGenesisError::RecordMismatch.into());
        }
        let creation_op_digest = self
            .ensure_bootstrap_device_leaf(authority, &restored, false)
            .await?;
        let genesis_location = Self::bootstrap_genesis_location(authority);
        let genesis_bytes = self
            .effects
            .secure_retrieve(&genesis_location, &[SecureStorageCapability::Read])
            .await?;
        if genesis_bytes.len() > 2048 {
            return Err(BootstrapGenesisError::RecordSize.into());
        }
        let genesis: BootstrapGenesisRecord = serde_json::from_slice(&genesis_bytes)?;
        let package_digest = aura_core::hash::hash(&restored.public_key_package);
        if genesis.version != 1
            || genesis.authority != *authority
            || genesis.device != self.effects.device_id()
            || genesis.epoch != 0
            || genesis.public_package_digest != package_digest
            || !matches!(genesis.state, BootstrapGenesisState::Complete { creation_op_digest: digest } if digest == creation_op_digest)
        {
            return Err(BootstrapGenesisError::RecordMismatch.into());
        }
        let original_metadata = ThresholdConfigMetadata {
            threshold_k: 1,
            total_n: 1,
            participants: vec![legacy.clone()],
            mode: SigningMode::SingleSigner,
            agreement_mode: restored.agreement_mode,
            bootstrap_migration_origin: None,
        };
        let original_policy = serde_json::to_vec(&original_metadata)?;
        let old_canonical = Self::participant_share_location(authority, 0, &legacy);
        let old_location = if self.effects.secure_exists(&old_canonical).await? {
            old_canonical
        } else {
            SecureStorageLocation::with_sub_key("signing_keys", format!("{authority}:0"), "1")
        };
        let old_envelope = self
            .effects
            .secure_retrieve(&old_location, &[SecureStorageCapability::Read])
            .await?;
        if old_envelope.len() > 131_072 {
            return Err(BootstrapGenesisError::RecordSize.into());
        }
        let original_secret = zeroize::Zeroizing::new(
            self.decrypt_participant_key_package(authority, 0, &legacy, &old_envelope)
                .await?,
        );
        let original_package = SingleSignerKeyPackage::import_from_secure_storage(
            &original_secret,
            SecretExportContext::secure_storage("aura-agent::legacy-bootstrap-migration"),
        )?;
        let public = Self::group_public_key_bytes(&restored)?;
        let derived = self
            .effects
            .ed25519_public_key(original_package.signing_key())
            .await?;
        if original_package.verifying_key() != public.as_slice()
            || derived.as_slice() != public.as_slice()
        {
            return Err(BootstrapGenesisError::DeviceKeyMismatch.into());
        }
        let decision = LegacyBootstrapMigrationDecision {
            version: 1,
            authority: *authority,
            device: self.effects.device_id(),
            original_policy_digest: aura_core::hash::hash(&original_policy),
            original_envelope_digest: aura_core::hash::hash(&old_envelope),
            public_package_digest: package_digest,
            creation_op_digest,
        };
        let decision_bytes = serde_json::to_vec(&decision)?;
        let origin = aura_core::hash::hash(&decision_bytes);
        let physical_metadata = ThresholdConfigMetadata {
            participants: vec![physical.clone()],
            bootstrap_migration_origin: Some(origin),
            ..original_metadata
        };
        let converted = serde_json::to_vec(&physical_metadata)?;
        if current != original_policy && current != converted {
            return Err(BootstrapGenesisError::RecordMismatch.into());
        }
        if decision_bytes.len() > 4096 {
            return Err(BootstrapGenesisError::RecordSize.into());
        }
        self.effects
            .secure_store_immutable(
                &location,
                &decision_bytes,
                &[SecureStorageCapability::Write],
            )
            .await?;
        let retained = self
            .effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 4096 || retained != decision_bytes {
            return Err(BootstrapGenesisError::RecordMismatch.into());
        }
        let target = Self::participant_share_location(authority, 0, &physical);
        if !self.effects.secure_exists(&target).await? {
            let envelope = self
                .encrypt_participant_key_package(authority, 0, &physical, &original_secret)
                .await?;
            self.effects
                .secure_store_immutable(&target, &envelope, &[SecureStorageCapability::Write])
                .await?;
        }
        let target_bytes = self
            .effects
            .secure_retrieve(&target, &[SecureStorageCapability::Read])
            .await?;
        if target_bytes.len() > 131_072 {
            return Err(BootstrapGenesisError::RecordSize.into());
        }
        let target_secret = zeroize::Zeroizing::new(
            self.decrypt_participant_key_package(authority, 0, &physical, &target_bytes)
                .await?,
        );
        if target_secret != original_secret {
            return Err(BootstrapGenesisError::DeviceKeyMismatch.into());
        }
        if current != converted {
            self.effects
                .secure_store(
                    &config_location,
                    &converted,
                    &[SecureStorageCapability::Write],
                )
                .await?;
        }
        if self
            .effects
            .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
            .await?
            != converted
        {
            return Err(BootstrapGenesisError::RecordMismatch.into());
        }
        let completed = serde_json::to_vec(&(
            1_u16,
            aura_core::hash::hash(&decision_bytes),
            aura_core::hash::hash(&target_bytes),
        ))?;
        let completion_location = SecureStorageLocation::new(
            "bootstrap_physical_participant_migration_completed_v1",
            authority.to_string(),
        );
        self.effects
            .secure_store_immutable(
                &completion_location,
                &completed,
                &[SecureStorageCapability::Write],
            )
            .await?;
        if self
            .effects
            .secure_retrieve(&completion_location, &[SecureStorageCapability::Read])
            .await?
            != completed
        {
            return Err(BootstrapGenesisError::CompletionMismatch.into());
        }
        restored.participants = vec![physical];
        Ok(restored)
    }

    fn participant_share_location(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        )
    }

    fn participant_wrap_key_location(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "participant_share_wrap_keys",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        )
    }

    fn participant_key_package_aad(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Vec<u8> {
        format!(
            "{}:{}:{}:{}",
            PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN,
            authority,
            epoch,
            participant.storage_key()
        )
        .into_bytes()
    }

    async fn load_or_create_participant_wrap_key(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Result<[u8; 32], AuraError> {
        let location = Self::participant_wrap_key_location(authority, epoch, participant);
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        let bytes = if self.effects.secure_exists(&location).await? {
            self.effects.secure_retrieve(&location, &caps).await?
        } else {
            let candidate = self.effects.random_bytes_32().await;
            match self
                .effects
                .secure_store_immutable(&location, &candidate, &caps)
                .await?
            {
                aura_core::effects::secure::ImmutableSecureStoreOutcome::Created => {
                    candidate.to_vec()
                }
                aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists => {
                    self.effects.secure_retrieve(&location, &caps).await?
                }
            }
        };
        let length = bytes.len();
        bytes.try_into().map_err(|_| AuraError::Storage {
            message: "participant share wrapping key has invalid length".into(),
            source: Some(Arc::new(InvalidParticipantWrappingKeyLength {
                actual: length,
            })),
        })
    }

    async fn encrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        key_package: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let wrap_key = self
            .load_or_create_participant_wrap_key(authority, epoch, participant)
            .await?;
        let cipher = ChaCha20Poly1305::new((&wrap_key).into());
        let nonce = self.effects.random_bytes(12).await;
        let aad = Self::participant_key_package_aad(authority, epoch, participant);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: key_package,
                    aad: &aad,
                },
            )
            .map_err(|source| AuraError::Crypto {
                message: "encrypt participant key package".into(),
                source: Some(Arc::new(source)),
            })?;
        let envelope = ParticipantKeyPackageEnvelope {
            version: PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION,
            authority: *authority,
            epoch,
            recipient: participant.clone(),
            nonce,
            ciphertext,
        };
        serde_json::to_vec(&envelope).map_err(|source| AuraError::Serialization {
            message: "serialize participant key package envelope".into(),
            source: Some(Arc::new(source)),
        })
    }

    async fn decrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        envelope_bytes: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        // The effect owner dispatches the versioned envelope and validates original
        // allocation custody for v2; the service must not reinterpret it as v1.
        self.effects
            .decrypt_participant_key_package(authority, epoch, participant, envelope_bytes)
            .await
    }

    async fn store_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        location: &SecureStorageLocation,
        key_package: &[u8],
    ) -> Result<(), AuraError> {
        let envelope = self
            .encrypt_participant_key_package(authority, epoch, participant, key_package)
            .await?;
        self.effects
            .secure_store(
                location,
                &envelope,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|source| AuraError::Storage {
                message: "store participant key package envelope".into(),
                source: Some(Arc::new(source)),
            })
    }

    async fn retrieve_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, AuraError> {
        let confirmation = super::enrollment_profile::load_original_committed_profile_confirmation(
            self.effects.as_ref(),
        )
        .await?;
        if let Some(confirmed) = &confirmation {
            let manifest = confirmed.confirmation().manifest();
            if manifest.subject == *authority
                && manifest.pending_epoch == epoch
                && manifest.invitee_device == self.effects.device_id()
                && *participant == ParticipantIdentity::device(self.effects.device_id())
            {
                return self
                    .confirmed_participant_key_package(confirmed, authority, epoch, participant)
                    .await;
            }
        }
        let envelope = self
            .effects
            .secure_retrieve(location, &[SecureStorageCapability::Read])
            .await
            .map_err(|source| AuraError::Storage {
                message: "load participant key package envelope".into(),
                source: Some(Arc::new(source)),
            })?;
        self.decrypt_participant_key_package(authority, epoch, participant, &envelope)
            .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "DurableConfirmedEnrollmentCapability",
        family = "runtime_helper"
    )]
    async fn confirmed_participant_key_package(
        &self,
        confirmed: &crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Result<Vec<u8>, AuraError> {
        let manifest = confirmed.confirmation().manifest();
        if manifest.subject != *authority
            || manifest.pending_epoch != epoch
            || *participant != ParticipantIdentity::device(manifest.invitee_device)
        {
            return Err(AuraError::permission_denied(
                "confirmed signing reference differs from original generation",
            ));
        }
        let owner = self
            .effects
            .load_confirmed_activation_envelope(confirmed)
            .await?;
        self.effects
            .decrypt_confirmed_activation_envelope(&owner)
            .await
    }

    /// Activate only the exact imported generation after the immutable pinned
    /// issuer Committed receipt has been reverified. No cached-status shortcut.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "durable_confirmed_enrollment",
        capability_type = DurableConfirmedEnrollmentCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn activate_confirmed_enrollment(
        &self,
        confirmed:crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability,
    ) -> Result<(), AuraError> {
        let _generation = self.effects.acquire_enrollment_generation_custody().await;
        crate::handlers::invitation::enrollment_manifest_admission::require_confirmed_import_generation(self.effects.as_ref(),&confirmed)
            .await.map_err(|source| AuraError::PermissionDenied { message:"import generation confirmation owner mismatch".into(),source:Some(Arc::new(source)) })?;
        // Keep the actual tree mutation lease through all durable key activation.
        let _current_tree = self
            .effects
            .install_confirmed_enrollment_transition(&confirmed)
            .await?;
        crate::handlers::invitation::enrollment_parent_archive::retain_confirmed_parent_archive(
            self.effects.as_ref(),
            &confirmed,
        )
        .await?;
        let archive=crate::handlers::invitation::enrollment_parent_archive::load_confirmed_parent_archive_from_confirmed(self.effects.as_ref(),&confirmed).await?;
        let history =
            self.effects
                .export_tree_ops()
                .await
                .map_err(|source| AuraError::Internal {
                    message: "read held confirmed imported parent history".into(),
                    source: Some(Arc::new(source)),
                })?;
        self.effects
            .collect_imported_enrollment_parent_inventory(&archive, &history)
            .await?;
        let manifest = confirmed.confirmation().manifest();
        let authority = &manifest.subject;
        let epoch = manifest.pending_epoch;
        let participant = ParticipantIdentity::device(self.effects.device_id());
        let share_location = Self::participant_share_location(authority, epoch, &participant);
        let stored = zeroize::Zeroizing::new(
            self.effects
                .secure_retrieve(&share_location, &[SecureStorageCapability::Read])
                .await?,
        );
        if aura_core::hash::hash(&stored) != manifest.pending_share_digest {
            return Err(AuraError::invalid(
                "original immutable imported share differs from confirmed manifest",
            ));
        }
        // Preserve signed raw import bytes. This distinct immutable envelope
        // is minted only by the original confirmed generation capability.
        let activation = self
            .effects
            .retain_confirmed_activation_envelope(&confirmed)
            .await?;
        let (candidate, mut metadata, _) = self
            .load_retained_signing_candidate_with_confirmation(
                authority,
                epoch,
                Some(activation.confirmed()),
            )
            .await?;
        metadata.agreement_mode = AgreementMode::Provisional;
        let config = serde_json::to_vec(&metadata).map_err(|source| AuraError::Serialization {
            message: "encode confirmed imported generation config".into(),
            source: Some(Arc::new(source)),
        })?;
        if candidate.my_signer_index.is_none()
            || aura_core::hash::hash(&candidate.public_key_package)
                != manifest.pending_public_key_package_digest
            || aura_core::hash::hash(&config)
                != *manifest.pending_threshold_config_digest.as_bytes()
        {
            return Err(AuraError::invalid(
                "actual imported signing generation differs from committed manifest",
            ));
        }
        let expected = VerifiedPendingSigningGeneration {
            authority: *authority,
            epoch,
            package_digest: manifest.pending_public_key_package_digest,
            config_digest: *manifest.pending_threshold_config_digest.as_bytes(),
        };
        // This owner holds the generation gate. The activation helper enforces
        // persisted/in-memory monotone epoch and exact generation checks, then
        // retains the signed configuration, derives finalized policy from the
        // receipt, and durably updates the epoch before exposing signing context.
        self.activate_retained_generation(
            authority,
            epoch,
            Some(&expected),
            Some(&_generation),
            Some(activation.confirmed()),
        )
        .await
    }

    /// Decrypted key package stored for `participant` at `epoch` during a rotation.
    pub(crate) async fn participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Result<Vec<u8>, AuraError> {
        let location = Self::participant_share_location(authority, epoch, participant);
        self.retrieve_participant_key_package(authority, epoch, participant, &location)
            .await
    }

    /// Return the current local key-agreement secret for the active signing
    /// participant on this device.
    pub async fn current_local_key_agreement_secret(
        &self,
        authority: &AuthorityId,
    ) -> Result<[u8; 32], AuraError> {
        let state = self.shared.state.read().await;
        let context = state
            .contexts
            .get(authority)
            .cloned()
            .ok_or_else(|| AuraError::not_found("authority context not found"))?;
        drop(state);

        match context.mode {
            SigningMode::SingleSigner => {
                let (participant, location) = self
                    .require_local_solo_share_location(authority, &context)
                    .await?;
                let key_package = self
                    .retrieve_participant_key_package(
                        authority,
                        context.epoch,
                        &participant,
                        &location,
                    )
                    .await?;
                let package = SingleSignerKeyPackage::import_from_secure_storage(
                    &key_package,
                    SecretExportContext::secure_storage(
                        "aura-agent::runtime::services::threshold_signing::current_local_key_agreement_secret",
                    ),
                )
                .map_err(|error| {
                    AuraError::internal(format!(
                        "failed to decode single-signer key package for key agreement: {error}"
                    ))
                })?;
                let mut scalar = [0u8; 32];
                scalar.copy_from_slice(package.signing_key());
                Ok(scalar)
            }
            SigningMode::Threshold => {
                let signer_index = context.my_signer_index.ok_or_else(|| {
                    AuraError::invalid(
                        "current device is not an active signer for this authority context",
                    )
                })?;
                let participant = context
                    .participants
                    .get(usize::from(signer_index.saturating_sub(1)))
                    .cloned()
                    .ok_or_else(|| {
                        AuraError::internal(
                            "local signer index is out of bounds for threshold participants",
                        )
                    })?;
                let location =
                    Self::participant_share_location(authority, context.epoch, &participant);
                let key_package = self
                    .retrieve_participant_key_package(
                        authority,
                        context.epoch,
                        &participant,
                        &location,
                    )
                    .await?;
                let share = tree_signing::share_from_key_package_bytes(&key_package)?;
                if share.value.len() != 32 {
                    return Err(AuraError::internal(
                        "threshold signing share must be 32 bytes for key agreement",
                    ));
                }
                let mut scalar = [0u8; 32];
                scalar.copy_from_slice(&share.value);
                Ok(scalar)
            }
        }
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
            &self.shared.state,
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
            &self.shared.state,
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
        let state = self.shared.state.read().await;
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
        let state = self.shared.state.read().await;
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
    fn require_local_solo_participant(
        &self,
        authority: &AuthorityId,
        state: &SigningContextState,
    ) -> Result<ParticipantIdentity, AuraError> {
        if state.mode != SigningMode::SingleSigner
            || state.config.threshold != 1
            || state.config.total_participants != 1
            || state.my_signer_index != Some(1)
            || state.participants.len() != 1
        {
            return Err(AuraError::crypto(
                "invalid authoritative local single-signer context",
            ));
        }
        let participant = state.participants[0].clone();
        match &participant {
            ParticipantIdentity::Device(id) if *id == self.effects.device_id() => Ok(participant),
            ParticipantIdentity::Guardian(id) if id == authority => Ok(participant),
            _ => Err(AuraError::crypto(
                "single-signer context does not own this physical signer",
            )),
        }
    }
    /// Select only the exact participant/epoch authorized by this owned context.
    /// Canonical presence commits the required read to it, including corruption.
    /// Explicit absence alone permits the historical solo-layout companion.
    async fn require_local_solo_share_location(
        &self,
        authority: &AuthorityId,
        state: &SigningContextState,
    ) -> Result<(ParticipantIdentity, SecureStorageLocation), AuraError> {
        let metadata_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            authority.to_string(),
            state.epoch.to_string(),
        );
        let metadata_bytes = self
            .effects
            .secure_retrieve(&metadata_location, &[SecureStorageCapability::Read])
            .await?;
        if metadata_bytes.len() > 131_072 {
            return Err(BootstrapGenesisError::RecordSize.into());
        }
        let metadata: ThresholdConfigMetadata = serde_json::from_slice(&metadata_bytes)?;
        if let Some(origin) = metadata.bootstrap_migration_origin {
            validate_bootstrap_migration_origin(
                self.effects.as_ref(),
                authority,
                state.epoch,
                origin,
            )
            .await?;
        }
        let participant = self.require_local_solo_participant(authority, state)?;
        let canonical = Self::participant_share_location(authority, state.epoch, &participant);
        let location = if self.effects.secure_exists(&canonical).await? {
            canonical
        } else {
            SecureStorageLocation::with_sub_key(
                "signing_keys",
                format!("{}:{}", authority, state.epoch),
                "1",
            )
        };
        Ok((participant, location))
    }

    async fn sign_solo_ed25519(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        tracing::debug!(?authority, "Signing with Ed25519 single-signer");

        let (participant, location) = self
            .require_local_solo_share_location(authority, state)
            .await?;

        let key_package = self
            .retrieve_participant_key_package(authority, state.epoch, &participant, &location)
            .await?;

        // Direct Ed25519 signing (no FROST overhead)
        let signature = self
            .effects
            .sign_with_key(message, &key_package, SigningMode::SingleSigner)
            .await?;

        tracing::info!(?authority, "Ed25519 single-signer signing complete");

        Ok(ThresholdSignature::single_signer(
            signature,
            state.public_key_package.clone(),
            state.epoch,
        ))
    }

    /// Serialize non-tree signing context for signing.
    fn serialize_signing_context(
        context: &SigningContext,
        epoch: u64,
    ) -> Result<Vec<u8>, AuraError> {
        threshold_signing_context_transcript_bytes(context, epoch).map_err(|e| {
            AuraError::internal(format!("Failed to encode signing context transcript: {e}"))
        })
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
                    AuraError::crypto_with_source(
                        "Decode single-signer public key package",
                        Arc::new(e),
                    )
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
                            AuraError::crypto_with_source(
                                "Decode threshold public key package",
                                Arc::new(e),
                            )
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

    /// Validate required native policy and the actual local share before denying a raw quorum route.
    async fn unowned_threshold_route_error(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> AuraError {
        // This required validator checks native public policy and the actual
        // local encrypted participant package before QuorumOwnerRequired. It
        // never loads another physical participant's private share.
        match self
            .effects
            .require_local_physical_solo_identity_policy(authority, epoch)
            .await
        {
            Err(source) => source,
            Ok(_) => AuraError::Crypto {
                message: "cached threshold context contradicts protected current signing mode"
                    .into(),
                source: Some(Arc::new(UnownedThresholdContextModeError)),
            },
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ThresholdSigningEffects for ThresholdSigningService {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        // Lock order: generation custody precedes signing transitions and tree decisions.
        let _generation = self.effects.enrollment_retirement_generation_guard().await;
        let _transition = self.shared.transitions.lock().await;
        if let Some(state) = self.shared.state.read().await.contexts.get(authority) {
            return Ok(state.public_key_package.clone());
        }
        let epoch_location = SecureStorageLocation::new("epoch_state", authority.to_string());
        let has_epoch = self.effects.secure_exists(&epoch_location).await?;
        let has_genesis_record = self
            .effects
            .secure_exists(&Self::bootstrap_genesis_location(authority))
            .await?;
        if !has_epoch && has_genesis_record {
            let bytes = self
                .effects
                .secure_retrieve(
                    &Self::bootstrap_genesis_location(authority),
                    &[SecureStorageCapability::Read],
                )
                .await?;
            if bytes.len() > 2048 {
                return Err(BootstrapGenesisError::RecordSize.into());
            }
            let record: BootstrapGenesisRecord = serde_json::from_slice(&bytes)?;
            if !matches!(record.state, BootstrapGenesisState::Pending) {
                return Err(BootstrapGenesisError::MissingActiveEpoch.into());
            }
        }
        if has_epoch || has_genesis_record {
            let epoch = if has_epoch {
                let bytes = self
                    .effects
                    .secure_retrieve(&epoch_location, &[SecureStorageCapability::Read])
                    .await?;
                u64::from_le_bytes(bytes.try_into().map_err(|_| {
                    AuraError::storage("persisted signing epoch has invalid length")
                })?)
            } else {
                0
            };
            let config_location = SecureStorageLocation::with_sub_key(
                "threshold_config",
                authority.to_string(),
                epoch.to_string(),
            );
            let protected_config = self
                .effects
                .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
                .await?;
            let metadata = self
                .read_original_effective_signing_policy(authority, epoch, &protected_config)
                .await?;
            let config = ThresholdConfig::new(metadata.threshold_k, metadata.total_n)?;
            let participants = metadata.resolved_participants();
            if participants.len() != usize::from(metadata.total_n)
                || participants.iter().collect::<HashSet<_>>().len() != participants.len()
                || (metadata.mode == SigningMode::SingleSigner
                    && (metadata.threshold_k != 1 || metadata.total_n != 1))
                || (metadata.mode == SigningMode::Threshold && metadata.threshold_k < 2)
            {
                return Err(AuraError::storage(
                    "persisted signing policy is inconsistent",
                ));
            }
            let device = self.effects.device_id();
            let my_signer_index = participants
                .iter()
                .position(|participant| match participant {
                    ParticipantIdentity::Device(id) => *id == device,
                    ParticipantIdentity::Guardian(id) => id == authority,
                    ParticipantIdentity::GroupMember { .. } => false,
                })
                .map(|index| (index + 1) as u16);
            let pubkey_location = SecureStorageLocation::with_sub_key(
                "threshold_pubkey",
                authority.to_string(),
                epoch.to_string(),
            );
            let public_key_package = self
                .effects
                .secure_retrieve(&pubkey_location, &[SecureStorageCapability::Read])
                .await?;
            if let Some(origin) = metadata.bootstrap_migration_origin {
                validate_bootstrap_migration_origin(
                    self.effects.as_ref(),
                    authority,
                    epoch,
                    origin,
                )
                .await?;
            }
            let restored = SigningContextState {
                config,
                my_signer_index,
                epoch,
                public_key_package: public_key_package.clone(),
                mode: metadata.mode,
                participants,
                agreement_mode: metadata.agreement_mode,
            };
            Self::group_public_key_bytes(&restored)?;
            self.validate_retained_threshold_signer(authority, &restored)
                .await?;
            if restored.mode == SigningMode::SingleSigner && restored.my_signer_index.is_some() {
                let context = SigningContext::message(
                    *authority,
                    "aura.signing-context.restore".to_owned(),
                    Vec::new(),
                );
                let message = Self::serialize_signing_context(&context, restored.epoch)?;
                let proof = self.sign_solo(authority, &message, &restored).await?;
                if !self
                    .effects
                    .verify_signature(
                        &message,
                        &proof.signature,
                        &public_key_package,
                        restored.mode,
                    )
                    .await?
                {
                    return Err(AuraError::storage(
                        "persisted signing key does not match its public package",
                    ));
                }
            }
            if epoch == 0
                && restored.mode == SigningMode::SingleSigner
                && (restored.participants == vec![ParticipantIdentity::guardian(*authority)]
                    || restored.participants
                        == vec![ParticipantIdentity::device(self.effects.device_id())])
            {
                self.complete_bootstrap_genesis(authority, &restored)
                    .await?;
            }
            let restored = if epoch == 0 {
                self.migrate_proved_legacy_bootstrap(authority, restored)
                    .await?
            } else {
                restored
            };
            if !has_epoch {
                self.effects
                    .secure_store(
                        &epoch_location,
                        &epoch.to_le_bytes(),
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await?;
            }
            with_state_mut_validated(
                &self.shared.state,
                |state| {
                    state.contexts.insert(*authority, restored);
                },
                |state| state.validate(),
            )
            .await;
            return Ok(public_key_package);
        }
        // A partial prior bootstrap must not overwrite already-created keys.
        let initial_key =
            SecureStorageLocation::with_sub_key("signing_keys", format!("{authority}:0"), "1");
        let initial_config =
            SecureStorageLocation::with_sub_key("threshold_config", authority.to_string(), "0");
        let initial_public =
            SecureStorageLocation::with_sub_key("threshold_pubkey", authority.to_string(), "0");
        let initial_participant = ParticipantIdentity::device(self.effects.device_id());
        let initial_wrap = Self::participant_wrap_key_location(authority, 0, &initial_participant);
        let initial_share = Self::participant_share_location(authority, 0, &initial_participant);
        for location in [
            &initial_key,
            &initial_config,
            &initial_public,
            &initial_wrap,
            &initial_share,
        ] {
            if self.effects.secure_exists(location).await? {
                return Err(AuraError::storage(
                    "incomplete signing bootstrap requires recovery; refusing key replacement",
                ));
            }
        }
        let epoch = 0u64;
        let participant = ParticipantIdentity::device(self.effects.device_id());
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

        let pending = BootstrapGenesisRecord {
            version: 1,
            authority: *authority,
            device: self.effects.device_id(),
            epoch: 0,
            public_package_digest: aura_core::hash::hash(&key_result.public_key_package),
            state: BootstrapGenesisState::Pending,
        };
        self.effects
            .secure_store(
                &Self::bootstrap_genesis_location(authority),
                &serde_json::to_vec(&pending)?,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;

        // Store key package in secure storage
        // Location: signing_keys/<authority>/<epoch>/1
        let location = SecureStorageLocation::with_sub_key(
            "signing_keys",
            format!("{}:{}", authority, epoch),
            "1", // signer index 1
        );

        self.store_participant_key_package(
            authority,
            epoch,
            &participant,
            &location,
            &key_result.key_packages[0],
        )
        .await
        .map_err(|e| AuraError::internal(format!("Failed to store key package: {}", e)))?;

        // Store participant share for consensus/DKG helpers.
        let participant_location = Self::participant_share_location(authority, epoch, &participant);
        self.store_participant_key_package(
            authority,
            epoch,
            &participant,
            &participant_location,
            &key_result.key_packages[0],
        )
        .await
        .map_err(|e| AuraError::internal(format!("Failed to store participant share: {}", e)))?;

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
            bootstrap_migration_origin: None,
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

        // Key readiness alone cannot publish a usable bootstrap context.
        self.complete_bootstrap_genesis(authority, &state).await?;
        with_state_mut_validated(
            &self.shared.state,
            |state_map| {
                state_map.contexts.insert(*authority, state);
            },
            |state_map| state_map.validate(),
        )
        .await;

        let (_key_packages, public_key_package, _mode) = key_result.into_parts();
        Ok(public_key_package)
    }

    async fn sign(&self, context: SigningContext) -> Result<ThresholdSignature, AuraError> {
        let state = self
            .shared
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
            _ => Self::serialize_signing_context(&context, state.epoch)?,
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

        // Enforce theorem-pack runtime capability for threshold signing paths.
        self.runtime_capabilities
            .require_capabilities(&[CapabilityKey::new("byzantine_envelope")])
            .await
            .map_err(|source| {
                use aura_core::effects::AdmissionError;
                match source {
                    AdmissionError::InventoryUnavailable { .. }
                    | AdmissionError::Internal { .. } => AuraError::Internal {
                        message: "required threshold capability inventory failed".into(),
                        source: Some(Arc::new(source)),
                    },
                    AdmissionError::MissingCapability { .. }
                    | AdmissionError::MissingTheoremPack { .. }
                    | AdmissionError::MissingTheoremPackCapability { .. }
                    | AdmissionError::MissingRuntimeContracts => AuraError::PermissionDenied {
                        message: "required threshold capability admission denied".into(),
                        source: Some(Arc::new(source)),
                    },
                }
            })?;

        // A raw context cannot authorize a distributed quorum round. Required
        // local material is validated before reporting the missing owner.
        Err(self
            .unowned_threshold_route_error(&context.authority, state.epoch)
            .await)
    }

    async fn threshold_config(&self, authority: &AuthorityId) -> Option<ThresholdConfig> {
        self.shared
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.config.clone())
    }

    async fn threshold_state(&self, authority: &AuthorityId) -> Option<ThresholdState> {
        self.shared
            .state
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
        self.shared
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.my_signer_index.is_some())
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        self.shared
            .state
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
        // Lock order: generation custody precedes signing transitions and tree decisions.
        let _generation = self.effects.enrollment_retirement_generation_guard().await;
        let _transition = self.shared.transitions.lock().await;
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
            .shared
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|s| s.epoch)
            .unwrap_or(0);

        let new_epoch = current_epoch
            .checked_add(1)
            .ok_or_else(|| AuraError::invalid("signing epoch overflow"))?;
        let pending_config = SecureStorageLocation::with_sub_key(
            "threshold_config",
            authority.to_string(),
            new_epoch.to_string(),
        );
        let pending_public = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            authority.to_string(),
            new_epoch.to_string(),
        );
        for location in [&pending_config, &pending_public] {
            if self.effects.secure_exists(location).await? {
                return Err(AuraError::storage(
                    "pending signing epoch already exists; refusing replacement",
                ));
            }
        }

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
            let (key_packages, public_key_package, _mode) = result.into_parts();

            aura_core::effects::crypto::FrostKeyGenResult {
                key_packages,
                public_key_package,
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
            let location = Self::participant_share_location(authority, new_epoch, participant);

            self.store_participant_key_package(
                authority,
                new_epoch,
                participant,
                &location,
                key_package,
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
            bootstrap_migration_origin: None,
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

        let (key_packages, public_key_package) = key_result.into_parts();
        Ok((new_epoch, key_packages, public_key_package))
    }

    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), AuraError> {
        self.activate_retained_generation(authority, new_epoch, None, None, None)
            .await
    }

    async fn rollback_key_rotation(
        &self,
        authority: &AuthorityId,
        failed_epoch: u64,
    ) -> Result<(), AuraError> {
        // Lock order: generation custody precedes signing transitions and tree decisions.
        let _generation = self.effects.enrollment_retirement_generation_guard().await;
        let _transition = self.shared.transitions.lock().await;
        let current = self
            .shared
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .map(|context| context.epoch);
        if current.is_some_and(|active| failed_epoch <= active) {
            return Err(AuraError::invalid(
                "cannot roll back an active or historical signing epoch",
            ));
        }
        let epoch_location = SecureStorageLocation::new("epoch_state", authority.to_string());
        if self.effects.secure_exists(&epoch_location).await? {
            let bytes = self
                .effects
                .secure_retrieve(&epoch_location, &[SecureStorageCapability::Read])
                .await?;
            let active =
                u64::from_le_bytes(bytes.try_into().map_err(|_| {
                    AuraError::storage("persisted signing epoch has invalid length")
                })?);
            if failed_epoch <= active {
                return Err(AuraError::invalid(
                    "cannot roll back a persisted active or historical signing epoch",
                ));
            }
        }
        if self
            .effects
            .secure_exists(
                &crate::runtime::effects::enrollment_generation_profile_location(
                    authority,
                    failed_epoch,
                ),
            )
            .await?
        {
            return Err(AuraError::invalid(
                "generic rollback cannot retire an enrollment-owned generation",
            ));
        }
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
                    format!("{}:{}", authority, failed_epoch),
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for ThresholdSigningService {
    fn name(&self) -> &'static str {
        "threshold_signing"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["ceremony_tracker"]
    }

    async fn start(&self, _context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        *self.shared.lifecycle.write().await = ServiceHealth::Healthy;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        *self.shared.lifecycle.write().await = ServiceHealth::Stopping;
        if let Err(source) = self.shared.quorum.drain_all().await {
            *self.shared.lifecycle.write().await = ServiceHealth::Unhealthy {
                reason: source.to_string(),
            };
            return Err(ServiceError::shutdown_failed(
                "threshold_signing",
                "original enrollment quorum owners could not acknowledge teardown",
            )
            .with_cause(source));
        }
        // Clear signing contexts + leases on shutdown
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.contexts.clear();
                state.leases.clear();
            },
            |state| state.validate(),
        )
        .await;
        *self.shared.lifecycle.write().await = ServiceHealth::Stopped;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.shared.lifecycle.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::StorageConfig;
    use crate::core::AgentConfig;
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

    fn isolated_test_config() -> (tempfile::TempDir, AgentConfig) {
        let temp = tempfile::tempdir().unwrap();
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                ..Default::default()
            },
            ..Default::default()
        };
        (temp, config)
    }

    fn genesis_error_kind(error: &AuraError) -> Option<&BootstrapGenesisError> {
        std::error::Error::source(error)?.downcast_ref()
    }

    #[tokio::test]
    async fn material_mutations_wait_for_generation_before_transition_lock() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let participants = [ParticipantIdentity::device(effects.device_id())];
        let owner = effects.enrollment_retirement_generation_guard().await;
        {
            let bootstrap = service.bootstrap_authority(&authority);
            tokio::pin!(bootstrap);
            assert!(futures::poll!(&mut bootstrap).is_pending());
            assert!(service.shared.transitions.try_lock().is_ok());
            let rotation = service.rotate_keys(&authority, 1, 1, &participants);
            tokio::pin!(rotation);
            assert!(futures::poll!(&mut rotation).is_pending());
            assert!(service.shared.transitions.try_lock().is_ok());
            let rollback = service.rollback_key_rotation(&authority, 1);
            tokio::pin!(rollback);
            assert!(futures::poll!(&mut rollback).is_pending());
            assert!(service.shared.transitions.try_lock().is_ok());
        }
        drop(owner);
        service
            .bootstrap_authority(&authority)
            .await
            .expect("actual bootstrap progresses after generation release");
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
    fn test_serialize_signing_context() {
        let context = SigningContext::message(
            test_authority(),
            "test.threshold".to_string(),
            vec![1, 2, 3],
        );
        let result = ThresholdSigningService::serialize_signing_context(&context, 1);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bootstrap_authority_seeds_initial_device_leaf_in_tree_ops() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();

        service.bootstrap_authority(&authority).await.unwrap();

        let metadata = effects
            .require_threshold_config_metadata(&authority, 0)
            .await
            .expect("actual required bootstrap policy");
        assert!(metadata.contains_participant(&ParticipantIdentity::device(effects.device_id())));
        assert!(!metadata.contains_participant(&ParticipantIdentity::guardian(authority)));
        let restarted = ThresholdSigningService::new(effects.clone());
        assert_eq!(
            restarted
                .bootstrap_authority(&authority)
                .await
                .expect("actual physical bootstrap restart"),
            service
                .public_key_package(&authority)
                .await
                .expect("original public package")
        );
        assert_eq!(
            restarted
                .current_local_key_agreement_secret(&authority)
                .await
                .expect("restored contextual physical share"),
            service
                .current_local_key_agreement_secret(&authority)
                .await
                .expect("original contextual physical share")
        );
        let ops = effects.export_tree_ops().await.unwrap();
        assert!(
            !ops.is_empty(),
            "bootstrapped authority should export a non-empty baseline tree oplog"
        );

        let state = effects.get_current_state().await.unwrap();
        let current_device = effects.device_id();
        assert!(
            state
                .leaves
                .values()
                .any(|leaf| leaf.role == LeafRole::Device && leaf.device_id == current_device),
            "bootstrapped authority should persist the current device as a real tree leaf"
        );
    }

    #[tokio::test]
    async fn enrollment_setup_export_binds_actual_device_and_retains_exact_code() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        assert!(matches!(
            service.export_device_enrollment_setup_request(authority).await,
            Err(EnrollmentSetupExportError::MissingSigningContext(id)) if id == authority
        ));
        let package = service.bootstrap_authority(&authority).await.unwrap();
        let code = service
            .export_device_enrollment_setup_request(authority)
            .await
            .unwrap();
        let request = DeviceEnrollmentSetupRequest::decode(&code).unwrap();
        assert_eq!(request.statement.authority, authority);
        assert_eq!(request.statement.device, effects.device_id());
        assert_eq!(request.statement.public_key_package, package);
        let now = effects.physical_time().await.unwrap().ts_ms;
        request
            .clone()
            .verify_possession(effects.as_ref(), now)
            .await
            .unwrap();
        let location = SecureStorageLocation::with_sub_key(
            "device_enrollment_setup",
            format!("{authority}:{}", hex::encode(request.statement.nonce)),
            "request",
        );
        let retained = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .unwrap();
        assert_eq!(retained, code.as_bytes());

        // A fresh service sharing persisted effects recovers the same signer.
        let restarted = ThresholdSigningService::new(effects.clone());
        let recovered = restarted.bootstrap_authority(&authority).await.unwrap();
        assert_eq!(recovered, package);
        let restored =
            DeviceEnrollmentSetupRequest::decode(std::str::from_utf8(&retained).unwrap()).unwrap();
        restored
            .verify_possession(effects.as_ref(), now)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bootstrap_genesis_failure_resumes_retained_keys_without_publishing_early() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let index = aura_journal::commitment_tree::storage::TREE_OPS_INDEX_KEY;
        effects
            .store(index, b"corrupt-index".to_vec())
            .await
            .unwrap();
        assert!(service.bootstrap_authority(&authority).await.is_err());
        assert!(service.threshold_config(&authority).await.is_none());
        let public_location =
            SecureStorageLocation::with_sub_key("threshold_pubkey", authority.to_string(), "0");
        let package = effects
            .secure_retrieve(&public_location, &[SecureStorageCapability::Read])
            .await
            .unwrap();
        let location = ThresholdSigningService::bootstrap_genesis_location(&authority);
        let pending: BootstrapGenesisRecord = serde_json::from_slice(
            &effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(pending.state, BootstrapGenesisState::Pending));
        effects.remove(index).await.unwrap();
        assert_eq!(
            service.bootstrap_authority(&authority).await.unwrap(),
            package
        );
        let restarted = ThresholdSigningService::new(effects.clone());
        assert_eq!(
            restarted.bootstrap_authority(&authority).await.unwrap(),
            package
        );
        assert_eq!(effects.export_tree_ops().await.unwrap().len(), 1);
        let complete: BootstrapGenesisRecord = serde_json::from_slice(
            &effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            complete.state,
            BootstrapGenesisState::Complete { .. }
        ));
    }

    #[tokio::test]
    async fn bootstrap_genesis_requires_durable_creation_and_exact_completion_digest() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        service.bootstrap_authority(&authority).await.unwrap();
        let location = ThresholdSigningService::bootstrap_genesis_location(&authority);
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        let original = effects.secure_retrieve(&location, &caps).await.unwrap();
        let mut record: BootstrapGenesisRecord = serde_json::from_slice(&original).unwrap();
        record.state = BootstrapGenesisState::Complete {
            creation_op_digest: [99; 32],
        };
        effects
            .secure_store(&location, &serde_json::to_vec(&record).unwrap(), &caps)
            .await
            .unwrap();
        let restarted = ThresholdSigningService::new(effects.clone());
        let error = restarted.bootstrap_authority(&authority).await.unwrap_err();
        assert!(matches!(
            genesis_error_kind(&error),
            Some(BootstrapGenesisError::CompletionMismatch)
        ));
        assert!(restarted.threshold_config(&authority).await.is_none());
        effects
            .secure_store(&location, &original, &caps)
            .await
            .unwrap();
        effects
            .remove(aura_journal::commitment_tree::storage::TREE_OPS_INDEX_KEY)
            .await
            .unwrap();
        let error = restarted.bootstrap_authority(&authority).await.unwrap_err();
        assert!(matches!(
            genesis_error_kind(&error),
            Some(BootstrapGenesisError::MissingDurableCreation)
        ));
        assert!(restarted.threshold_config(&authority).await.is_none());
    }

    #[tokio::test]
    async fn legacy_bootstrap_migrates_only_an_authenticated_existing_genesis() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let package = service.bootstrap_authority(&authority).await.unwrap();
        let location = ThresholdSigningService::bootstrap_genesis_location(&authority);
        effects
            .secure_delete(&location, &[SecureStorageCapability::Delete])
            .await
            .unwrap();
        let migrated = ThresholdSigningService::new(effects.clone());
        assert_eq!(
            migrated.bootstrap_authority(&authority).await.unwrap(),
            package
        );
        assert_eq!(effects.export_tree_ops().await.unwrap().len(), 1);
        effects
            .secure_delete(&location, &[SecureStorageCapability::Delete])
            .await
            .unwrap();
        effects.replace_tree_ops(&[]).await.unwrap();
        let missing = ThresholdSigningService::new(effects.clone());
        let error = missing.bootstrap_authority(&authority).await.unwrap_err();
        assert!(matches!(
            genesis_error_kind(&error),
            Some(BootstrapGenesisError::MissingCreation)
        ));
        assert!(missing.threshold_config(&authority).await.is_none());
        let public_location =
            SecureStorageLocation::with_sub_key("threshold_pubkey", authority.to_string(), "0");
        assert_eq!(
            effects
                .secure_retrieve(&public_location, &[SecureStorageCapability::Read])
                .await
                .unwrap(),
            package
        );
        assert!(effects.export_tree_ops().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn signing_bootstrap_is_idempotent_under_concurrent_calls() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let (left, right) = tokio::join!(
            service.bootstrap_authority(&authority),
            service.bootstrap_authority(&authority)
        );
        assert_eq!(left.unwrap(), right.unwrap());
        assert_eq!(effects.export_tree_ops().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn signing_bootstrap_corruption_preserves_existing_keys() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let package = service.bootstrap_authority(&authority).await.unwrap();
        let epoch_location = SecureStorageLocation::new("epoch_state", authority.to_string());
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        effects
            .secure_store(&epoch_location, &[1, 2, 3], &caps)
            .await
            .unwrap();
        let restarted = ThresholdSigningService::new(effects.clone());
        assert!(matches!(
            restarted.bootstrap_authority(&authority).await,
            Err(AuraError::Storage { .. })
        ));
        let public_location =
            SecureStorageLocation::with_sub_key("threshold_pubkey", authority.to_string(), "0");
        assert_eq!(
            effects
                .secure_retrieve(&public_location, &caps)
                .await
                .unwrap(),
            package
        );
        effects
            .secure_delete(&epoch_location, &[SecureStorageCapability::Delete])
            .await
            .unwrap();
        assert!(matches!(
            restarted.bootstrap_authority(&authority).await,
            Err(AuraError::Storage { .. })
        ));
        assert_eq!(
            effects
                .secure_retrieve(&public_location, &caps)
                .await
                .unwrap(),
            package
        );
    }

    #[tokio::test]
    async fn signing_recovery_never_creates_missing_wrapping_key() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        service.bootstrap_authority(&authority).await.unwrap();
        let location = ThresholdSigningService::participant_wrap_key_location(
            &authority,
            0,
            &ParticipantIdentity::device(effects.device_id()),
        );
        // Immutable wrapping keys cannot be deleted through the API; inject the
        // backing-provider loss instead.
        assert!(effects
            .fault_remove_secure_record_for_test(&location)
            .await
            .unwrap());
        let restarted = ThresholdSigningService::new(effects.clone());
        assert!(restarted.bootstrap_authority(&authority).await.is_err());
        assert!(!effects.secure_exists(&location).await.unwrap());
    }

    // Raw enrollment-share adoption is covered through actual signed setup,
    // pinned manifest, committed receipt and exact generation activation in
    // handlers::invitation::enrollment_vm_admission::committed_receipt_tests.

    #[tokio::test]
    async fn commit_key_rotation_uses_threshold_config_metadata_written_by_effects() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let participants = vec![ParticipantIdentity::Device(effects.device_id())];

        let (new_epoch, _, _) = effects
            .rotate_keys(&authority, 1, 1, &participants)
            .await
            .expect("effect-layer rotate_keys should write shared threshold config metadata");

        service
            .commit_key_rotation(&authority, new_epoch)
            .await
            .expect("commit should accept the shared threshold_config record");

        let state = service
            .threshold_state(&authority)
            .await
            .expect("committed threshold state should be available");
        assert_eq!(state.epoch, new_epoch);
        assert_eq!(state.threshold, 1);
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn participant_key_packages_are_wrapped_before_secure_storage() {
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effects.clone());
        let authority = test_authority();
        let participants = vec![ParticipantIdentity::Device(effects.device_id())];

        let (new_epoch, key_packages, _) = service
            .rotate_keys(&authority, 1, 1, &participants)
            .await
            .expect("rotate keys");
        let raw_key_package = &key_packages[0];
        let participant = &participants[0];
        let location =
            ThresholdSigningService::participant_share_location(&authority, new_epoch, participant);

        let stored = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .expect("stored wrapped participant package");
        assert!(
            !stored
                .windows(raw_key_package.len())
                .any(|window| window == raw_key_package),
            "participant key package was stored directly"
        );

        let envelope: ParticipantKeyPackageEnvelope =
            serde_json::from_slice(&stored).expect("stored package envelope");
        assert_eq!(envelope.version, PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION);
        assert_eq!(envelope.recipient, *participant);
        assert_ne!(envelope.ciphertext, *raw_key_package);

        let decrypted = service
            .decrypt_participant_key_package(&authority, new_epoch, participant, &stored)
            .await
            .expect("decrypt participant package");
        assert_eq!(decrypted, *raw_key_package);
        let wrong_authority = AuthorityId::new_from_entropy([229; 32]);
        let foreign = service
            .decrypt_participant_key_package(&wrong_authority, new_epoch, participant, &stored)
            .await
            .expect_err("canonical envelope must reject another authority");
        assert!(matches!(foreign, AuraError::PermissionDenied { .. }));

        let raw = service
            .decrypt_participant_key_package(&authority, new_epoch, participant, raw_key_package)
            .await
            .expect_err("raw package bytes are never an envelope compatibility path");
        assert!(matches!(raw, AuraError::Serialization { .. }));
        assert!(
            std::error::Error::source(&raw)
                .and_then(|source| source.downcast_ref::<serde_json::Error>())
                .is_some(),
            "actual envelope codec cause must survive"
        );
    }
}

#[cfg(all(test, unix))]
mod proved_legacy_bootstrap_tests {
    use super::*;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use aura_app::runtime_bridge::RuntimeBridge;

    async fn historical_original() -> (std::sync::Arc<crate::AuraAgent>, Vec<u8>) {
        let authority = AuthorityId::new_from_entropy([232; 32]);
        let config = crate::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy([233; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::tempdir().expect("historical profile").keep(),
                ..Default::default()
            },
            ..Default::default()
        };
        let context = aura_core::context::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy([234; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let agent = std::sync::Arc::new(
            crate::AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .expect("actual runtime"),
        );
        AgentRuntimeBridge::new(agent.clone())
            .bootstrap_signing_keys()
            .await
            .expect("original bootstrap");
        let effects = agent.runtime().effects();
        let encoder = ThresholdSigningService::new(effects.clone());
        let physical = ParticipantIdentity::device(effects.device_id());
        let physical_location =
            ThresholdSigningService::participant_share_location(&authority, 0, &physical);
        let bytes = effects
            .secure_retrieve(&physical_location, &[SecureStorageCapability::Read])
            .await
            .expect("original physical secret envelope");
        let secret = zeroize::Zeroizing::new(
            encoder
                .decrypt_participant_key_package(&authority, 0, &physical, &bytes)
                .await
                .expect("actual original decryption"),
        );
        let guardian = ParticipantIdentity::guardian(authority);
        let legacy = encoder
            .encrypt_participant_key_package(&authority, 0, &guardian, &secret)
            .await
            .expect("actual historical encryption/AAD producer");
        let solo =
            SecureStorageLocation::with_sub_key("signing_keys", format!("{authority}:0"), "1");
        effects
            .secure_store(&solo, &legacy, &[SecureStorageCapability::Write])
            .await
            .expect("historical mutable key row");
        let legacy_location =
            ThresholdSigningService::participant_share_location(&authority, 0, &guardian);
        effects
            .secure_store(&legacy_location, &legacy, &[SecureStorageCapability::Write])
            .await
            .expect("historical mutable participant row");
        // Physical backing loss reproduces an old layout without introducing a
        // generic delete exemption for lifetime-protected originals.
        assert!(effects
            .fault_remove_secure_record_for_test(&physical_location)
            .await
            .expect("selected old-layout backing removal"));
        let config_location =
            SecureStorageLocation::with_sub_key("threshold_config", authority.to_string(), "0");
        let mut metadata: ThresholdConfigMetadata = serde_json::from_slice(
            &effects
                .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
                .await
                .expect("original config"),
        )
        .expect("actual original metadata");
        metadata.participants = vec![guardian];
        effects
            .secure_store(
                &config_location,
                &serde_json::to_vec(&metadata).expect("historical metadata encoding"),
                &[SecureStorageCapability::Write],
            )
            .await
            .expect("historical mutable policy");
        let public = effects
            .secure_retrieve(
                &SecureStorageLocation::with_sub_key(
                    "threshold_pubkey",
                    authority.to_string(),
                    "0",
                ),
                &[SecureStorageCapability::Read],
            )
            .await
            .expect("original authenticated public package");
        (agent, public)
    }

    #[tokio::test]
    async fn proved_historical_encoding_migrates_without_regeneration_and_restarts() {
        let (agent, public) = historical_original().await;
        let effects = agent.runtime().effects();
        let authority = agent.authority_id();
        let restored = ThresholdSigningService::new(effects.clone());
        assert_eq!(
            restored
                .bootstrap_authority(&authority)
                .await
                .expect("proved original migration"),
            public
        );
        let config_location =
            SecureStorageLocation::with_sub_key("threshold_config", authority.to_string(), "0");
        let config_bytes = effects
            .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
            .await
            .expect("converted canonical metadata");
        let metadata: ThresholdConfigMetadata =
            serde_json::from_slice(&config_bytes).expect("converted metadata");
        assert_eq!(
            metadata.participants,
            vec![ParticipantIdentity::device(effects.device_id())]
        );
        let origin = metadata
            .bootstrap_migration_origin
            .expect("required original provenance");
        validate_bootstrap_migration_origin(effects.as_ref(), &authority, 0, origin)
            .await
            .expect("protected original proof");
        let restarted = ThresholdSigningService::new(effects.clone());
        assert_eq!(
            restarted
                .bootstrap_authority(&authority)
                .await
                .expect("same original service restart"),
            public
        );
        assert_eq!(
            effects
                .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
                .await
                .expect("stable original metadata"),
            config_bytes
        );
        let proof = restarted
            .sign(SigningContext::message(
                authority,
                "aura.migration.original-proof".into(),
                vec![1],
            ))
            .await
            .expect("actual migrated signature");
        assert_eq!(proof.public_key_package, public);
        let decision = SecureStorageLocation::new(
            "bootstrap_physical_participant_migration_v1",
            authority.to_string(),
        );
        assert!(effects
            .fault_remove_secure_record_for_test(&decision)
            .await
            .expect("original proof physical loss"));
        let failed = ThresholdSigningService::new(effects.clone())
            .bootstrap_authority(&authority)
            .await
            .expect_err("converted metadata cannot be relabeled fresh after protected origin loss");
        assert!(matches!(failed, AuraError::Storage { .. }));
        assert!(std::error::Error::source(&failed).is_some());
        let native_failure = effects
            .sign(SigningContext::message(
                authority,
                "aura.migration.original-proof".into(),
                vec![3],
            ))
            .await
            .expect_err("required effect policy must retain original origin too");
        assert!(matches!(native_failure, AuraError::Storage { .. }));
        assert!(
            restarted
                .sign(SigningContext::message(
                    authority,
                    "aura.migration.original-proof".into(),
                    vec![2]
                ))
                .await
                .is_err(),
            "live cached context cannot bypass protected origin loss"
        );
        assert_eq!(
            effects
                .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
                .await
                .expect("no metadata repair after proof loss"),
            config_bytes
        );
    }

    #[tokio::test]
    async fn missing_durable_creation_proof_cannot_allocate_migration_decision() {
        let (agent, _) = historical_original().await;
        let effects = agent.runtime().effects();
        let authority = agent.authority_id();
        assert!(effects
            .remove(aura_journal::commitment_tree::storage::TREE_OPS_INDEX_KEY)
            .await
            .expect("actual durable creation index loss"));
        let failure = ThresholdSigningService::new(effects.clone())
            .bootstrap_authority(&authority)
            .await
            .expect_err("cached history cannot replace original durable commit evidence");
        assert!(matches!(failure, AuraError::Storage { .. }));
        assert!(!effects
            .secure_exists(&SecureStorageLocation::new(
                "bootstrap_physical_participant_migration_v1",
                authority.to_string()
            ))
            .await
            .expect("required decision absence read"));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod original_stop_window_tests {
    use super::super::traits::ServiceErrorKind;
    use super::*;
    use aura_testkit::time::ManualPhysicalClock;
    use std::time::Duration;

    fn timeout_cause(error: &ServiceError) -> Option<&aura_core::TimeoutBudgetError> {
        let mut current: &(dyn std::error::Error + 'static) = error;
        loop {
            if let Some(cause) = current.downcast_ref::<aura_core::TimeoutBudgetError>() {
                return Some(cause);
            }
            current = current.source()?;
        }
    }

    #[tokio::test]
    async fn required_service_health_retains_original_deadline_and_sticky_clock_rollback(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let authority = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"work10-original-health-window-actual-authority",
        ));
        let context = crate::runtime::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"work10-original-health-window-actual-context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let config = crate::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy(aura_core::hash::hash(
                b"work10-original-health-window-actual-device",
            )),
            storage: crate::core::config::StorageConfig {
                base_path: profile.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let clock = Arc::new(ManualPhysicalClock::new(100));
        let runtime = crate::runtime::builder::EffectSystemBuilder::testing()
            .with_config(config)
            .with_authority(authority)
            .with_physical_time_provider(clock.clone())
            .build(&context)
            .await?;
        runtime
            .authorities()
            .ensure_authority(authority, 100)
            .await?;
        runtime
            .authorities()
            .set_status(authority, super::super::AuthorityStatus::Active, 100)
            .await?;
        let original = runtime.close_for_shutdown_test().await?;
        let service = runtime.threshold_signing();

        // Hold the actual state while stop has published Stopping. Queue a
        // writer behind stop's final lifecycle write, ahead of its health read.
        let held_state = service.shared.state.write().await;
        clock.set_time(29_000);
        let stop = runtime.stop_runtime_service(&service, &original);
        tokio::pin!(stop);
        assert!(futures::poll!(stop.as_mut()).is_pending());
        let lifecycle_reader = service.shared.lifecycle.read().await;
        assert_eq!(*lifecycle_reader, ServiceHealth::Stopping);
        drop(held_state);
        assert!(futures::poll!(stop.as_mut()).is_pending());
        let health_writer = service.shared.lifecycle.write();
        tokio::pin!(health_writer);
        assert!(futures::poll!(health_writer.as_mut()).is_pending());
        drop(lifecycle_reader);
        assert!(futures::poll!(stop.as_mut()).is_pending());
        let held_health = match futures::poll!(health_writer.as_mut()) {
            std::task::Poll::Ready(guard) => guard,
            std::task::Poll::Pending => panic!("actual stop must precede queued health writer"),
        };
        assert_eq!(*held_health, ServiceHealth::Stopped);

        // Service stop has finished; only its required actual health read is
        // blocked. It cannot allocate a new window after successful stop.
        clock.set_time(30_100);
        let error = stop
            .await
            .expect_err("health ACK cannot renew original window");
        assert_eq!(error.kind, ServiceErrorKind::Timeout);
        assert!(matches!(
            timeout_cause(&error),
            Some(aura_core::TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 30_100,
                observed_at_ms: 30_100,
            })
        ));
        assert_eq!(
            runtime.runtime_activity_state(),
            crate::runtime::system::RuntimeActivityState::Stopping
        );
        assert_eq!(
            runtime
                .authorities()
                .get_authority(authority)
                .await?
                .ok_or("actual authority")?
                .status,
            super::super::AuthorityStatus::Active
        );
        drop(held_health);

        // The same original observation owner keeps rollback sticky even if
        // physical time is later restored. No replacement cap is issued.
        for now in [50, 30_100] {
            clock.set_time(now);
            let rollback = runtime
                .stop_runtime_service(&service, &original)
                .await
                .expect_err("original clock rollback cannot be repaired by retry");
            assert_eq!(rollback.kind, ServiceErrorKind::Internal);
            assert!(matches!(
                timeout_cause(&rollback),
                Some(aura_core::TimeoutBudgetError::ClockRollback {
                    previous_observed_at_ms: 30_100,
                    observed_at_ms: 50,
                })
            ));
            assert_eq!(service.health().await, ServiceHealth::Stopped);
        }
        runtime
            .tasks()
            .shutdown_with_timeout(Duration::from_secs(5))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn required_service_stop_retains_original_shutdown_deadline_under_actual_state_contention(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let authority = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"work10-original-stop-window-actual-authority",
        ));
        let context = crate::runtime::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"work10-original-stop-window-actual-context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let config = crate::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy(aura_core::hash::hash(
                b"work10-original-stop-window-actual-device",
            )),
            storage: crate::core::config::StorageConfig {
                base_path: profile.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let clock = Arc::new(ManualPhysicalClock::new(100));
        let runtime = crate::runtime::builder::EffectSystemBuilder::testing()
            .with_config(config)
            .with_authority(authority)
            .with_physical_time_provider(clock.clone())
            .build(&context)
            .await?;
        runtime
            .authorities()
            .ensure_authority(authority, 100)
            .await?;
        runtime
            .authorities()
            .set_status(authority, super::super::AuthorityStatus::Active, 100)
            .await?;
        let original = runtime.close_for_shutdown_test().await?;
        let service = runtime.threshold_signing();
        clock
            .fail_next_observation(aura_core::effects::TimeError::ServiceUnavailable)
            .await;
        let Err(provider_error) = runtime.stop_runtime_service(&service, &original).await else {
            panic!("required provider fault cannot count as successful stop")
        };
        assert_eq!(provider_error.kind, ServiceErrorKind::Internal);
        let mut cause: &(dyn std::error::Error + 'static) = &provider_error;
        let mut provider_retained = false;
        loop {
            if matches!(
                cause.downcast_ref::<aura_core::effects::TimeError>(),
                Some(aura_core::effects::TimeError::ServiceUnavailable)
            ) {
                provider_retained = true;
            }
            match cause.source() {
                Some(next) => cause = next,
                None => break,
            }
        }
        assert!(
            provider_retained,
            "original configured provider failure remains typed"
        );
        let held_state = service.shared.state.write().await;
        clock.set_time(29_000);
        let stop = runtime.stop_services(&original);
        tokio::pin!(stop);
        assert!(futures::poll!(stop.as_mut()).is_pending());
        // A renewed five-second child would still be live at this observation.
        clock.set_time(30_100);
        let Err(error) = stop.await else {
            panic!("original deadline cannot be renewed while state is held")
        };
        assert_eq!(error.kind, ServiceErrorKind::Timeout);
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        let mut actual_deadline = None;
        loop {
            if let Some(aura_core::TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms,
                observed_at_ms,
            }) = cause.downcast_ref::<aura_core::TimeoutBudgetError>()
            {
                actual_deadline = Some((*deadline_at_ms, *observed_at_ms));
            }
            match cause.source() {
                Some(next) => cause = next,
                None => break,
            }
        }
        assert_eq!(actual_deadline, Some((30_100, 30_100)));
        assert_eq!(
            runtime.runtime_activity_state(),
            crate::runtime::system::RuntimeActivityState::Stopping
        );
        let authority_state = runtime
            .authorities()
            .get_authority(authority)
            .await?
            .ok_or_else(|| std::io::Error::other("actual authority registry missing"))?;
        assert_eq!(
            authority_state.status,
            super::super::AuthorityStatus::Active,
            "failed stop cannot publish authority termination before service ACK"
        );
        drop(held_state);
        runtime
            .tasks()
            .shutdown_with_timeout(Duration::from_secs(5))
            .await?;
        Ok(())
    }
    #[tokio::test]
    async fn required_prior_task_failure_withholds_whole_shutdown_authority_termination(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let authority = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"work10-prior-task-failed-shutdown-actual-authority",
        ));
        let context = crate::runtime::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"work10-prior-task-failed-shutdown-actual-context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let config = crate::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy(aura_core::hash::hash(
                b"work10-prior-task-failed-shutdown-actual-device",
            )),
            storage: crate::core::config::StorageConfig {
                base_path: profile.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let clock = Arc::new(ManualPhysicalClock::new(100));
        let runtime = crate::runtime::builder::EffectSystemBuilder::testing()
            .with_config(config)
            .with_authority(authority)
            .with_physical_time_provider(clock.clone())
            .build(&context)
            .await?;
        runtime
            .authorities()
            .ensure_authority(authority, 100)
            .await?;
        runtime
            .authorities()
            .set_status(authority, super::super::AuthorityStatus::Active, 100)
            .await?;
        let observer = runtime.authorities().observe_status_for_test();
        let gate = runtime.activity_gate();
        let task = runtime
            .tasks()
            .spawn_try_named("required_prior_shutdown_task_failure", async {
                Err(aura_core::AuraError::Internal {
                    message: "actual failed runtime task".into(),
                    source: None,
                })
            });
        assert!(runtime
            .tasks()
            .wait_for_idle(Duration::from_secs(5))
            .await
            .is_err());
        let Err(error) = runtime.shutdown_typed(&context).await else {
            panic!("failed task tree cannot acknowledge whole shutdown")
        };
        assert!(matches!(
            error,
            crate::runtime::system::RuntimeShutdownError::TaskTree(_)
        ));
        assert_eq!(
            gate.state(),
            crate::runtime::system::RuntimeActivityState::Stopping
        );
        assert_eq!(observer.status(authority).await, Some(super::super::AuthorityStatus::Active),
            "successful later service stops cannot erase prior task-tree failure or publish termination");
        drop(task);
        Ok(())
    }
}
// Local signing material retains the original protected tree allocation.

pub(super) struct ValidatedLocalEnrollmentSigningMaterial<'tree, 'custody, 'owner, 'runtime> {
    effects: Arc<AuraEffectSystem>,
    custody:
        &'tree crate::runtime::effects::EnrollmentTranscriptTreeOwner<'custody, 'owner, 'runtime>,
    device: DeviceId,
    index: u16,
    threshold: u16,
    ordered_devices: Vec<DeviceId>,
    public_package: Vec<u8>,
    verifying_key: Vec<u8>,
    local_share: zeroize::Zeroizing<Vec<u8>>,
}

impl ThresholdSigningService {
    /// The only local material producer. Inputs retain explicit user consent,
    /// exact runtime origin and actual original generation custody. No remote
    /// key/config/message field selects the trusted material.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentTranscriptTreeOwner",
        family = "runtime_helper"
    )]
    pub(super) async fn admit_local_enrollment_transcript_material<
        'tree,
        'custody: 'tree,
        'owner: 'custody,
        'runtime: 'owner,
    >(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        custody: &'tree crate::runtime::effects::EnrollmentTranscriptTreeOwner<
            'custody,
            'owner,
            'runtime,
        >,
    ) -> Result<ValidatedLocalEnrollmentSigningMaterial<'tree, 'custody, 'owner, 'runtime>, AuraError>
    {
        let effects = approval.effects();
        custody.require_manifest(effects.as_ref(), approval.manifest())?;
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(AuraError::permission_denied(
                "approved enrollment signing owner differs from selected local material owner",
            ));
        }
        let manifest = approval.manifest();
        let held = self.shared.state.read().await;
        let active = held.contexts.get(&manifest.subject).ok_or_else(|| {
            AuraError::not_found(
                "approved enrollment subject has no retained active local signing context",
            )
        })?;
        if active.epoch != manifest.final_epoch
            || active.mode != SigningMode::Threshold
            || active.agreement_mode != AgreementMode::ConsensusFinalized
            || active.config.threshold < 2
            || active.my_signer_index.is_none()
        {
            return Err(AuraError::permission_denied(
                "approved enrollment requires the original active threshold participant",
            ));
        }
        let active_epoch = effects
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", manifest.subject.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let original_epoch: [u8; 8] =
            active_epoch
                .as_slice()
                .try_into()
                .map_err(|source| AuraError::Serialization {
                    message: "decode original approved participant epoch".into(),
                    source: Some(Arc::new(source)),
                })?;
        if u64::from_le_bytes(original_epoch) != active.epoch {
            return Err(AuraError::permission_denied(
                "protected active participant epoch differs from original context",
            ));
        }
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            manifest.subject.to_string(),
            active.epoch.to_string(),
        );
        let protected_config = effects
            .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
            .await?;
        if protected_config.len() > 131_072 {
            return Err(AuraError::invalid(
                "protected approved signing configuration exceeds bounds",
            ));
        }
        let policy = self
            .read_original_effective_signing_policy(
                &manifest.subject,
                active.epoch,
                &protected_config,
            )
            .await?;
        let roster = active.participants.clone();
        if policy.mode != SigningMode::Threshold
            || policy.agreement_mode != active.agreement_mode
            || policy.threshold_k != active.config.threshold
            || policy.total_n != active.config.total_participants
            || policy.participants != roster
            || roster.len() != usize::from(policy.total_n)
            || roster.len() > 1024
            || roster
                .iter()
                .enumerate()
                .any(|(index, participant)| roster[..index].contains(participant))
        {
            return Err(AuraError::permission_denied(
                "protected signing policy differs from original approved active context",
            ));
        }
        let public_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            manifest.subject.to_string(),
            active.epoch.to_string(),
        );
        let public_package = effects
            .secure_retrieve(&public_location, &[SecureStorageCapability::Read])
            .await?;
        if public_package != active.public_key_package || public_package.len() > 131_072 {
            return Err(AuraError::permission_denied(
                "protected public package differs from original approved signing context",
            ));
        }
        let native_public = frost_ed25519::keys::PublicKeyPackage::deserialize(&public_package)
            .map_err(|source| AuraError::Crypto {
                message: "decode original approved native public package".into(),
                source: Some(Arc::new(source)),
            })?;
        let ordered_devices = roster
            .iter()
            .map(|participant| match participant {
                ParticipantIdentity::Device(device) => Ok(*device),
                _ => Err(AuraError::permission_denied(
                    "enrollment transcript roster is not physical-device owned",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let position = ordered_devices
            .iter()
            .position(|device| *device == effects.device_id())
            .ok_or_else(|| {
                AuraError::permission_denied(
                    "original approved signing policy excludes current physical device",
                )
            })?;
        let index = u16::try_from(position + 1).map_err(|source| AuraError::Crypto {
            message: "approved native participant index exceeds bounds".into(),
            source: Some(Arc::new(source)),
        })?;
        let participant = ParticipantIdentity::device(effects.device_id());
        // This is the only physical participant package read. No roster loop
        // loads private packages and the coordinator never receives this grant.
        let local_share = zeroize::Zeroizing::new(
            self.participant_key_package(&manifest.subject, active.epoch, &participant)
                .await?,
        );
        let native_local = zeroize::Zeroizing::new(
            frost_ed25519::keys::KeyPackage::deserialize(&local_share).map_err(|source| {
                AuraError::Crypto {
                    message: "decode original approved local participant share".into(),
                    source: Some(Arc::new(source)),
                }
            })?,
        );
        let native_index =
            frost_ed25519::Identifier::try_from(index).map_err(|source| AuraError::Crypto {
                message: "decode original approved participant index".into(),
                source: Some(Arc::new(source)),
            })?;
        if *native_local.identifier() != native_index
            || *native_local.min_signers() != policy.threshold_k
            || native_local.verifying_key() != native_public.verifying_key()
            || native_public.verifying_shares().get(&native_index)
                != Some(native_local.verifying_share())
            || native_public.verifying_shares().len() != ordered_devices.len()
        {
            return Err(AuraError::permission_denied(
                "actual local native share does not satisfy original approved policy",
            ));
        }
        let root = manifest
            .final_inventory
            .as_ref()
            .and_then(|inventory| {
                inventory
                    .iter()
                    .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
            })
            .ok_or_else(|| {
                AuraError::permission_denied("approved manifest lacks exact active root inventory")
            })?;
        if root.epoch != active.epoch
            || root.commitment != manifest.final_commitment
            || root.agreement != policy.agreement_mode
            || root.mode != SigningMode::Threshold
            || root.threshold != policy.threshold_k
            || root.participants != roster
            || root.public_key_package != public_package
        {
            return Err(AuraError::permission_denied("user-approved manifest inventory differs from independently protected local policy"));
        }
        Ok(ValidatedLocalEnrollmentSigningMaterial {
            effects: effects.clone(),
            custody,
            device: effects.device_id(),
            index,
            threshold: policy.threshold_k,
            ordered_devices,
            public_package,
            verifying_key: native_public.verifying_key().serialize().to_vec(),
            local_share,
        })
    }
}
/// Local approval after protected generation and exact transcript verification.
/// No wire DTO, deserialization or clone can mint this local grant.
pub(super) struct ApprovedLocalEnrollmentTranscript<'tree, 'custody, 'owner, 'runtime> {
    custody:
        &'tree crate::runtime::effects::EnrollmentTranscriptTreeOwner<'custody, 'owner, 'runtime>,
    effects: Arc<AuraEffectSystem>,
    device: DeviceId,
    index: u16,
    threshold: u16,
    public_package: Vec<u8>,
    domains: [ApprovedEnrollmentTranscriptDomain; 3],
    local_share: &'tree [u8],
    /// Original sealed participant-local window; no peer clock is accepted.
    original_window: crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
}

struct ApprovedEnrollmentTranscriptDomain {
    message: Vec<u8>,
    retirement_location: SecureStorageLocation,
    approval_digest: [u8; 32],
}

/// Minted only from the independently retained native policy and explicit
/// canonical three-domain approval. It contains public material exclusively.
pub(super) struct ApprovedEnrollmentTranscriptRound {
    effects: Arc<AuraEffectSystem>,
    original_window: crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    message: Vec<u8>,
    public_package: Vec<u8>,
    verifying_key: aura_core::TrustedPublicKey,
    threshold: u16,
    participants: Vec<(DeviceId, u16)>,
}

// The participant actor is a private child
// module of this owner, so private material fields need no public secret getters.
impl<'tree, 'custody, 'owner, 'runtime>
    ValidatedLocalEnrollmentSigningMaterial<'tree, 'custody, 'owner, 'runtime>
{
    fn approved_rounds(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        original_window: &crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> Result<[ApprovedEnrollmentTranscriptRound; 3], AuraError> {
        use aura_signature::SecurityTranscript;
        self.custody
            .require_manifest(self.effects.as_ref(), approval.manifest())?;
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.device != approval.manifest().initiator_device
            || self.verifying_key != approval.manifest().initiator_confirmation_verifier
        {
            return Err(AuraError::permission_denied(
                "coordinator is not the original approved initiating device",
            ));
        }
        aura_invitation::shareable::require_transport_manifest(
            approval.transport(),
            approval.manifest(),
        )
        .map_err(|source| {
            AuraError::crypto_with_source(
                "bind original coordinator transport intent",
                Arc::new(source),
            )
        })?;
        let participants = self
            .ordered_devices
            .iter()
            .enumerate()
            .map(|(offset, device)| {
                u16::try_from(offset + 1)
                    .map(|index| (*device, index))
                    .map_err(|source| {
                        AuraError::crypto_with_source(
                            "retain original ordered coordinator roster",
                            Arc::new(source),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let messages = [
            approval
                .manifest()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode approved coordinator manifest",
                        Arc::new(source),
                    )
                })?,
            approval
                .transport()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode approved coordinator public transport",
                        Arc::new(source),
                    )
                })?,
            approval
                .initial_request()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode explicitly approved initial request",
                        Arc::new(source),
                    )
                })?,
        ];
        Ok(messages.map(|message| ApprovedEnrollmentTranscriptRound {
            effects: self.effects.clone(),
            original_window: original_window.clone(),
            message,
            public_package: self.public_package.clone(),
            verifying_key: aura_core::TrustedPublicKey::active(
                aura_core::TrustedKeyDomain::AuthorityThreshold,
                Some(approval.manifest().final_epoch),
                self.verifying_key.clone(),
                aura_core::Hash32(aura_core::hash::hash(&self.verifying_key)),
            ),
            threshold: self.threshold,
            participants: participants.clone(),
        }))
    }
    /// Exactly the three approved domains; no caller-provided signing message is accepted.
    fn prepare_manifest_participant<'grant>(
        &'grant self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        original_window: &crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> Result<
        (
            enrollment_transcript_signing::EnrollmentTranscriptParticipantIngress,
            impl std::future::Future<Output = Result<(), AuraError>>
                + 'grant
                + use<'grant, 'tree, 'custody, 'owner, 'runtime>,
        ),
        AuraError,
    >
    where
        'runtime: 'owner,
        'owner: 'custody,
        'custody: 'tree,
        'tree: 'grant,
    {
        use aura_signature::SecurityTranscript;
        self.custody
            .require_manifest(self.effects.as_ref(), approval.manifest())?;
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.verifying_key != approval.manifest().initiator_confirmation_verifier
        {
            return Err(AuraError::PermissionDenied {
                message: "approved manifest identity differs from original native group key".into(),
                source: Some(Arc::new(
                    crate::runtime::effects::EnrollmentFinalInventoryError::OwnerBinding,
                )),
            });
        }
        let message = approval
            .manifest()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode exact user-approved manifest domain",
                    Arc::new(source),
                )
            })?;
        aura_invitation::shareable::require_transport_manifest(
            approval.transport(),
            approval.manifest(),
        )
        .map_err(|source| {
            AuraError::crypto_with_source(
                "bind originally approved transport to manifest",
                Arc::new(source),
            )
        })?;
        let transport_message =
            approval
                .transport()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode originally approved public transport domain",
                        Arc::new(source),
                    )
                })?;
        let request_message = approval
            .initial_request()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode explicitly approved initial request domain",
                    Arc::new(source),
                )
            })?;
        // Ceremony/invitation/domain make the immutable approval allocation
        // stable on restart. This is a session binding, not a remote deadline.
        let session_bytes = aura_core::util::serialization::to_vec(&(
            "aura.enrollment.manifest-approval-retirement.v1",
            approval.manifest().subject,
            &approval.manifest().ceremony,
            &approval.manifest().invitation,
            aura_core::hash::hash(&message),
        ))?;
        let digest = aura_core::hash::hash(&session_bytes);
        let retirement_location = SecureStorageLocation::with_sub_key(
            "enrollment_signing_approval_retirement",
            approval.manifest().subject.to_string(),
            hex::encode(digest),
        );
        let transport_digest = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
            "aura.enrollment.public-transport-approval-retirement.v1",
            approval.manifest().subject,
            &approval.manifest().ceremony,
            &approval.manifest().invitation,
            aura_core::hash::hash(&transport_message),
            approval.canonical_intent_digest(),
        ))?);
        let transport_location = SecureStorageLocation::with_sub_key(
            "enrollment_signing_approval_retirement",
            approval.manifest().subject.to_string(),
            hex::encode(transport_digest),
        );
        let request_digest = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
            "aura.enrollment.initial-request-approval-retirement.v1",
            approval.manifest().subject,
            &approval.manifest().ceremony,
            &approval.manifest().invitation,
            aura_core::hash::hash(&request_message),
            approval.canonical_intent_digest(),
        ))?);
        let request_location = SecureStorageLocation::with_sub_key(
            "enrollment_signing_approval_retirement",
            approval.manifest().subject.to_string(),
            hex::encode(request_digest),
        );
        let grant = ApprovedLocalEnrollmentTranscript {
            custody: self.custody,
            effects: self.effects.clone(),
            device: self.device,
            index: self.index,
            threshold: self.threshold,
            public_package: self.public_package.clone(),
            domains: [
                ApprovedEnrollmentTranscriptDomain {
                    message,
                    retirement_location,
                    approval_digest: digest,
                },
                ApprovedEnrollmentTranscriptDomain {
                    message: transport_message,
                    retirement_location: transport_location,
                    approval_digest: transport_digest,
                },
                ApprovedEnrollmentTranscriptDomain {
                    message: request_message,
                    retirement_location: request_location,
                    approval_digest: request_digest,
                },
            ],
            local_share: self.local_share.as_slice(),
            original_window: original_window.clone(),
        };
        Ok(enrollment_transcript_signing::admitted_participant(grant))
    }
}

impl ThresholdSigningService {
    pub(crate) async fn prepare_original_quorum_issuer(
        &self,
        agent: Arc<crate::core::AuraAgent>,
        nickname: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning, AuraError> {
        if !Arc::ptr_eq(&self.effects, &agent.runtime().effects()) {
            return Err(AuraError::permission_denied(
                "prepared issuer belongs to another actual runtime",
            ));
        }
        let capacity = self.shared.quorum.reserve()?;
        let original_task_id =
            aura_core::effects::RandomExtendedEffects::random_uuid(self.effects.as_ref()).await;
        let group = agent.runtime().tasks().group(format!(
            "enrollment.original-prepared-issuer-{original_task_id}"
        ));
        let (preparation, ready, approved_sender) =
            crate::runtime_bridge::enrollment_quorum::OriginalIssuerPreparation::owned_channels();
        let (completed_sender, completed) = tokio::sync::oneshot::channel();
        let issuer_task = async move {
            let bridge = crate::runtime_bridge::AgentRuntimeBridge::new(agent);
            match bridge
                .issue_original_device_enrollment(nickname, setup, Some(preparation))
                .await
            {
                Ok(result) => completed_sender.send(Ok(result)).map_err(|_| {
                    AuraError::invalid("original prepared issuer terminal receiver was cancelled")
                }),
                Err(source) => {
                    let native = AuraError::Internal {
                        message: "original owned enrollment issuance failed".into(),
                        source: Some(Arc::new(source)),
                    };
                    let _ = completed_sender.send(Err(aura_invitation::enrollment_setup::EnrollmentIssuanceError::at(
                        aura_invitation::enrollment_setup::EnrollmentIssuanceStage::InvitationExport, native.clone(),
                    )));
                    Err(native)
                }
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let task = group.spawn_local_try_named("enrollment.original-prepared-issuer", issuer_task);
            } else {
                let task = group.spawn_try_named("enrollment.original-prepared-issuer", issuer_task);
            }
        }
        let ready = match ready.await {
            Ok(ready) => ready,
            Err(channel_source) => {
                // No clock is manufactured for a failure before allocation.
                // Cancellation/forced-abort remains a failed teardown, never
                // evidence that the original native profile may be handed off.
                group.request_cancellation();
                let primary = match completed.await {
                    Ok(Err(source)) => AuraError::Internal {
                        message: "original issuer failed before preparation".into(),
                        source: Some(Arc::new(source)),
                    },
                    _ => AuraError::Internal {
                        message: "original issuer preparation channel ended".into(),
                        source: Some(Arc::new(channel_source)),
                    },
                };
                if let Err(source) = group.abort_remaining() {
                    return Err(AuraError::Internal { message: "original issuer failed before clock admission and could not acknowledge teardown".into(), source: Some(Arc::new(EnrollmentParticipantStartupFailure { primary, teardown: source })) });
                }
                return Err(primary);
            }
        };
        let (observed, original_completion, digest) = ready.into_owned_parts();
        let entry = enrollment_quorum_registry::PreparedIssuerEntry::new(
            capacity,
            aura_guards::GuardContextProvider::authority_id(self.effects.as_ref()),
            observed.ceremony_id.clone(),
            digest,
            self.effects.clone(),
            original_completion,
            approved_sender,
            completed,
            group,
            task,
        );
        if let Err((primary, entry)) = self.shared.quorum.insert_prepared(entry).await {
            if let Err(cleanup) = (*entry).cancel_and_drain().await {
                return Err(enrollment_quorum_registry::joined(primary, cleanup));
            }
            return Err(primary);
        }
        Ok(observed)
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RuntimeApprovedEnrollmentSigningIntent",
        family = "authorizer"
    )]
    pub(crate) async fn approve_original_quorum_participant(
        &self,
        approval: crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        group: &crate::task_registry::TaskGroup,
    ) -> Result<(), AuraError> {
        if approval.manifest().initiator_device == self.effects.device_id() {
            return Err(AuraError::permission_denied(
                "original issuer must resume its prepared owner",
            ));
        }
        let permit = self.shared.quorum.reserve()?;
        let started = self
            .install_approved_manifest_participant(approval, group)
            .await?;
        // Registry insertion below must use the retained exact runtime consent;
        // the actor owns it, so its opaque started record supplies that binding.
        self.shared
            .quorum
            .insert_started_participant(started, permit)
            .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RuntimeApprovedEnrollmentSigningIntent",
        family = "runtime_helper"
    )]
    pub(crate) async fn resume_original_quorum_issuer(
        &self,
        approval: crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<aura_app::runtime_bridge::DeviceEnrollmentStart, AuraError> {
        self.shared
            .quorum
            .take_for_original_approval(&approval)
            .await?
            .finish(approval)
            .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentWindowCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn sign_original_approved_enrollment_domains(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        custody: &crate::runtime::effects::EnrollmentTranscriptTreeOwner<'_, '_, '_>,
        window: &crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> Result<[Vec<u8>; 3], AuraError> {
        window
            .execute(self.effects.as_ref(), || async {
                let material = self
                    .admit_local_enrollment_transcript_material(approval, custody)
                    .await?;
                let [manifest_round, transport_round, request_round] =
                    material.approved_rounds(approval, window)?;
                let (local_ingress, local_participant) =
                    material.prepare_manifest_participant(approval, window)?;
                let mut ingresses = vec![local_ingress];
                let mut remote_participants = Vec::new();
                for device in material
                    .ordered_devices
                    .iter()
                    .copied()
                    .filter(|device| *device != material.device)
                {
                    let (ingress, participant) =
                        material.original_remote_proxy(approval, window, device)?;
                    ingresses.push(ingress);
                    remote_participants.push(participant);
                }
                let coordinate = async {
                    let manifest = manifest_round.sign(approval.manifest(), &ingresses).await?;
                    let transport = transport_round
                        .sign(approval.transport(), &ingresses)
                        .await?;
                    let request = request_round
                        .sign(approval.initial_request(), &ingresses)
                        .await?;
                    Ok::<_, AuraError>([manifest, transport, request])
                };
                let (_, _, signatures) = tokio::try_join!(
                    local_participant,
                    futures::future::try_join_all(remote_participants),
                    coordinate,
                )?;
                // All original participant/proxy frames are acknowledged before the
                // caller can consume the held issuer reservation or execution lease.
                Ok::<_, AuraError>(signatures)
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "original sealed approved enrollment quorum",
                    Arc::new(source),
                )
            })
    }
}
// Runtime assembly owns this group and
// retains each returned move-only actor handle in its bounded service ingress.
pub(super) struct StartedEnrollmentManifestParticipant {
    subject: aura_core::AuthorityId,
    ceremony: aura_core::CeremonyId,
    ingress: enrollment_transcript_signing::EnrollmentTranscriptParticipantIngress,
    task: aura_core::OwnedTaskHandle<u64>,
    group: crate::task_registry::TaskGroup,
    effects: Arc<AuraEffectSystem>,
    original_window: crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
}

impl StartedEnrollmentManifestParticipant {
    async fn cancel_and_drain(self) -> Result<(), AuraError> {
        tracing::debug!(
            task_id = *self.task.handle_id(),
            "Cancel original approved signing participant"
        );
        let result = self
            .original_window
            .shutdown_owned_group(self.effects.as_ref(), &self.group)
            .await
            .map_err(|source| AuraError::Crypto {
                message: "original approved participant cancellation drain failed".into(),
                source: Some(Arc::new(source)),
            });
        // Release retained bounded ingress after the required shutdown attempt.
        drop(self.ingress);
        result
    }
}

impl ThresholdSigningService {
    /// Internal actor assembly; native consent is already bound to its original
    /// app runtime. This is not a public raw-effect/transcript signing method.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RuntimeApprovedEnrollmentSigningIntent",
        family = "runtime_helper"
    )]
    async fn install_approved_manifest_participant(
        &self,
        approval: crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        group: &crate::task_registry::TaskGroup,
    ) -> Result<StartedEnrollmentManifestParticipant, AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects()) {
            return Err(AuraError::PermissionDenied {
                message: "original approved actor differs from selected runtime".into(),
                source: Some(Arc::new(
                    crate::runtime::effects::EnrollmentFinalInventoryError::OwnerBinding,
                )),
            });
        }
        let service = self.clone();
        let subject = approval.manifest().subject;
        let ceremony = approval.manifest().ceremony.clone();
        let actor_group = group.group(format!(
            "enrollment-manifest-{}-{}",
            approval.manifest().ceremony,
            self.effects.device_id()
        ));
        let effects = self.effects.clone();
        // Clock admission precedes native custody acquisition. It cannot mint
        // signing authority and cannot accept a reconstructed caller budget.
        let original = crate::runtime::services::enrollment_window::EnrollmentWindowCapability::approved_signing(
            self.effects.clone(), &approval,
        ).await?;
        let readiness_window = original.clone();
        let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
        let participant_task = async move {
            let mut ready_sender = Some(ready_sender);
            let outcome = async {
                let custody = original.execute(
                    effects.as_ref(),
                    || effects.acquire_approved_enrollment_tree_custody(&approval),
                ).await.map_err(|source| AuraError::Crypto {
                    message: "original approved participant custody acquisition failed".into(),
                    source: Some(Arc::new(source)),
                })?;
                let owner = crate::runtime::effects::EnrollmentTranscriptTreeOwner::from_original_participant(&custody);
                let material = original.execute(
                    effects.as_ref(),
                    || service.admit_local_enrollment_transcript_material(&approval, &owner),
                ).await.map_err(|source| AuraError::Crypto {
                    message: "original approved participant material admission failed".into(),
                    source: Some(Arc::new(source)),
                })?;
                let (ingress, participant) = material.prepare_manifest_participant(&approval, &original)?;
                let sender = ready_sender.take().ok_or_else(|| AuraError::Crypto {
                    message: "original approved readiness sender already retired".into(),
                    source: Some(Arc::new(enrollment_transcript_signing::EnrollmentTranscriptSigningError::ResponseCancelled)),
                })?;
                sender.send(Ok(ingress.clone()))
                    .map_err(|_| AuraError::Crypto {
                        message: "original approved participant readiness receiver cancelled".into(),
                        source: Some(Arc::new(enrollment_transcript_signing::EnrollmentTranscriptSigningError::ResponseCancelled)),
                    })?;
                // All actual native custody remains in this owned actor frame.
                let dispatcher = material.dispatch_original_participant(&approval, &original, &ingress);
                tokio::try_join!(participant, dispatcher).map(|_| ())
            }.await;
            if let Err(original) = &outcome {
                if let Some(sender) = ready_sender.take() {
                    let _ = sender.send(Err(original.clone()));
                }
            }
            outcome
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let task = actor_group.spawn_local_try_named("enrollment.approved-manifest-participant", participant_task);
            } else {
                let task = actor_group.spawn_try_named("enrollment.approved-manifest-participant", participant_task);
            }
        }
        let readiness = readiness_window
            .execute(self.effects.as_ref(), || async {
                match ready_receiver.await {
                    Ok(outcome) => outcome,
                    Err(channel_source) => {
                        // Observe actual original task teardown before choosing
                        // a channel error, preserving native admission/panic cause.
                        readiness_window
                            .wait_owned_group(self.effects.as_ref(), &actor_group)
                            .await
                            .map_err(|source| AuraError::Crypto {
                                message: "original approved actor failed before readiness".into(),
                                source: Some(Arc::new(source)),
                            })?;
                        Err(AuraError::Crypto {
                            message: "original approved actor readiness channel ended".into(),
                            source: Some(Arc::new(channel_source)),
                        })
                    }
                }
            })
            .await
            .map_err(|source| AuraError::Crypto {
                message: "original approved actor readiness window failed".into(),
                source: Some(Arc::new(source)),
            });
        let ingress = match readiness {
            Ok(ingress) => ingress,
            Err(primary) => {
                // TaskGroup has no cancelling Drop. Every failure after spawn
                // explicitly retires the owned actor before returning.
                if let Err(teardown) = readiness_window
                    .shutdown_owned_group(self.effects.as_ref(), &actor_group)
                    .await
                {
                    return Err(AuraError::Crypto {
                        message: "approved participant readiness and teardown failed".into(),
                        source: Some(Arc::new(EnrollmentParticipantStartupFailure {
                            primary,
                            teardown,
                        })),
                    });
                }
                return Err(primary);
            }
        };
        Ok(StartedEnrollmentManifestParticipant {
            subject,
            ceremony,
            ingress,
            task,
            group: actor_group,
            effects: self.effects.clone(),
            original_window: readiness_window,
        })
    }
}

#[derive(Debug)]
struct EnrollmentParticipantStartupFailure {
    primary: AuraError,
    teardown: crate::task_registry::TaskSupervisionError,
}

impl std::fmt::Display for EnrollmentParticipantStartupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; owned teardown: {}",
            self.primary, self.teardown
        )
    }
}

impl std::error::Error for EnrollmentParticipantStartupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}
