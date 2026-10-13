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

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub(super) type EnrollmentOperationFuture<'a, T> = futures::future::LocalBoxFuture<'a, T>;
    } else {
        pub(super) type EnrollmentOperationFuture<'a, T> = futures::future::BoxFuture<'a, T>;
    }
}

mod device_quorum;
pub(crate) use device_quorum::DevicePossessionProof;
pub(super) use device_quorum::OriginalDeviceProposalConsent;
mod enrollment_peer;
mod enrollment_quorum_registry;
#[cfg(test)]
mod peer_contract_tests;
pub(super) use enrollment_peer::OriginalEnrollmentPeerNativeProgress;
pub(crate) use enrollment_quorum_registry::OriginalPreparedIssuerRegistration;
mod enrollment_transcript_proxy;
mod enrollment_transcript_signing;
#[cfg(test)]
pub(crate) use enrollment_transcript_signing::{
    OriginalPositiveActorTest, OriginalPositiveActorTestMode,
};
mod prepared_issuer_custody;
pub(super) use enrollment_transcript_signing::{
    original_participant_actor, EnrollmentTranscriptParticipantIngress,
    EnrollmentTranscriptSigningError, OriginalEnrollmentTranscriptActor,
};
mod conditional_retirement;
mod positive_terminal;
pub(crate) use device_quorum::original_device_quorum_response_matches;
pub(super) use positive_terminal::OriginalPositiveDispatcher;
pub(crate) use positive_terminal::{OriginalPositivePacket, OriginalPositiveTransport};
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
use aura_invitation::enrollment_mailbox::{
    EnrollmentReturnEnvelopeAuthorization, EnrollmentReturnEnvelopeStatement,
    PendingEnrollmentMailboxError, PendingEnrollmentMailboxRegistration,
    PendingEnrollmentMailboxStatement, VerifiedIssuerEnrollmentReturnRegistration,
};
use aura_invitation::enrollment_setup::{
    DeviceEnrollmentSetupRequest, DeviceEnrollmentSetupStatement, EnrollmentSetupExportError,
};
use aura_protocol::effects::TreeEffects;
use aura_signature::threshold_signing_context_transcript_bytes;
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
pub(crate) use conditional_retirement::{
    OriginalConditionalRetirementTransport, OriginalRetiredConditionalTransport,
    OriginalRetiredConditionalTransportCustody,
};
pub(in crate::runtime::services) use enrollment_transcript_signing::{
    OriginalConditionalActorExit, OriginalParticipantCompletion,
    OriginalParticipantCompletionCustody,
};
pub(super) use enrollment_transcript_wire::{
    OriginalConditionalDispatcherExit, OriginalEnrollmentTranscriptDispatcher,
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

#[derive(Debug, thiserror::Error)]
#[error("signing service belongs to another runtime effect owner")]
pub(crate) struct ForeignThresholdSigningRuntime;

struct ThresholdSigningShared {
    quorum: Arc<enrollment_quorum_registry::EnrollmentQuorumRegistry>,
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

/// A runtime-local signer handle for the original pre-adoption return route.
/// Weak references prevent the effect system from retaining its own runtime.
pub(crate) trait OriginalEnrollmentReturnSigner: Send + Sync {
    fn sign_envelope<'a>(
        &'a self,
        effects: &'a AuraEffectSystem,
        registration: &'a VerifiedIssuerEnrollmentReturnRegistration,
        baseline: &'a aura_invitation::enrollment_manifest::VerifiedEnrollmentBaseline,
        setup: &'a aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
        envelope: &'a aura_core::effects::transport::TransportEnvelope,
    ) -> EnrollmentOperationFuture<
        'a,
        Result<EnrollmentReturnEnvelopeAuthorization, PendingEnrollmentMailboxSigningError>,
    >;
}

pub(crate) struct WeakEnrollmentReturnSigner {
    effects: std::sync::Weak<AuraEffectSystem>,
    shared: std::sync::Weak<ThresholdSigningShared>,
    runtime_capabilities: RuntimeCapabilityHandler,
}

impl WeakEnrollmentReturnSigner {
    pub(crate) fn upgrade_for(
        &self,
        effects: &AuraEffectSystem,
    ) -> Option<ThresholdSigningService> {
        let owned_effects = self.effects.upgrade()?;
        if !std::ptr::eq(owned_effects.as_ref(), effects) {
            return None;
        }
        Some(ThresholdSigningService {
            effects: owned_effects,
            shared: self.shared.upgrade()?,
            runtime_capabilities: self.runtime_capabilities.clone(),
        })
    }
}

impl OriginalEnrollmentReturnSigner for WeakEnrollmentReturnSigner {
    fn sign_envelope<'a>(
        &'a self,
        effects: &'a AuraEffectSystem,
        registration: &'a VerifiedIssuerEnrollmentReturnRegistration,
        baseline: &'a aura_invitation::enrollment_manifest::VerifiedEnrollmentBaseline,
        setup: &'a aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
        envelope: &'a aura_core::effects::transport::TransportEnvelope,
    ) -> EnrollmentOperationFuture<
        'a,
        Result<EnrollmentReturnEnvelopeAuthorization, PendingEnrollmentMailboxSigningError>,
    > {
        Box::pin(async move {
            let signer = self
                .upgrade_for(effects)
                .ok_or(PendingEnrollmentMailboxSigningError::ForeignSetup)?;
            signer
                .sign_enrollment_return_envelope_authorization(
                    registration,
                    baseline,
                    setup,
                    envelope,
                )
                .await
        })
    }
}

impl ThresholdSigningService {
    pub(crate) fn downgrade_for_original_return(&self) -> WeakEnrollmentReturnSigner {
        WeakEnrollmentReturnSigner {
            effects: Arc::downgrade(&self.effects),
            shared: Arc::downgrade(&self.shared),
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

#[derive(Debug, thiserror::Error)]
pub(crate) enum PendingEnrollmentMailboxSigningError {
    #[error("pending enrollment mailbox setup belongs to another runtime or signing policy")]
    ForeignSetup,
    #[error("pending enrollment mailbox provisional signing context is absent")]
    MissingContext,
    #[error("pending enrollment mailbox selected time failed: {0}")]
    Time(#[from] aura_core::effects::time::TimeError),
    #[error("pending enrollment mailbox binding or verification failed: {0}")]
    Registration(#[from] PendingEnrollmentMailboxError),
    #[error("pending enrollment mailbox signing failed: {0}")]
    Signing(#[from] AuraError),
}

impl ThresholdSigningService {
    /// Sign only while the actual provisional device key still owns the
    /// admitted setup context. This signature proves mailbox registration
    /// possession; it never authorizes account membership or identity signing.
    pub(crate) async fn sign_pending_enrollment_mailbox_registration(
        &self,
        baseline: &aura_invitation::enrollment_manifest::VerifiedEnrollmentBaseline,
        setup: &aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
        retrieval_credential_commitment: [u8; 32],
    ) -> Result<PendingEnrollmentMailboxRegistration, PendingEnrollmentMailboxSigningError> {
        let _transition = self.shared.transitions.lock().await;
        let now = self.effects.physical_time().await?.ts_ms;
        let statement = PendingEnrollmentMailboxStatement::for_admitted_original(
            baseline,
            setup,
            retrieval_credential_commitment,
        )?;
        let setup_statement = setup.statement();
        if self.effects.device_id() != setup_statement.device
            || setup_statement.signing_mode != SigningMode::SingleSigner
            || setup_statement.threshold != 1
            || setup_statement.participants != 1
        {
            return Err(PendingEnrollmentMailboxSigningError::ForeignSetup);
        }
        let guard = self.shared.state.read().await;
        let state = guard
            .contexts
            .get(&setup_statement.authority)
            .ok_or(PendingEnrollmentMailboxSigningError::MissingContext)?;
        if state.my_signer_index != Some(1)
            || state.mode != setup_statement.signing_mode
            || state.epoch != setup_statement.signing_epoch
            || state.config.threshold != setup_statement.threshold
            || state.config.total_participants != setup_statement.participants
            || state.public_key_package != setup_statement.public_key_package
        {
            return Err(PendingEnrollmentMailboxSigningError::ForeignSetup);
        }
        let context = statement.signing_context()?;
        let message = Self::serialize_signing_context(&context, state.epoch)?;
        let proof = self
            .sign_solo(&setup_statement.authority, &message, state)
            .await?;
        let registration = PendingEnrollmentMailboxRegistration { statement, proof };
        registration
            .clone()
            .verify_for_admitted_original(self.effects.as_ref(), baseline, setup, now)
            .await?;
        Ok(registration)
    }

    /// Authorize one reverse VM frame with Bob's original provisional setup
    /// key. The independently verified issuer route is required as input;
    /// the broker receipt alone never authenticates the original enrollment.
    pub(crate) async fn sign_enrollment_return_envelope_authorization(
        &self,
        registration: &VerifiedIssuerEnrollmentReturnRegistration,
        baseline: &aura_invitation::enrollment_manifest::VerifiedEnrollmentBaseline,
        setup: &aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
        envelope: &aura_core::effects::transport::TransportEnvelope,
    ) -> Result<EnrollmentReturnEnvelopeAuthorization, PendingEnrollmentMailboxSigningError> {
        let _transition = self.shared.transitions.lock().await;
        let statement = EnrollmentReturnEnvelopeStatement::for_admitted_original(
            registration,
            baseline,
            setup,
            envelope,
        )?;
        let policy = setup.statement();
        if self.effects.device_id() != policy.device
            || policy.signing_mode != SigningMode::SingleSigner
            || policy.threshold != 1
            || policy.participants != 1
        {
            return Err(PendingEnrollmentMailboxSigningError::ForeignSetup);
        }
        let guard = self.shared.state.read().await;
        let state = guard
            .contexts
            .get(&policy.authority)
            .ok_or(PendingEnrollmentMailboxSigningError::MissingContext)?;
        if state.my_signer_index != Some(1)
            || state.mode != policy.signing_mode
            || state.epoch != policy.signing_epoch
            || state.config.threshold != 1
            || state.config.total_participants != 1
            || state.public_key_package != policy.public_key_package
        {
            return Err(PendingEnrollmentMailboxSigningError::ForeignSetup);
        }
        let now = self.effects.physical_time().await?.ts_ms;
        if now < policy.issued_at_ms || now >= registration.statement().expires_at_ms() {
            return Err(PendingEnrollmentMailboxError::OutsideValidity.into());
        }
        let message = Self::serialize_signing_context(&statement.signing_context()?, state.epoch)?;
        let proof = self.sign_solo(&policy.authority, &message, state).await?;
        let authorization = EnrollmentReturnEnvelopeAuthorization { statement, proof };
        authorization
            .clone()
            .verify_for_admitted_original(
                self.effects.as_ref(),
                registration,
                baseline,
                setup,
                envelope,
            )
            .await?;
        Ok(authorization)
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "ThresholdSigningService", receiver_type = ThresholdSigningService, family = "runtime_helper")]
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.as_ref(), effects) {
            return Err(AuraError::PermissionDenied {
                message: "signing service has another runtime owner".into(),
                source: Some(Arc::new(ForeignThresholdSigningRuntime)),
            });
        }
        Ok(())
    }
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
                quorum: Arc::new(enrollment_quorum_registry::EnrollmentQuorumRegistry::new()),
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
                quorum: Arc::new(enrollment_quorum_registry::EnrollmentQuorumRegistry::new()),
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
            .bootstrap_signing_keys(
                crate::runtime::services::RuntimeServiceContext::test_original(
                    agent.runtime().tasks(),
                    agent.runtime().effects(),
                )
                .await
                .startup_window(),
            )
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
        let original_effects = runtime.effects();
        let original_tasks = runtime.tasks();
        original
            .execute_service(
                original_effects.as_ref(),
                &original_tasks,
                "test_startup_drain",
                || async {
                    original_tasks
                        .await_registered_task_completion_for_test(
                            "runtime.runtime_startup_maintenance::initial_lan_descriptor",
                        )
                        .await
                        .map_err(|source| {
                            ServiceError::shutdown_failed(
                                "test_startup_drain",
                                "required startup task failed",
                            )
                            .with_cause(source)
                        })
                },
            )
            .await?;
        assert!(!original_tasks
            .active_tasks()
            .iter()
            .any(|task| { task == "runtime.runtime_startup_maintenance::initial_lan_descriptor" }));
        assert_eq!(clock.now_ms(), 100);
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

enum EnrollmentSigningMaterialCustody {
    Issuer(
        Arc<crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<'static, 'static>>,
    ),
    Owned(Arc<crate::runtime::effects::ApprovedEnrollmentTreeCustody<'static>>),
}
impl EnrollmentSigningMaterialCustody {
    fn generation(&self) -> &crate::runtime::effects::EnrollmentGenerationCustodyCapability<'_> {
        match self {
            Self::Issuer(original) => original.generation(),
            Self::Owned(original) => original.generation(),
        }
    }
    async fn send_owned_enrollment_round(
        &self,
        effects: &AuraEffectSystem,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
        window: &super::enrollment_window::EnrollmentExecutionChild,
        destination: DeviceId,
        packet: &aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket,
    ) -> Result<(), AuraError> {
        match self {
            Self::Issuer(original) => {
                let owner =
                    crate::runtime::effects::EnrollmentTranscriptTreeOwner::from_original_issuer(
                        original,
                    );
                effects
                    .send_owned_enrollment_round(&owner, manifest, window, destination, packet)
                    .await
            }
            Self::Owned(original) => {
                let original = crate::runtime::effects::EnrollmentTranscriptTreeOwner::from_original_participant(original.as_ref());
                effects
                    .send_owned_enrollment_round(&original, manifest, window, destination, packet)
                    .await
            }
        }
    }

    fn require_manifest(
        &self,
        effects: &AuraEffectSystem,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> Result<(), AuraError> {
        match self {
            Self::Issuer(original) => original.require_manifest(effects, manifest),
            Self::Owned(original) => original.require_manifest(effects, manifest),
        }
    }
}
pub(super) struct ValidatedLocalEnrollmentSigningMaterial {
    effects: Arc<AuraEffectSystem>,
    custody: EnrollmentSigningMaterialCustody,
    device: DeviceId,
    approved_intent_digest: [u8; 32],
    index: u16,
    threshold: u16,
    ordered_devices: Vec<DeviceId>,
    public_package: Vec<u8>,
    verifying_key: Vec<u8>,
    local_share: zeroize::Zeroizing<Vec<u8>>,
}

impl ThresholdSigningService {
    pub(super) async fn admit_original_issuer_enrollment_material(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        custody: Arc<
            crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<'static, 'static>,
        >,
    ) -> Result<ValidatedLocalEnrollmentSigningMaterial, AuraError> {
        self.admit_local_enrollment_material_with_custody(
            approval,
            EnrollmentSigningMaterialCustody::Issuer(custody),
        )
        .await
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalApprovedNativeAdmission",
        family = "runtime_helper"
    )]
    pub(super) async fn admit_original_owned_enrollment_material(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        custody: &crate::runtime::effects::OriginalApprovedNativeAdmission,
    ) -> Result<ValidatedLocalEnrollmentSigningMaterial, AuraError> {
        let original = custody.original_authenticated(approval)?;
        self.admit_local_enrollment_material_with_custody(
            approval,
            EnrollmentSigningMaterialCustody::Owned(original),
        )
        .await
    }
    async fn admit_local_enrollment_material_with_custody(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        custody: EnrollmentSigningMaterialCustody,
    ) -> Result<ValidatedLocalEnrollmentSigningMaterial, AuraError> {
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
            .iter()
            .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
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
            approved_intent_digest: approval.canonical_intent_digest(),
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
pub(super) struct ApprovedLocalEnrollmentTranscript<'tree> {
    custody: &'tree EnrollmentSigningMaterialCustody,
    effects: Arc<AuraEffectSystem>,
    device: DeviceId,
    index: u16,
    threshold: u16,
    public_package: Vec<u8>,
    domains: [ApprovedEnrollmentTranscriptDomain; 4],
    approved_intent_digest: [u8; 32],
    local_share: &'tree [u8],
    /// Original sealed participant-local window; no peer clock is accepted.
    original_window: &'tree crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
}

struct ApprovedEnrollmentTranscriptDomain {
    message: Vec<u8>,
    retirement_location: SecureStorageLocation,
    approval_digest: [u8; 32],
}

/// Minted only from the independently retained native policy and explicit
/// canonical four-signature-domain approval. It contains public material exclusively.
pub(super) struct ApprovedEnrollmentTranscriptRound<'window> {
    effects: Arc<AuraEffectSystem>,
    original_window: &'window crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    message: Vec<u8>,
    public_package: Vec<u8>,
    verifying_key: aura_core::TrustedPublicKey,
    threshold: u16,
    participants: Vec<(DeviceId, u16)>,
}

// The participant actor is a private child
// module of this owner, so private material fields need no public secret getters.
impl ValidatedLocalEnrollmentSigningMaterial {
    fn approved_rounds<'window>(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        original_window: &'window crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<[ApprovedEnrollmentTranscriptRound<'window>; 4], AuraError> {
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
            approval
                .issuer_return_mailbox()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode approved issuer return mailbox",
                        Arc::new(source),
                    )
                })?,
        ];
        Ok(messages.map(|message| ApprovedEnrollmentTranscriptRound {
            effects: self.effects.clone(),
            original_window,
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
    pub(super) fn original_participant_actor(
        &self,
    ) -> (
        EnrollmentTranscriptParticipantIngress,
        OriginalEnrollmentTranscriptActor,
    ) {
        original_participant_actor(self.device, self.index)
    }
    /// Exactly the four approved signature domains; no caller-provided message is accepted.
    pub(super) fn approved_local_transcript<'grant>(
        &'grant self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        original_window: &'grant crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<ApprovedLocalEnrollmentTranscript<'grant>, AuraError> {
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
        let return_message = approval
            .issuer_return_mailbox()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode explicitly approved issuer return mailbox",
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
        let return_digest = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
            "aura.enrollment.issuer-return-mailbox-approval-retirement.v1",
            approval.manifest().subject,
            &approval.manifest().ceremony,
            &approval.manifest().invitation,
            aura_core::hash::hash(&return_message),
            approval.canonical_intent_digest(),
        ))?);
        let return_location = SecureStorageLocation::with_sub_key(
            "enrollment_signing_approval_retirement",
            approval.manifest().subject.to_string(),
            hex::encode(return_digest),
        );
        let grant = ApprovedLocalEnrollmentTranscript {
            custody: &self.custody,
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
                ApprovedEnrollmentTranscriptDomain {
                    message: return_message,
                    retirement_location: return_location,
                    approval_digest: return_digest,
                },
            ],
            approved_intent_digest: approval.canonical_intent_digest(),
            local_share: self.local_share.as_slice(),
            original_window,
        };
        Ok(grant)
    }
}

impl ThresholdSigningService {
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    async fn launch_original_issuer(
        &self,
        agent: Arc<crate::core::AuraAgent>,
        nickname: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        mode: crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode,
    ) -> Result<Arc<enrollment_quorum_registry::PreparedIssuerEntry>, AuraError> {
        if !Arc::ptr_eq(&self.effects, &agent.runtime().effects()) {
            return Err(AuraError::permission_denied(
                "prepared issuer belongs to another actual runtime",
            ));
        }
        let capacity = self.shared.quorum.reserve()?;
        let (launch, launch_observation) =
            super::enrollment_window::OriginalIssuerLaunchWindow::select(self.effects.clone())
                .await?;
        let original_task_id = launch_observation
            .allocate_task_identity(&self.effects)
            .await?;
        let group = agent.runtime().tasks().group(format!(
            "enrollment.original-prepared-issuer-{original_task_id}"
        ));
        let (preparation, ready, approved_sender) =
            crate::runtime_bridge::enrollment_quorum::OriginalIssuerPreparation::owned_channels(
                mode, launch,
            );
        let retained = preparation.retained.clone();
        let registration = preparation.registration.clone();
        let ready_sender = preparation.ready.clone();
        let (start, started) = tokio::sync::oneshot::channel();
        let (completed_sender, completed) = tokio::sync::oneshot::channel();
        let completed_result = Arc::new(parking_lot::Mutex::new(None));
        let task_result = completed_result.clone();
        let task_ready = ready_sender.clone();
        let operation = prepared_issuer_custody::OriginalPreparedIssuerOperation {
            started,
            agent,
            nickname,
            setup,
            preparation,
            completed: completed_sender,
            result: task_result,
            mode,
            ready: task_ready,
        };
        #[cfg(test)]
        let resources_observation = Arc::downgrade(&retained);
        let custody = prepared_issuer_custody::OriginalPreparedIssuerTaskCustody {
            operation: Some(operation),
            retained,
            registration: registration.clone(),
            effects: self.effects.clone(),
            first_failure: None,
            ready: ready_sender,
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let task = group.spawn_original_local_required_custody("enrollment.original-prepared-issuer".into(), Box::new(custody), None)
                    .map_err(|rejected| prepared_issuer_custody::retain_rejected(rejected.into_local()))?;
            } else {
                let task = group.spawn_original_required_custody("enrollment.original-prepared-issuer".into(), Box::new(custody), None)
                    .map_err(|rejected| prepared_issuer_custody::retain_rejected(rejected.into_native()))?;
            }
        }
        let entry = Arc::new(enrollment_quorum_registry::PreparedIssuerEntry::new(
            capacity,
            aura_core::CeremonyId::new(format!("preparation:{original_task_id}")),
            aura_guards::GuardContextProvider::authority_id(self.effects.as_ref()),
            self.effects.clone(),
            approved_sender,
            ready,
            launch_observation,
            completed,
            completed_result,
            group,
            task,
        ));
        #[cfg(test)]
        entry.retain_resources_observation_for_test(resources_observation);
        registration
            .set(OriginalPreparedIssuerRegistration::new(
                &entry,
                &self.shared.quorum,
            ))
            .map_err(|_| AuraError::invalid("original prepared registration already installed"))?;
        if let Err((primary, entry)) = self.shared.quorum.insert_prepared(entry.clone()) {
            drop(start);
            if let Err(cleanup) = entry.cancel_and_drain().await {
                return Err(enrollment_quorum_registry::joined(primary, cleanup));
            }
            return Err(primary);
        }
        start
            .send(())
            .map_err(|_| AuraError::invalid("original prepared task ended before admission"))?;
        Ok(entry)
    }

    pub(crate) async fn prepare_original_quorum_issuer(
        &self,
        agent: Arc<crate::core::AuraAgent>,
        nickname: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning, AuraError> {
        let entry = self.launch_original_issuer(agent, nickname, setup,
            crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode::UserApprovedQuorum).await?;
        entry.receive_initial_ready().await?.into_observed()
    }

    pub(crate) async fn issue_original_owned_issuer(
        &self,
        agent: Arc<crate::core::AuraAgent>,
        nickname: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<
        (
            aura_app::runtime_bridge::DeviceEnrollmentStart,
            crate::runtime_bridge::OriginalIssuedEnrollmentContinuation,
        ),
        AuraError,
    > {
        let entry = self
            .launch_original_issuer(
                agent,
                nickname,
                setup,
                crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode::SingleSigner,
            )
            .await?;
        match entry.receive_initial_ready().await? {
            crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady::Issued => {
                entry.receive_owned_result().await
            }
            _ => Err(AuraError::permission_denied(
                "ordinary issuer cannot observe quorum approval readiness",
            )),
        }
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
        capability = "EnrollmentExecutionChild",
        family = "runtime_helper"
    )]
    pub(crate) async fn sign_original_approved_enrollment_domains(
        &self,
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        custody: Arc<
            crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<'static, 'static>,
        >,
        window: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        retained: &mut Option<Arc<tokio::sync::Mutex<OriginalIssuerTranscriptSigningCustody>>>,
    ) -> Result<[Vec<u8>; 4], AuraError> {
        window
            .execute(self.effects.as_ref(), || async {
                if retained.is_none() {
                    *retained = Some(Arc::new(tokio::sync::Mutex::new(
                        OriginalIssuerTranscriptSigningCustody::new(Arc::clone(&approval)),
                    )));
                }
                let retained = retained
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original issuer signing custody missing"))?;
                let mut retained_guard = retained.lock().await;
                let retained = &mut *retained_guard;
                if retained.material.is_none() && retained.negative.is_none() {
                    retained.material = Some(
                        self.admit_original_issuer_enrollment_material(&approval, custody)
                            .await?,
                    );
                }
                let material = retained.material.as_ref().ok_or_else(|| {
                    AuraError::invalid("original issuer initial material already transferred")
                })?;
                let [manifest_round, transport_round, request_round, return_round] =
                    material.approved_rounds(&approval, window)?;
                if retained.local_actor.is_none() {
                    let (local_ingress, local_actor) =
                        enrollment_transcript_signing::original_participant_actor(
                            material.device,
                            material.index,
                        );
                    retained.ingresses.push(local_ingress);
                    retained.local_actor = Some(local_actor);
                    for (offset, device) in material
                        .ordered_devices
                        .iter()
                        .copied()
                        .enumerate()
                        .filter(|(_, device)| *device != material.device)
                    {
                        let index = u16::try_from(offset + 1).map_err(|source| {
                            AuraError::crypto_with_source(
                                "original proxy roster index",
                                Arc::new(source),
                            )
                        })?;
                        let (ingress, actor) =
                            enrollment_transcript_proxy::original_remote_proxy_actor(device, index);
                        retained.ingresses.push(ingress);
                        retained.remote_actors.push(actor);
                    }
                }
                let local_participant = retained
                    .local_actor
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original local signing actor missing"))?
                    .run_initial(
                        material.approved_local_transcript(&approval, window)?,
                        Arc::clone(&retained.completion),
                    );
                let remote_participants = retained
                    .remote_actors
                    .iter_mut()
                    .map(|actor| actor.run_initial(material, &approval, window));
                let ingresses = &retained.ingresses;
                let coordinate = async {
                    let manifest = manifest_round.sign(approval.manifest(), ingresses).await?;
                    let transport = transport_round
                        .sign(approval.transport(), ingresses)
                        .await?;
                    let request = request_round
                        .sign(approval.initial_request(), ingresses)
                        .await?;
                    let return_mailbox = return_round
                        .sign(approval.issuer_return_mailbox(), ingresses)
                        .await?;
                    Ok::<_, AuraError>([manifest, transport, request, return_mailbox])
                };
                let (_, _, signatures) = tokio::try_join!(
                    local_participant,
                    futures::future::try_join_all(remote_participants),
                    coordinate,
                )?;
                let actual = retained.material.take().ok_or_else(|| {
                    AuraError::invalid("original issuer material missing at transition")
                })?;
                match actual.transfer_original_negative(
                    &approval,
                    Arc::clone(&retained.completion),
                    window,
                ) {
                    Ok(actual) => retained.negative = Some(actual),
                    Err(rejected) => {
                        let (actual, source) = *rejected;
                        retained.material = Some(actual);
                        return Err(source);
                    }
                }
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
/// Original existing-device peer scope retained by the original participant
/// holder and entry. Public inventory is observed; this is not an activation
/// or a negative signing capability.
pub(in crate::runtime::services) struct OriginalEnrollmentPeerContinuation {
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    original: Arc<crate::runtime::services::enrollment_window::EnrollmentExecutionChild>,
    native: Option<OriginalEnrollmentPeerNativeInventory>,
    peer_native: Option<crate::runtime::effects::OriginalApprovedNativeAdmission>,
    native_progress: OriginalEnrollmentPeerNativeProgress,
    proposal: Option<aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal>,
    accepted_at_ms: Option<u64>,
    acceptance: Option<aura_sync::protocols::device_epoch_rotation::DeviceEpochAcceptance>,
    commit: Option<aura_sync::protocols::device_epoch_rotation::DeviceEpochCommit>,
    committed: bool,
    commit_execution_acknowledged: bool,
    #[cfg(test)]
    commit_postread_test: Option<Arc<OriginalPeerCommitPostreadTest>>,
    #[cfg(test)]
    positive_actor_test: Option<Arc<OriginalPositiveActorTest>>,
    pub(super) conditional_retirement: Option<Arc<OriginalRetiredConditionalTransportCustody>>,
    peer_disposal: Option<crate::runtime::effects::AcknowledgedApprovedNativeDisposal>,
    positive_terminal_retirement: Option<RetiredOriginalSigningMaterial>,
    positive_contribution_completed: bool,
    positive_execution_acknowledged: bool,
    positive_execution_outcome: enrollment_transcript_signing::ConditionalOutcomeCustody,
    positive_cleanup_retirement: Option<RetiredOriginalSigningMaterial>,
    first_failure: Option<AuraError>,
    changed: Arc<tokio::sync::Notify>,
    proposal_consent: Option<OriginalDeviceProposalConsent>,
    proposal_scope:
        Option<aura_sync::protocols::device_epoch_rotation::DeviceEpochProposalTranscriptPayload>,
}
struct OriginalEnrollmentPeerNativeInventory {
    device: DeviceId,
    index: u16,
    ordered_devices: Vec<DeviceId>,
    public_package: Vec<u8>,
    baseline_digest: [u8; 32],
    baseline_count: u32,
    final_commitment: [u8; 32],
    baseline: Vec<aura_core::AttestedOp>,
}
impl OriginalEnrollmentPeerContinuation {
    pub(super) fn from_original_root(
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        original: Arc<crate::runtime::services::enrollment_window::EnrollmentExecutionChild>,
        changed: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            approval,
            original,
            native: None,
            peer_native: None,
            native_progress: OriginalEnrollmentPeerNativeProgress::default(),
            proposal: None,
            accepted_at_ms: None,
            acceptance: None,
            commit: None,
            committed: false,
            commit_execution_acknowledged: false,
            #[cfg(test)]
            commit_postread_test: None,
            #[cfg(test)]
            positive_actor_test: None,
            conditional_retirement: None,
            peer_disposal: None,
            positive_terminal_retirement: None,
            positive_contribution_completed: false,
            positive_execution_acknowledged: false,
            positive_execution_outcome:
                enrollment_transcript_signing::ConditionalOutcomeCustody::default(),
            positive_cleanup_retirement: None,
            first_failure: None,
            changed,
            proposal_consent: None,
            proposal_scope: None,
        }
    }

    fn retain_original_proposal_consent(
        &mut self,
        lookup: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposalTranscriptPayload,
        consent: &mut Option<OriginalDeviceProposalConsent>,
    ) -> Result<(), AuraError> {
        use aura_signature::SecurityTranscript;
        use aura_sync::protocols::device_epoch_rotation::DeviceEpochProposalPurpose;
        let native = self.native.as_ref().ok_or_else(|| {
            AuraError::permission_denied("original peer native inventory not yet retained")
        })?;
        let actual = consent
            .as_ref()
            .ok_or_else(|| AuraError::invalid("actual original proposal consent absent"))?;
        let manifest = self.approval.manifest();
        let expected = enrollment_transcript_wire::original_participant_verifier(
            &native.ordered_devices,
            &native.public_package,
            manifest.initiator_device,
        )?;
        let scope = actual.require_original_scope(self.approval.effects().as_ref(), &expected)?;
        let manifest_bytes = manifest.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("bind original peer proposal manifest", Arc::new(source))
        })?;
        let DeviceEpochProposalPurpose::Enrollment {
            manifest_digest,
            enrolling_device_id,
        } = &scope.purpose
        else {
            return Err(AuraError::permission_denied(
                "peer consent is not enrollment scope",
            ));
        };
        if aura_core::util::serialization::to_vec(&scope)?
            != aura_core::util::serialization::to_vec(lookup)?
            || *manifest_digest != aura_core::Hash32::new(aura_core::hash::hash(&manifest_bytes))
            || *enrolling_device_id != manifest.invitee_device
            || scope.subject_authority != manifest.subject
            || scope.ceremony_id != manifest.ceremony
            || scope.pending_epoch != manifest.pending_epoch
            || scope.initiator_device_id != manifest.initiator_device
            || scope.participant_device_id != native.device
            || scope.public_key_package_hash
                != aura_core::Hash32::new(manifest.pending_public_key_package_digest)
            || scope.threshold_config_hash != manifest.pending_threshold_config_digest
            || native.baseline_digest != manifest.baseline_digest
            || native.baseline_count != manifest.baseline_count
            || native.final_commitment != manifest.final_commitment
        {
            return Err(AuraError::permission_denied(
                "proposal consent differs from complete original enrollment scope",
            ));
        }
        if self.proposal_consent.is_some() {
            return Err(AuraError::permission_denied(
                "original peer proposal consent already retained",
            ));
        }
        self.proposal_scope = Some(scope);
        self.proposal_consent = consent.take();
        Ok(())
    }

    fn require_proposal(
        &self,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
    ) -> Result<(), AuraError> {
        let native = self
            .native
            .as_ref()
            .ok_or_else(|| AuraError::permission_denied("original peer inventory absent"))?;
        let verifier = enrollment_transcript_wire::original_participant_verifier(
            &native.ordered_devices,
            &native.public_package,
            self.approval.manifest().initiator_device,
        )?;
        self.proposal_consent
            .as_ref()
            .ok_or_else(|| AuraError::permission_denied("actual original proposal consent absent"))?
            .require_original_proposal(self.approval.effects().as_ref(), proposal, &verifier)?;
        if let Some(original) = &self.proposal {
            if aura_core::util::serialization::to_vec(original)?
                != aura_core::util::serialization::to_vec(proposal)?
            {
                return Err(AuraError::permission_denied(
                    "original peer proposal already bound differently",
                ));
            }
        }
        Ok(())
    }
    fn retain_failure(&mut self, source: &AuraError) {
        self.first_failure.get_or_insert_with(|| source.clone());
        self.changed.notify_waiters();
    }
    pub(super) fn dispose_peer_positive_for_cleanup(&mut self) -> Result<(), AuraError> {
        self.native_progress.retire_positive_for_cleanup(
            &self.approval,
            &self.original,
            &mut self.positive_cleanup_retirement,
        )
    }
    pub(super) fn dispose_peer_native(&mut self) -> Result<(), AuraError> {
        self.native_progress.dispose_original_material();
        if self.peer_native.is_none() && self.peer_disposal.is_none() {
            return Ok(());
        }
        if let Some(native) = self.peer_native.take() {
            match native.dispose_original(&self.approval) {
                Ok(actual) => self.peer_disposal = Some(actual),
                Err(rejected) => {
                    let (native, source) = *rejected;
                    self.peer_native = Some(native);
                    return Err(source);
                }
            }
        }
        let actual = self
            .peer_disposal
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original peer native disposal absent"))?;
        actual.require_original(&self.approval)?;
        if let Some(source) = actual.original_failure() {
            return Err(source.clone());
        }
        self.changed.notify_waiters();
        Ok(())
    }
    pub(super) fn peer_activation_ready(&self) -> Result<bool, AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        if !(self.committed && self.peer_disposal.is_some() && self.commit_execution_acknowledged) {
            return Ok(false);
        }
        let disposal = self
            .peer_disposal
            .as_ref()
            .ok_or_else(|| AuraError::invalid("peer disposal absent"))?;
        disposal.require_original(&self.approval)?;
        if let Some(source) = disposal.original_failure() {
            return Err(source.clone());
        }
        if self.positive_terminal_retirement.is_some() {
            return Ok(true);
        }
        let material = self
            .native_progress
            .positive_terminal()
            .ok_or_else(|| AuraError::invalid("original peer positive owner absent"))?;
        material.require_original(&self.approval, &self.original)?;
        Ok(true)
    }
    pub(super) async fn finish_original_positive(
        owner: &super::enrollment_window::OriginalPeerPositiveContinuationOwner,
        original: &super::enrollment_window::EnrollmentExecutionChild,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        actor: &mut OriginalEnrollmentTranscriptActor,
        dispatcher: &mut OriginalPositiveDispatcher,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<(), AuraError> {
        let mut retained_guard = None;
        let observed = owner
            .execute(original, approval, |cell| {
                let retained_guard = &mut retained_guard;
                async move {
                    *retained_guard = Some(cell.lock_owned().await);
                    let actual = retained_guard
                        .as_mut()
                        .ok_or_else(|| AuraError::invalid("original peer lock absent"))?;
                    let operation = async {
                        if !actual.peer_activation_ready()? {
                            return Err(AuraError::invalid("original peer activation unresolved"));
                        }
                        #[cfg(test)]
                        if let Some(test) = actual.positive_actor_test.take() {
                            actor.install_positive_test(test);
                        }
                        if !actual.positive_contribution_completed {
                            let material =
                                actual.native_progress.positive_terminal().ok_or_else(|| {
                                    AuraError::invalid("original peer positive owner absent")
                                })?;
                            tokio::try_join!(
                                actor.run_positive(material, approval, original),
                                dispatcher.run(material, approval, original, ingress)
                            )?;
                            actual.positive_contribution_completed = true;
                        }
                        if actual.positive_terminal_retirement.is_none() {
                            let Self {
                                native_progress,
                                positive_terminal_retirement,
                                ..
                            } = &mut **actual;
                            native_progress.retire_positive_for_cleanup(
                                approval,
                                original,
                                positive_terminal_retirement,
                            )?;
                        }
                        Ok::<(), AuraError>(())
                    }
                    .await;
                    if let Err(source) = &operation {
                        actual.retain_failure(source);
                    }
                    operation
                }
            })
            .await;
        let result = match observed {
            Ok(observed) => match retained_guard.as_mut() {
                Some(actual) => actual.positive_execution_outcome.retain(observed),
                None => Err(AuraError::invalid(
                    "original peer operation did not acquire its retained cell",
                )),
            },
            Err(source) => Err(source),
        };
        if let Some(actual) = retained_guard.as_mut() {
            match &result {
                Ok(()) => {
                    actual.positive_execution_acknowledged = true;
                    actual.changed.notify_waiters();
                }
                Err(source) => actual.retain_failure(source),
            }
        }
        result
    }
    #[cfg(test)]
    pub(super) fn completed_peer_activation(&self) -> Result<bool, AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        if !(self.committed && self.peer_disposal.is_some() && self.commit_execution_acknowledged) {
            return Ok(false);
        }
        if !(self.positive_contribution_completed && self.positive_execution_acknowledged) {
            return Ok(false);
        }
        let Some(retired) = &self.positive_terminal_retirement else {
            return Ok(false);
        };
        retired.require_original(&self.approval)?;
        Ok(true)
    }

    pub(super) fn retain_original_native_reentry(
        &mut self,
        disposal: &mut crate::runtime::effects::AcknowledgedApprovedNativeDisposal,
    ) -> Result<(), AuraError> {
        disposal.require_original(&self.approval)?;
        if self.native.is_none() {
            return Err(AuraError::permission_denied(
                "actual original peer baseline absent",
            ));
        }
        if self.peer_native.is_none() {
            self.peer_native = Some(disposal.prepare_original_peer_reentry(&self.approval)?);
        }
        Ok(())
    }

    pub(super) fn retain_original_native_inventory(
        &mut self,
        material: &ValidatedLocalEnrollmentSigningMaterial,
        baseline: &[aura_core::AttestedOp],
    ) -> Result<(), AuraError> {
        material
            .custody
            .require_manifest(material.effects.as_ref(), self.approval.manifest())?;
        if !Arc::ptr_eq(&material.effects, self.approval.effects())
            || material.approved_intent_digest != self.approval.canonical_intent_digest()
            || material.device != self.approval.effects().device_id()
        {
            return Err(AuraError::permission_denied(
                "peer inventory differs from original participant",
            ));
        }
        if let Some(native) = &self.native {
            if native.device != material.device
                || native.index != material.index
                || native.ordered_devices != material.ordered_devices
                || native.public_package != material.public_package
                || aura_core::util::serialization::to_vec(&native.baseline)?
                    != aura_core::util::serialization::to_vec(&baseline)?
            {
                return Err(AuraError::permission_denied(
                    "original peer inventory already retained differently",
                ));
            }
            return Ok(());
        }
        let manifest = self.approval.manifest();
        self.native = Some(OriginalEnrollmentPeerNativeInventory {
            device: material.device,
            index: material.index,
            ordered_devices: material.ordered_devices.clone(),
            public_package: material.public_package.clone(),
            baseline_digest: manifest.baseline_digest,
            baseline_count: manifest.baseline_count,
            final_commitment: manifest.final_commitment,
            baseline: baseline.to_vec(),
        });
        Ok(())
    }
}

pub(super) struct StartedEnrollmentManifestParticipant {
    subject: aura_core::AuthorityId,
    ceremony: aura_core::CeremonyId,
    ingress: enrollment_transcript_signing::EnrollmentTranscriptParticipantIngress,
    task: crate::task_registry::OriginalRequiredTaskHandle,
    group: crate::task_registry::TaskGroup,
    effects: Arc<AuraEffectSystem>,
    original_window: Arc<crate::runtime::services::enrollment_window::EnrollmentExecutionChild>,
    peer_continuation: Arc<tokio::sync::Mutex<OriginalEnrollmentPeerContinuation>>,
}

/// Finite existing-device enrollment admission, distinct from invitee proof and
/// conditional negative signing. The actual state stays in the original holder.
pub(crate) struct OriginalEnrollmentPeerAdmission {
    cell: Arc<tokio::sync::Mutex<OriginalEnrollmentPeerContinuation>>,
    original: Arc<crate::runtime::services::enrollment_window::EnrollmentExecutionChild>,
    effects: Arc<AuraEffectSystem>,
    service: ThresholdSigningService,
    proposal_digest: [u8; 32],
}
impl OriginalEnrollmentPeerAdmission {
    fn require_proposal(
        &self,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
    ) -> Result<(), AuraError> {
        if aura_core::hash::hash(&aura_core::util::serialization::to_vec(proposal)?)
            != self.proposal_digest
        {
            return Err(AuraError::permission_denied(
                "peer admission proposal changed",
            ));
        }
        Ok(())
    }
    pub(crate) async fn prepare_acceptance(
        &self,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
    ) -> Result<aura_sync::protocols::device_epoch_rotation::DeviceEpochAcceptance, AuraError> {
        self.require_proposal(proposal)?;
        let mut retained_guard = None;
        let result = self
            .original
            .execute(self.effects.as_ref(), || async {
                retained_guard = Some(Arc::clone(&self.cell).lock_owned().await);
                let cell = retained_guard
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original peer guard absent"))?;
                let result = async {
                    cell.require_proposal(proposal)?;
                    if let Some(source) = &cell.first_failure {
                        return Err(source.clone());
                    }
                    if let Some(actual) = &cell.acceptance {
                        return Ok(actual.clone());
                    }
                    if cell.accepted_at_ms.is_none() {
                        cell.accepted_at_ms = Some(self.effects.physical_time().await?.ts_ms);
                    }
                    let accepted_at = cell
                        .accepted_at_ms
                        .ok_or_else(|| AuraError::invalid("original acceptance time absent"))?;
                    let OriginalEnrollmentPeerContinuation {
                        peer_native,
                        approval,
                        proposal_consent,
                        native_progress,
                        acceptance,
                        ..
                    } = &mut **cell;
                    let actual = self
                        .service
                        .prepare_original_enrollment_peer_acceptance(
                            peer_native.as_mut().ok_or_else(|| {
                                AuraError::invalid("original peer native owner absent")
                            })?,
                            approval,
                            proposal_consent
                                .as_ref()
                                .ok_or_else(|| AuraError::invalid("original consent absent"))?,
                            proposal,
                            accepted_at,
                            native_progress,
                        )
                        .await?;
                    *acceptance = Some(actual.clone());
                    Ok(actual)
                }
                .await;
                if let Err(source) = &result {
                    cell.retain_failure(source);
                }
                result
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "prepare original enrollment peer acceptance",
                    Arc::new(source),
                )
            });
        if let Err(source) = &result {
            if let Some(cell) = retained_guard.as_mut() {
                cell.retain_failure(source);
            }
        }
        result
    }
    #[cfg(test)]
    pub(crate) async fn apply_verified_commit_with_postread_drop_test(
        &self,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
        commit: &aura_sync::protocols::device_epoch_rotation::DeviceEpochCommit,
    ) -> Result<(), AuraError> {
        let test = self.cell.lock().await.commit_postread_test.clone();
        let Some(test) = test else {
            return self.apply_verified_commit(proposal, commit).await;
        };
        let mut original = Box::pin(self.apply_verified_commit(proposal, commit));
        tokio::select! {
            _ = test.entered.notified() => {},
            result = &mut original => panic!("actual original commit ended before postread gate: {result:?}"),
        }
        drop(original);
        {
            let actual = self.cell.lock().await;
            assert!(actual.committed, "real signed commit retained");
            assert!(
                actual.peer_disposal.is_some(),
                "actual native disposal retained"
            );
            assert!(!actual.commit_execution_acknowledged);
            assert!(actual.first_failure.is_none());
            assert!(!actual.completed_peer_activation()?);
        }
        if test.cleanup {
            let source = AuraError::crypto_with_source(
                "actual peer execution cancelled by test",
                test.cleanup_source.clone(),
            );
            self.cell.lock().await.retain_failure(&source);
            test.release.notify_one();
            let observer = test
                .disposal
                .lock()
                .expect("original disposal observation lock")
                .take()
                .expect("actual original disposal observer");
            let disposed = observer.await_disposal().await;
            let aura_core::effects::task::RequiredTaskExit::Failed(actual_source) =
                disposed.outcome()
            else {
                panic!(
                    "expected original failed execution, got {:?}",
                    disposed.outcome()
                );
            };
            let mut native: Option<&(dyn std::error::Error + 'static)> = Some(&actual_source);
            let mut found = false;
            while let Some(actual) = native {
                if let Some(actual) = actual.downcast_ref::<OriginalPeerCleanupCancellation>() {
                    found = std::ptr::eq(actual, test.cleanup_source.as_ref());
                    break;
                }
                native = actual.source();
            }
            assert!(
                found,
                "same concrete cancellation source retained through required disposal"
            );
            {
                let actual = self.cell.lock().await;
                assert!(actual.committed && actual.peer_disposal.is_some());
                assert!(actual.native_progress.positive_terminal().is_none());
                actual
                    .positive_cleanup_retirement
                    .as_ref()
                    .expect("actual cleanup retirement")
                    .require_original(&actual.approval)?;
                assert!(actual.positive_terminal_retirement.is_none());
                assert!(actual.completed_peer_activation().is_err());
                assert!(!actual.commit_execution_acknowledged);
            }
            test.completed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            test.cleanup_completed.notify_one();
            eprintln!("original positive cleanup regression: actual material retired by SAME required holder; original Committed decision unchanged; no positive completion");
            return Err(source);
        }
        test.release.notify_one();
        self.apply_verified_commit(proposal, commit).await?;
        {
            let actual = self.cell.lock().await;
            assert!(actual.commit_execution_acknowledged);
            assert!(actual.committed && actual.peer_disposal.is_some());
            assert!(actual.native_progress.positive_terminal().is_some());
            assert!(
                !actual.completed_peer_activation()?,
                "positive terminal owner still retained"
            );
        }
        test.completed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        eprintln!("original peer postread Drop regression: actual commit/disposal retained unresolved; SAME admission reentry validated commit ACK; positive terminal owner remains retained");
        Ok(())
    }
    pub(crate) async fn apply_verified_commit(
        &self,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
        commit: &aura_sync::protocols::device_epoch_rotation::DeviceEpochCommit,
    ) -> Result<(), AuraError> {
        self.require_proposal(proposal)?;
        let mut retained_guard = None;
        let result = self
            .original
            .execute(self.effects.as_ref(), || async {
                retained_guard = Some(Arc::clone(&self.cell).lock_owned().await);
                let cell = retained_guard
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original peer guard absent"))?;
                let result = async {
                    cell.require_proposal(proposal)?;
                    if let Some(source) = &cell.first_failure {
                        return Err(source.clone());
                    }
                    if let Some(original) = &cell.commit {
                        if aura_core::util::serialization::to_vec(original)?
                            != aura_core::util::serialization::to_vec(commit)?
                        {
                            return Err(AuraError::permission_denied(
                                "original peer commit changed",
                            ));
                        }
                    } else {
                        cell.commit = Some(commit.clone());
                    }
                    if !cell.committed {
                        let OriginalEnrollmentPeerContinuation {
                            peer_native,
                            approval,
                            proposal_consent,
                            native_progress,
                            conditional_retirement,
                            ..
                        } = &mut **cell;
                        let retirement = conditional_retirement.as_deref().ok_or_else(|| {
                            AuraError::invalid("actual conditional retirement absent")
                        })?;
                        retirement.require_original_window(&self.original)?;
                        self.service
                            .apply_original_enrollment_peer_commit(
                                peer_native.as_mut().ok_or_else(|| {
                                    AuraError::invalid("original peer native owner absent")
                                })?,
                                approval,
                                proposal_consent
                                    .as_ref()
                                    .ok_or_else(|| AuraError::invalid("original consent absent"))?,
                                proposal,
                                commit,
                                retirement,
                                &self.original,
                                native_progress,
                            )
                            .await?;
                        native_progress
                            .positive_terminal()
                            .ok_or_else(|| {
                                AuraError::invalid("actual positive terminal material absent")
                            })?
                            .require_original(approval, &self.original)?;
                        cell.committed = true;
                    }
                    cell.dispose_peer_native()?;
                    #[cfg(test)]
                    if !cell.commit_execution_acknowledged {
                        if let Some(test) = &cell.commit_postread_test {
                            test.arm();
                        }
                    }
                    Ok(())
                }
                .await;
                if let Err(source) = &result {
                    cell.retain_failure(source);
                }
                result
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "apply original enrollment peer commit",
                    Arc::new(source),
                )
            });
        if let Some(cell) = retained_guard.as_mut() {
            match &result {
                Ok(()) => {
                    cell.commit_execution_acknowledged = true;
                    cell.changed.notify_waiters();
                }
                Err(source) => cell.retain_failure(source),
            }
        }
        result
    }
}

impl ThresholdSigningService {
    #[cfg(test)]
    pub(crate) async fn install_original_peer_commit_postread_test(
        &self,
        subject: aura_core::AuthorityId,
        ceremony: &aura_core::CeremonyId,
        test: Arc<OriginalPeerCommitPostreadTest>,
    ) -> Result<(), AuraError> {
        let (cell, original, effects) = self.shared.quorum.original_peer_continuation(
            subject,
            ceremony,
            self.effects.device_id(),
        )?;
        if test.cleanup {
            *test
                .disposal
                .lock()
                .expect("original disposal observation lock") =
                Some(self.shared.quorum.original_peer_disposal_for_test(
                    subject,
                    ceremony,
                    self.effects.device_id(),
                )?);
        }
        original
            .execute(effects.as_ref(), || async {
                let mut actual = cell.lock().await;
                assert!(actual.commit_postread_test.is_none());
                actual.commit_postread_test = Some(test);
                Ok::<(), AuraError>(())
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "install original postread observation",
                    Arc::new(source),
                )
            })
    }
    #[cfg(test)]
    pub(crate) async fn install_original_positive_actor_test(
        &self,
        subject: aura_core::AuthorityId,
        ceremony: &aura_core::CeremonyId,
        test: Arc<OriginalPositiveActorTest>,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, AuraError> {
        let (cell, original, effects) = self.shared.quorum.original_peer_continuation(
            subject,
            ceremony,
            self.effects.device_id(),
        )?;
        let disposal = self.shared.quorum.original_peer_disposal_for_test(
            subject,
            ceremony,
            self.effects.device_id(),
        )?;
        original
            .execute(effects.as_ref(), || async {
                let mut actual = cell.lock().await;
                assert!(actual.positive_actor_test.is_none());
                actual.positive_actor_test = Some(test);
                Ok::<(), AuraError>(())
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "install actual positive actor test",
                    Arc::new(source),
                )
            })?;
        Ok(disposal)
    }
    #[cfg(test)]
    pub(crate) async fn assert_original_positive_actor_cleanup(
        &self,
        subject: aura_core::AuthorityId,
        ceremony: &aura_core::CeremonyId,
    ) {
        let (cell, _, _) = self
            .shared
            .quorum
            .original_peer_continuation(subject, ceremony, self.effects.device_id())
            .expect("same retained failed peer cell");
        let actual = cell.lock().await;
        assert!(actual.committed && actual.commit_execution_acknowledged);
        assert!(actual.peer_disposal.is_some());
        assert!(actual.first_failure.is_some());
        assert!(actual.native_progress.positive_terminal().is_none());
        actual
            .positive_cleanup_retirement
            .as_ref()
            .expect("actual original positive cleanup retirement")
            .require_original(&actual.approval)
            .unwrap();
        assert!(actual.positive_terminal_retirement.is_none());
        assert!(!actual.positive_contribution_completed && !actual.positive_execution_acknowledged);
        assert!(actual.completed_peer_activation().is_err());
    }
    pub(crate) async fn admit_original_enrollment_peer(
        &self,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
    ) -> Result<OriginalEnrollmentPeerAdmission, AuraError> {
        let (cell, original, effects) = self.shared.quorum.original_peer_continuation(
            proposal.subject_authority,
            &proposal.ceremony_id,
            self.effects.device_id(),
        )?;
        if !Arc::ptr_eq(&effects, &self.effects) {
            return Err(AuraError::permission_denied(
                "original peer runtime changed",
            ));
        }
        let mut retained_guard = None;
        let result = original
            .execute(effects.as_ref(), || async {
                retained_guard = Some(Arc::clone(&cell).lock_owned().await);
                let actual = retained_guard
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original peer guard absent"))?;
                let result = async {
                    actual.require_proposal(proposal)?;
                    if let Some(source) = &actual.first_failure {
                        return Err(source.clone());
                    }
                    if actual.proposal.is_none() {
                        actual.proposal = Some(proposal.clone());
                    }
                    let OriginalEnrollmentPeerContinuation {
                        approval,
                        peer_native,
                        ..
                    } = &mut **actual;
                    peer_native
                        .as_mut()
                        .ok_or_else(|| AuraError::invalid("original peer archive seed absent"))?
                        .acquire_original(approval)
                        .await
                }
                .await;
                if let Err(source) = &result {
                    actual.retain_failure(source);
                }
                result
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source("admit original enrollment peer", Arc::new(source))
            });
        if let Err(source) = &result {
            if let Some(actual) = retained_guard.as_mut() {
                actual.retain_failure(source);
            }
        }
        drop(retained_guard);
        result?;
        Ok(OriginalEnrollmentPeerAdmission {
            cell,
            original,
            effects,
            service: self.clone(),
            proposal_digest: aura_core::hash::hash(&aura_core::util::serialization::to_vec(
                proposal,
            )?),
        })
    }
}

impl ThresholdSigningService {
    pub(crate) fn verify_original_enrollment_peer_possession(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        device: DeviceId,
        claimed: &DevicePossessionProof,
        message: &[u8],
    ) -> Result<(), AuraError> {
        issued.require_runtime_owner(self.effects.as_ref())?;
        original.require_issued_notice_owner(issued)?;
        let manifest = issued.manifest();
        let root = manifest
            .final_inventory
            .iter()
            .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
            .ok_or_else(|| {
                AuraError::permission_denied("original enrollment root verifier absent")
            })?;
        let devices = root
            .participants
            .iter()
            .map(|participant| match participant {
                ParticipantIdentity::Device(device) => Ok(*device),
                _ => Err(AuraError::permission_denied(
                    "original enrollment roster is not device scoped",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let index = devices
            .iter()
            .position(|candidate| *candidate == device)
            .and_then(|offset| u16::try_from(offset + 1).ok())
            .ok_or_else(|| {
                AuraError::permission_denied("peer absent from original enrolled roster")
            })?;
        if claimed.epoch != root.epoch
            || claimed.index != index
            || claimed.package_digest != aura_core::Hash32::from_bytes(&root.public_key_package)
        {
            return Err(AuraError::permission_denied(
                "peer possession differs from original enrollment policy",
            ));
        }
        let verifier = enrollment_transcript_wire::original_participant_verifier(
            &devices,
            &root.public_key_package,
            device,
        )?;
        aura_core::crypto::participant_proof::verify_participant_key_proof(
            message,
            &claimed.proof,
            &verifier,
            aura_core::effects::crypto::SigningMode::Threshold,
        )
    }

    pub(super) async fn retain_original_enrollment_proposal_consent(
        &self,
        scope: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposalTranscriptPayload,
        consent: &mut Option<OriginalDeviceProposalConsent>,
    ) -> Result<(), AuraError> {
        let (cell, original, effects) = self.shared.quorum.original_peer_continuation(
            scope.subject_authority,
            &scope.ceremony_id,
            self.effects.device_id(),
        )?;
        if !Arc::ptr_eq(&effects, &self.effects) {
            return Err(AuraError::permission_denied(
                "original peer entry belongs to another runtime",
            ));
        }
        original
            .execute(effects.as_ref(), || async {
                cell.lock()
                    .await
                    .retain_original_proposal_consent(scope, consent)
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "retain actual original peer proposal consent",
                    Arc::new(source),
                )
            })
    }
}

impl StartedEnrollmentManifestParticipant {
    async fn cancel_and_drain(mut self) -> Result<(), AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        self.task.request_cancel();
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
        // Clock admission precedes native custody acquisition. It cannot mint
        // signing authority and cannot accept a reconstructed caller budget.
        let root =
            crate::runtime::services::enrollment_window::EnrollmentExecutionRoot::approved_signing(
                self.effects.clone(),
                approval,
                &actor_group,
            )
            .await?;

        let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
        let retained =
            crate::runtime::services::enrollment_window::OriginalSigningTaskCustody::admit(
                root,
                service,
                ready_sender,
            );
        let peer_continuation = retained.peer_continuation();
        let readiness_window = retained.peer_original_window();
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let task = actor_group.spawn_original_local_required_custody(
                    "enrollment.approved-manifest-participant".into(), Box::new(retained), None,
                ).map_err(|rejected| retain_rejected_signing_custody(rejected.into_local()))?;
            } else {
                let task = actor_group.spawn_original_required_custody(
                    "enrollment.approved-manifest-participant".into(), Box::new(retained), None,
                ).map_err(|rejected| retain_rejected_signing_custody(rejected.into_native()))?;
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
            peer_continuation,
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

#[derive(thiserror::Error)]
#[error("original approved signing custody admission failed")]
struct RejectedSigningTaskCustody<T: ?Sized + Send> {
    #[source]
    source: AuraError,
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    retained: parking_lot::Mutex<Box<T>>,
}
impl<T: ?Sized + Send> std::fmt::Debug for RejectedSigningTaskCustody<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RejectedSigningTaskCustody")
    }
}
#[allow(
    clippy::disallowed_types,
    reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
)]
fn retain_rejected_signing_custody<T: ?Sized + Send + 'static>(
    rejected: aura_core::effects::task::RejectedRequiredTask<T>,
) -> AuraError {
    AuraError::Internal {
        message: "admit original approved signing custody".into(),
        source: Some(Arc::new(RejectedSigningTaskCustody {
            source: rejected.source,
            retained: parking_lot::Mutex::new(rejected.obligation),
        })),
    }
}

pub(super) struct RetiredOriginalSigningMaterial {
    effects: Arc<AuraEffectSystem>,
    approved_intent_digest: [u8; 32],
    device: DeviceId,
}
impl RetiredOriginalSigningMaterial {
    pub(super) fn require_original(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
            || self.device != approval.effects().device_id()
        {
            return Err(AuraError::permission_denied(
                "original signing material disposal owner mismatch",
            ));
        }
        Ok(())
    }
}
impl ValidatedLocalEnrollmentSigningMaterial {
    pub(super) fn retire_original(
        mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<RetiredOriginalSigningMaterial, Box<(Self, AuraError)>> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
        {
            return Err(Box::new((
                self,
                AuraError::permission_denied("original signing material retirement owner mismatch"),
            )));
        }
        if let Err(source) = self
            .custody
            .require_manifest(self.effects.as_ref(), approval.manifest())
        {
            return Err(Box::new((self, source)));
        }
        use zeroize::Zeroize;
        self.local_share.zeroize();
        let actual = RetiredOriginalSigningMaterial {
            effects: Arc::clone(&self.effects),
            approved_intent_digest: self.approved_intent_digest,
            device: self.device,
        };
        drop(self);
        Ok(actual)
    }
}

#[cfg(test)]
impl ThresholdSigningService {
    pub(crate) async fn original_prepared_issuer_probe_for_test(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<enrollment_quorum_registry::OriginalPreparedIssuerProbe, AuraError> {
        Ok(self
            .shared
            .quorum
            .take_for_original_approval(approval)
            .await?
            .probe_for_test())
    }
    pub(crate) async fn launch_original_prepared_probe_for_test(
        &self,
        agent: Arc<crate::core::AuraAgent>,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<enrollment_quorum_registry::OriginalPreparedIssuerProbe, AuraError> {
        Ok(self.launch_original_issuer(agent, "Original allocation custody".into(), setup,
            crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode::UserApprovedQuorum).await?.probe_for_test())
    }
    pub(crate) async fn launch_original_single_signer_probe_for_test(
        &self,
        agent: Arc<crate::core::AuraAgent>,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<enrollment_quorum_registry::OriginalPreparedIssuerProbe, AuraError> {
        Ok(self
            .launch_original_issuer(
                agent,
                "Original single-signer custody".into(),
                setup,
                crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode::SingleSigner,
            )
            .await?
            .probe_for_test())
    }
    pub(crate) async fn cancel_original_prepared_for_test(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        self.shared
            .quorum
            .take_for_original_approval(approval)
            .await?
            .cancel_and_drain()
            .await
    }
}

/// Finite old-policy material retained for the actual committed control only.
/// This custody carries no arbitrary transcript or membership permission.
pub(super) struct OriginalPositiveTerminalSigningMaterial {
    effects: Arc<AuraEffectSystem>,
    approved_intent_digest: [u8; 32],
    device: DeviceId,
    index: u16,
    threshold: u16,
    ordered_devices: Vec<DeviceId>,
    public_package: Vec<u8>,
    local_share: zeroize::Zeroizing<Vec<u8>>,
    original_window: super::enrollment_window::OriginalConditionalSigningWindowIdentity,
    origin: OriginalPositiveTerminalOrigin,
}
enum OriginalPositiveTerminalOrigin {
    Issuer {
        original: std::sync::Weak<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        decision_digest: [u8; 32],
    },
    Peer(Box<OriginalPeerPositiveActivationReceipt>),
}
struct OriginalPeerPositiveActivationReceipt {
    proposal: aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
    commit: aura_sync::protocols::device_epoch_rotation::DeviceEpochCommit,
    acceptance: aura_sync::protocols::device_epoch_rotation::DeviceEpochAcceptance,
}
impl OriginalPositiveTerminalSigningMaterial {
    pub(super) fn retire_original(
        mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        child: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<RetiredOriginalSigningMaterial, Box<(Self, AuraError)>> {
        if let Err(source) = self.require_original(approval, child) {
            return Err(Box::new((self, source)));
        }
        use zeroize::Zeroize;
        self.local_share.zeroize();
        let retired = RetiredOriginalSigningMaterial {
            effects: Arc::clone(&self.effects),
            approved_intent_digest: self.approved_intent_digest,
            device: self.device,
        };
        drop(self);
        Ok(retired)
    }

    pub(super) fn require_original(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        child: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        self.original_window.require_child(child)?;
        let parents = approval.manifest().final_inventory().map_err(|source| {
            AuraError::crypto_with_source(
                "original positive terminal native policy",
                Arc::new(source),
            )
        })?;
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
            || self.device != self.effects.device_id()
            || parents.len() != 1
            || parents[0].public_key_package != self.public_package
            || self
                .ordered_devices
                .get(usize::from(self.index.saturating_sub(1)))
                != Some(&self.device)
            || self.local_share.is_empty()
            || self.threshold < 2
        {
            return Err(AuraError::permission_denied(
                "original positive terminal material differs from retained native owner",
            ));
        }
        if let OriginalPositiveTerminalOrigin::Peer(actual) = &self.origin {
            let hash = aura_sync::protocols::device_epoch_rotation::device_epoch_proposal_hash(
                &actual.proposal,
            )?;
            if actual.proposal.subject_authority != approval.manifest().subject
                || actual.proposal.ceremony_id != approval.manifest().ceremony
                || actual.proposal.pending_epoch != approval.manifest().pending_epoch
                || actual.proposal.participant_device_id != self.device
                || actual.acceptance.proposal_hash != hash
                || actual.commit.proposal_hash != hash
                || actual.commit.new_epoch != approval.manifest().pending_epoch
            {
                return Err(AuraError::permission_denied(
                    "positive terminal material lacks its actual peer activation",
                ));
            }
        }
        Ok(())
    }
}

/// The same issuer actors, conditional material and original signing sessions.
/// Physical native guard disposal is separate from final share retirement.
pub(crate) struct OriginalIssuerTranscriptSigningCustody {
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    material: Option<ValidatedLocalEnrollmentSigningMaterial>,
    negative: Option<OriginalNegativeTerminalSigningMaterial>,
    material_retirement: Option<RetiredOriginalSigningMaterial>,
    completion: Arc<OriginalParticipantCompletionCustody>,
    local_actor: Option<enrollment_transcript_signing::OriginalEnrollmentTranscriptActor>,
    remote_actors: Vec<enrollment_transcript_proxy::OriginalEnrollmentTranscriptProxy>,
    ingresses: Vec<enrollment_transcript_signing::EnrollmentTranscriptParticipantIngress>,
    device_quorum_sessions: Vec<device_quorum::OriginalDeviceQuorumSigningSession>,
    positive_control: positive_terminal::OriginalPositiveControlState,
    positive_terminal: Option<OriginalPositiveTerminalSigningMaterial>,
    positive_cleanup_retirement: Option<RetiredOriginalSigningMaterial>,
    device_quorum_failure: Option<AuraError>,
    retirement_calls: Vec<enrollment_transcript_signing::RetainedParticipantCall<()>>,
    retirement_started: bool,
    retirement_completed: bool,
    retirement_operation: Option<Result<(), AuraError>>,
    retirement_validation:
        Option<Result<aura_core::time::PhysicalTime, aura_core::TimeoutBudgetError>>,
    retirement_first_failure: Option<AuraError>,
    terminal_request: Option<Arc<OriginalNegativeTerminalRequest>>,
    terminal_started: bool,
    terminal_signature: Option<Vec<u8>>,
    terminal_operation: Option<Result<(), AuraError>>,
    terminal_validation:
        Option<Result<aura_core::time::PhysicalTime, aura_core::TimeoutBudgetError>>,
    terminal_first_failure: Option<AuraError>,
}
impl OriginalIssuerTranscriptSigningCustody {
    fn new(
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
    ) -> Self {
        Self {
            approval,
            material: None,
            negative: None,
            material_retirement: None,
            completion: Arc::new(OriginalParticipantCompletionCustody::fresh()),
            local_actor: None,
            remote_actors: Vec::new(),
            ingresses: Vec::new(),
            device_quorum_sessions: Vec::new(),
            positive_control: positive_terminal::OriginalPositiveControlState::default(),
            positive_terminal: None,
            positive_cleanup_retirement: None,
            device_quorum_failure: None,
            retirement_calls: Vec::new(),
            retirement_started: false,
            retirement_completed: false,
            retirement_operation: None,
            retirement_validation: None,
            retirement_first_failure: None,
            terminal_request: None,
            terminal_started: false,
            terminal_signature: None,
            terminal_operation: None,
            terminal_validation: None,
            terminal_first_failure: None,
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "EnrollmentActivationCapability", receiver_type = OriginalIssuerTranscriptSigningCustody, family = "runtime_helper")]
    pub(crate) fn retire_original_conditional_for_activation<'a>(
        &'a mut self,
        issued: &'a crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &'a super::enrollment_window::EnrollmentExecutionChild,
        activation: &'a super::ceremony_tracker::EnrollmentActivationCapability<'a>,
    ) -> EnrollmentOperationFuture<'a, Result<(), AuraError>> {
        Box::pin(async move {
            issued.require_runtime_owner(self.approval.effects().as_ref())?;
            child.require_issued_notice_owner(issued)?;
            activation.require_effects(self.approval.effects().as_ref())?;
            if activation.ceremony_id() != &self.approval.manifest().ceremony {
                return Err(AuraError::permission_denied(
                    "conditional retirement activation differs from original ceremony",
                ));
            }
            activation
                .require_generation(
                    self.approval.manifest().subject,
                    self.approval.manifest().pending_epoch,
                )
                .await?;
            if let Some(source) = &self.retirement_first_failure {
                return Err(source.clone());
            }
            if self.retirement_completed {
                return self
                    .material_retirement
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("original material retirement absent"))?
                    .require_original(&self.approval);
            }
            self.require_original_issued_control(issued, child)?;
            if self.terminal_request.is_some() || self.terminal_started || self.retirement_started {
                return Err(AuraError::permission_denied(
                    "original conditional retirement is occupied or unresolved",
                ));
            }
            if self.retirement_calls.is_empty() {
                self.retirement_calls = self
                    .ingresses
                    .iter()
                    .map(|_| enrollment_transcript_signing::RetainedParticipantCall::retire())
                    .collect();
            }
            self.retirement_started = true;
            let material = self
                .negative
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original conditional material absent"))?;
            let local = self
                .local_actor
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original conditional actor absent"))?;
            let remotes = &mut self.remote_actors;
            let calls = &mut self.retirement_calls;
            let ingresses = &self.ingresses;
            let approval = &self.approval;
            let observed = child
                .execute_original_negative_terminal(material, approval, || async {
                    let admit = async {
                        futures::future::try_join_all(
                            calls
                                .iter_mut()
                                .zip(ingresses)
                                .map(|(call, ingress)| call.admit(ingress)),
                        )
                        .await?;
                        Ok::<(), AuraError>(())
                    };
                    let actor = async {
                        match local.run_negative(material, approval, child).await? {
                            OriginalConditionalActorExit::PositiveRetirement => Ok(()),
                            _ => Err(AuraError::permission_denied(
                                "negative actor cannot acknowledge positive retirement",
                            )),
                        }
                    };
                    let proxies = async {
                        futures::future::try_join_all(remotes.iter_mut().map(|proxy| {
                            proxy.run_retirement(material, approval, child, activation)
                        }))
                        .await?;
                        Ok::<(), AuraError>(())
                    };
                    futures::try_join!(admit, actor, proxies)?;
                    Ok(())
                })
                .await?;
            self.retirement_operation = observed.operation;
            self.retirement_validation = Some(observed.validation);
            let settled = match (
                &self.retirement_operation,
                self.retirement_validation.as_ref(),
            ) {
                (Some(Ok(())), Some(Ok(_))) => Ok(()),
                (Some(Err(source)), _) => Err(source.clone()),
                (_, Some(Err(source))) => Err(source.clone().into()),
                _ => Err(AuraError::permission_denied(
                    "original conditional retirement remains unresolved",
                )),
            };
            if let Err(source) = settled {
                self.retirement_first_failure = Some(source.clone());
                return Err(source);
            }
            let actual = self
                .negative
                .take()
                .ok_or_else(|| AuraError::invalid("original conditional material absent"))?;
            match actual.retire_original(&self.approval) {
                Ok(actual) => self.material_retirement = Some(actual),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.negative = Some(actual);
                    self.retirement_first_failure = Some(source.clone());
                    return Err(source);
                }
            }
            let result = (|| {
                let local = self
                    .local_actor
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original local actor absent"))?;
                local.acknowledge_conditional_material_retirement(
                    self.material_retirement
                        .as_ref()
                        .ok_or_else(|| AuraError::invalid("original material retirement absent"))?,
                    &self.approval,
                )?;
                for call in &mut self.retirement_calls {
                    call.observe_ready_reply()?;
                }
                Ok::<(), AuraError>(())
            })();
            if let Err(source) = result {
                self.retirement_first_failure = Some(source.clone());
                return Err(source);
            }
            self.retirement_completed = true;
            Ok(())
        })
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = OriginalIssuerTranscriptSigningCustody, family = "runtime_helper")]
    pub(crate) async fn sign_original_device_quorum(
        &mut self,
        service: &ThresholdSigningService,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
        context: SigningContext,
    ) -> Result<ThresholdSignature, AuraError> {
        self.require_original_issued_control(issued, child)?;
        if !Arc::ptr_eq(&service.effects, self.approval.effects())
            || context.authority != self.approval.manifest().subject
        {
            return Err(AuraError::permission_denied(
                "quorum session differs from original enrollment runtime",
            ));
        }
        if let Some(source) = &self.device_quorum_failure {
            return Err(source.clone());
        }
        let index = match self
            .device_quorum_sessions
            .iter()
            .position(|session| session.require_context(&context).is_ok())
        {
            Some(index) => index,
            None => {
                if self.device_quorum_sessions.len() >= 64 {
                    return Err(AuraError::permission_denied(
                        "original enrollment quorum session capacity",
                    ));
                }
                self.device_quorum_sessions.push(
                    device_quorum::OriginalDeviceQuorumSigningSession::new(context),
                );
                self.device_quorum_sessions.len() - 1
            }
        };
        let session = &mut self.device_quorum_sessions[index];
        let result = child
            .execute(service.effects.as_ref(), || session.run(service))
            .await
            .map_err(|source| AuraError::Internal {
                message: "original enrollment device quorum execution".into(),
                source: Some(Arc::new(source)),
            });
        if let Err(source) = &result {
            self.device_quorum_failure = Some(source.clone());
        }
        result
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalEnrollmentPositiveDecisionReceipt", receiver_type = OriginalIssuerTranscriptSigningCustody, family = "runtime_helper")]
    pub(crate) fn retain_original_positive_terminal(
        &mut self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
        receipt: &crate::runtime::services::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt,
    ) -> Result<(), AuraError> {
        self.require_original_issued_control(issued, child)?;
        receipt.require_original_control(issued)?;
        if self.positive_terminal.is_some() {
            return Ok(());
        }
        let fence = receipt
            .committed_ops()
            .last()
            .ok_or_else(|| AuraError::invalid("actual original committed fence absent"))?;
        let context =
            SigningContext::self_tree_op(self.approval.manifest().subject, fence.op.clone());
        let session = self
            .device_quorum_sessions
            .iter_mut()
            .find(|session| session.require_context(&context).is_ok())
            .ok_or_else(|| {
                AuraError::permission_denied("actual original completed fence quorum owner absent")
            })?;
        self.positive_terminal = Some(session.take_original_positive_terminal(
            &self.approval,
            issued,
            child,
            receipt,
        )?);
        Ok(())
    }

    pub(crate) fn require_initial_transfer(&self) -> Result<(), AuraError> {
        self.negative
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original conditional material transition missing"))?
            .require_original(&self.approval)
    }
    pub(crate) fn require_original_issued_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        issued.require_runtime_owner(self.approval.effects().as_ref())?;
        use aura_signature::SecurityTranscript;
        if issued
            .manifest()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode original conditional issued control",
                    Arc::new(source),
                )
            })?
            != self
                .approval
                .manifest()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode original conditional approved manifest",
                        Arc::new(source),
                    )
                })?
        {
            return Err(AuraError::permission_denied(
                "conditional signing control differs from original approval",
            ));
        }
        if let Some(material) = self.negative.as_ref() {
            material.require_original(&self.approval)?;
            material.require_original_window(child)
        } else {
            self.material_retirement
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original conditional material missing"))?
                .require_original(&self.approval)
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "original_terminal_issuer_signing_resources", receiver_type = OriginalIssuerTranscriptSigningCustody, family = "runtime_helper")]
    pub(crate) fn retire_original_issuer_resources(
        &mut self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
        decision: super::enrollment_window::OriginalIssuerResourceDecision,
    ) -> Result<OriginalIssuerSigningResourceRetirement, AuraError> {
        self.require_original_issued_control(issued, child)?;
        decision.require_original(issued, child)?;
        if matches!(
            decision,
            super::enrollment_window::OriginalIssuerResourceDecision::VerifiedRejected
        ) {
            self.retire_original_positive_for_cleanup(child)?;
            self.retire_original_for_cleanup()?;
        }
        if self.negative.is_some() || self.material.is_some() {
            return Err(AuraError::permission_denied(
                "committed resource cleanup still owns initial signing material",
            ));
        }
        self.material_retirement
            .as_ref()
            .ok_or_else(|| AuraError::invalid("committed conditional material retirement missing"))?
            .require_original(&self.approval)?;
        let identity = child.retain_original_conditional_identity(&self.approval)?;
        self.retire_original_positive_for_cleanup(child)?;
        Ok(OriginalIssuerSigningResourceRetirement {
            approval: Arc::clone(&self.approval),
            identity,
        })
    }
    pub(crate) fn retire_original_positive_for_cleanup(
        &mut self,
        child: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if let Some(actor) = self.local_actor.as_mut() {
            actor.dispose_original_positive_nonces();
        }
        for session in &mut self.device_quorum_sessions {
            session.dispose_local_secrets();
        }
        if let Some(actual) = &self.positive_cleanup_retirement {
            actual.require_original(&self.approval)?;
            if self.positive_terminal.is_some() {
                return Err(AuraError::permission_denied(
                    "issuer positive owner reappeared after cleanup retirement",
                ));
            }
            return Ok(());
        }
        if let Some(actual) = self.positive_terminal.take() {
            match actual.retire_original(&self.approval, child) {
                Ok(actual) => self.positive_cleanup_retirement = Some(actual),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.positive_terminal = Some(actual);
                    return Err(source);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn retire_original_for_cleanup(&mut self) -> Result<(), AuraError> {
        if self.positive_terminal.is_some() {
            return Err(AuraError::permission_denied(
                "positive terminal cleanup requires original registered child",
            ));
        }
        for session in &mut self.device_quorum_sessions {
            session.dispose_local_secrets();
        }
        if let Some(actual) = self.material.take() {
            match actual.retire_original(&self.approval) {
                Ok(actual) => self.material_retirement = Some(actual),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.material = Some(actual);
                    return Err(source);
                }
            }
        }
        if let Some(actual) = self.negative.take() {
            match actual.retire_original(&self.approval) {
                Ok(actual) => self.material_retirement = Some(actual),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.negative = Some(actual);
                    return Err(source);
                }
            }
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalEnrollmentNegativeDecisionReceipt",
        receiver_type = OriginalIssuerTranscriptSigningCustody,
        family = "runtime_helper"
    )]
    pub(crate) fn sign_original_negative_terminal<'a>(
        &'a mut self,
        issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        child: &'a super::enrollment_window::EnrollmentExecutionChild,
        decision: &'a super::ceremony_tracker::OriginalEnrollmentNegativeDecisionReceipt,
    ) -> EnrollmentOperationFuture<'a, Result<Vec<u8>, AuraError>> {
        Box::pin(async move {
            self.require_original_issued_control(&issued, child)?;
            decision.require_original_control(&issued)?;
            if let Some(source) = &self.terminal_first_failure {
                return Err(source.clone());
            }
            if self.terminal_request.is_none() {
                self.terminal_request = Some(Arc::new(
                    OriginalNegativeTerminalRequest::from_original_decision(
                        Arc::clone(&self.approval),
                        issued,
                        decision,
                    )?,
                ));
            }
            let request = self
                .terminal_request
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original terminal request missing"))?;
            request.require_original_decision()?;
            if request.kind != decision.kind()
                || request.decision_digest != decision.decision_digest()
            {
                return Err(AuraError::permission_denied(
                    "original terminal decision changed",
                ));
            }
            if self.terminal_started {
                if matches!(&self.terminal_operation, Some(Ok(())))
                    && matches!(&self.terminal_validation, Some(Ok(_)))
                {
                    return self
                        .terminal_signature
                        .clone()
                        .ok_or_else(|| AuraError::invalid("original terminal signature missing"));
                }
                return Err(AuraError::permission_denied(
                    "original terminal signing remains unresolved",
                ));
            }
            let material = self
                .negative
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original conditional material missing"))?;
            request.require_material(material)?;
            let participants = material
                .ordered_devices
                .iter()
                .enumerate()
                .map(|(offset, device)| {
                    u16::try_from(offset + 1)
                        .map(|index| (*device, index))
                        .map_err(|source| {
                            AuraError::crypto_with_source(
                                "original negative roster index",
                                Arc::new(source),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let key = aura_core::TrustedPublicKey::active(
                aura_core::TrustedKeyDomain::AuthorityThreshold,
                Some(self.approval.manifest().final_epoch),
                material.verifying_key.clone(),
                aura_core::Hash32(aura_core::hash::hash(&material.verifying_key)),
            );
            let policy =
                aura_protocol::public_transcript_signing::PublicTranscriptSigningPolicy::checked(
                    &request.transcript,
                    &material.public_package,
                    &key,
                    material.threshold,
                    &participants,
                )?;
            self.terminal_started = true;
            let signature_slot = &mut self.terminal_signature;
            let ingresses = &self.ingresses;
            let local = self
                .local_actor
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original local actor missing"))?;
            let remotes = &mut self.remote_actors;
            let approval = &self.approval;
            let observed =
                child
                    .execute_original_negative_terminal(material, approval, || async {
                        let coordinate =
                            async {
                                futures::future::try_join_all(ingresses.iter().map(|ingress| {
                                    ingress.begin_negative_terminal(Arc::clone(request))
                                }))
                                .await?;
                                let signature =
                        aura_protocol::public_transcript_signing::sign_public_transcript(
                            material.effects.as_ref(),
                            policy,
                            |device, index| async move {
                                ingresses
                                    .iter()
                                    .find(|entry| entry.matches_original_participant(device, index))
                                    .ok_or_else(|| {
                                        AuraError::invalid("original negative participant missing")
                                    })?
                                    .commitment()
                                    .await
                            },
                            |device, index, package| async move {
                                ingresses
                                    .iter()
                                    .find(|entry| entry.matches_original_participant(device, index))
                                    .ok_or_else(|| {
                                        AuraError::invalid("original negative participant missing")
                                    })?
                                    .share(package)
                                    .await
                            },
                        )
                        .await?;
                                *signature_slot = Some(signature);
                                Ok::<(), AuraError>(())
                            };
                        tokio::try_join!(
                            local.run_negative(material, approval, child),
                            futures::future::try_join_all(
                                remotes
                                    .iter_mut()
                                    .map(|proxy| proxy.run_negative(material, approval, child))
                            ),
                            coordinate
                        )?;
                        Ok::<(), AuraError>(())
                    })
                    .await?;
            if let Some(Err(source)) = &observed.operation {
                self.terminal_first_failure
                    .get_or_insert_with(|| source.clone());
            }
            if let Err(source) = &observed.validation {
                self.terminal_first_failure.get_or_insert_with(|| {
                    AuraError::crypto_with_source(
                        "original negative signing validation",
                        Arc::new(source.clone()),
                    )
                });
            }
            self.terminal_operation = observed.operation;
            self.terminal_validation = Some(observed.validation);
            if let Some(source) = &self.terminal_first_failure {
                return Err(source.clone());
            }
            if !matches!(self.terminal_operation, Some(Ok(()))) {
                return Err(AuraError::invalid("original negative signing interrupted"));
            }
            self.terminal_signature
                .clone()
                .ok_or_else(|| AuraError::invalid("original terminal signature missing"))
        })
    }

    pub(crate) fn require_original_terminal_message(
        &self,
        message: &[u8],
    ) -> Result<(), AuraError> {
        let request = self
            .terminal_request
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original terminal request missing"))?;
        if request.message()? != message {
            return Err(AuraError::permission_denied(
                "terminal frame differs from actual approved negative request",
            ));
        }
        Ok(())
    }
}
pub(super) struct OriginalNegativeTerminalSigningMaterial {
    effects: Arc<AuraEffectSystem>,
    device: DeviceId,
    approved_intent_digest: [u8; 32],
    index: u16,
    threshold: u16,
    ordered_devices: Vec<DeviceId>,
    public_package: Vec<u8>,
    verifying_key: Vec<u8>,
    local_share: zeroize::Zeroizing<Vec<u8>>,
    negative_terminal_policy:
        aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalPolicy,
    completion: Arc<OriginalParticipantCompletionCustody>,
    original_window: super::enrollment_window::OriginalConditionalSigningWindowIdentity,
}

pub(crate) use enrollment_transcript_wire::OriginalConditionalPacket;

/// Borrowed transport execution rooted in the actual transferred share owner.
/// A missing request permits only pre-decision observation, never emission.
pub(crate) struct OriginalConditionalTransport<'owner> {
    material: &'owner OriginalNegativeTerminalSigningMaterial,
    approval: &'owner Arc<
        crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    >,
    child: &'owner super::enrollment_window::EnrollmentExecutionChild,
    request: Option<&'owner enrollment_transcript_wire::AdmittedNegativeTerminalRequest>,
    readiness: Option<&'owner crate::runtime::effects::AcknowledgedApprovedNativeDisposal>,
}
impl OriginalNegativeTerminalSigningMaterial {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalNegativeTerminalSigningMaterial", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    fn conditional_transport<'owner>(
        &'owner self,
        approval: &'owner Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &'owner super::enrollment_window::EnrollmentExecutionChild,
        request: Option<&'owner enrollment_transcript_wire::AdmittedNegativeTerminalRequest>,
    ) -> Result<OriginalConditionalTransport<'owner>, AuraError> {
        self.require_original(approval)?;
        self.require_original_window(child)?;
        if let Some(request) = request {
            request.require_material(self)?;
        }
        Ok(OriginalConditionalTransport {
            material: self,
            approval,
            child,
            request,
            readiness: None,
        })
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "AcknowledgedApprovedNativeDisposal", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    pub(super) fn readiness_transport<'owner>(
        &'owner self,
        approval: &'owner Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &'owner super::enrollment_window::EnrollmentExecutionChild,
        disposal: &'owner crate::runtime::effects::AcknowledgedApprovedNativeDisposal,
    ) -> Result<OriginalConditionalTransport<'owner>, AuraError> {
        disposal.require_original(approval)?;
        if let Some(source) = disposal.original_failure() {
            return Err(source.clone());
        }
        let mut actual = self.conditional_transport(approval, child, None)?;
        actual.readiness = Some(disposal);
        Ok(actual)
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "AcknowledgedApprovedNativeDisposal", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    pub(super) async fn produce_original_conditional_readiness(
        &self,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::enrollment_window::EnrollmentExecutionChild,
        disposal: &crate::runtime::effects::AcknowledgedApprovedNativeDisposal,
        retained: &mut Option<
            aura_protocol::transcript_round_packet::ParticipantProvenConditionalReadiness,
        >,
    ) -> Result<(), AuraError> {
        use aura_signature::SecurityTranscript;
        let owner = self.readiness_transport(approval, child, disposal)?;
        let manifest = approval.manifest();
        if self.device == manifest.initiator_device {
            return Err(AuraError::permission_denied(
                "native participant readiness cannot replace coordinator reservation disposition",
            ));
        }
        let statement = aura_protocol::transcript_round_packet::ConditionalReadinessAttestation {
            version: 1,
            approved_intent_digest: approval.canonical_intent_digest(),
            manifest_digest: aura_core::hash::hash(&manifest.required_transcript_bytes().map_err(
                |source| {
                    AuraError::crypto_with_source(
                        "encode original readiness manifest",
                        Arc::new(source),
                    )
                },
            )?),
            coordinator: manifest.initiator_device,
            participant: self.device,
            locator: owner.receive_locator(manifest.initiator_device)?,
        };
        let expected = enrollment_transcript_wire::original_participant_verifier(
            &self.ordered_devices,
            &self.public_package,
            self.device,
        )?;
        if let Some(actual) = retained.as_ref() {
            if aura_core::util::serialization::to_vec(&actual.statement)?
                != aura_core::util::serialization::to_vec(&statement)?
            {
                return Err(AuraError::permission_denied(
                    "retained readiness differs from original native participant",
                ));
            }
            return actual.verify_exact_participant(&expected);
        }
        let bytes = statement.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode original native readiness", Arc::new(source))
        })?;
        let possession_proof = self
            .effects
            .sign_participant_key_proof(&bytes, &self.local_share, SigningMode::Threshold)
            .await?;
        let actual =
            aura_protocol::transcript_round_packet::ParticipantProvenConditionalReadiness {
                statement,
                possession_proof,
            };
        actual.verify_exact_participant(&expected)?;
        *retained = Some(actual);
        Ok(())
    }
}
impl<'material> OriginalConditionalTransport<'material> {
    pub(crate) fn original_effects_origin(&self) -> std::sync::Weak<AuraEffectSystem> {
        Arc::downgrade(self.approval.effects())
    }
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        if !std::ptr::eq(self.material.effects.as_ref(), effects) {
            return Err(AuraError::permission_denied(
                "conditional transport belongs to another runtime",
            ));
        }
        self.material.require_original(self.approval)?;
        self.material.require_original_window(self.child)
    }
    pub(crate) fn receive_locator(&self, peer: DeviceId) -> Result<[u8; 32], AuraError> {
        self.require_effects(self.material.effects.as_ref())?;
        let manifest = self.approval.manifest();
        let coordinator = manifest.initiator_device;
        if peer == self.material.device || !self.material.ordered_devices.contains(&peer) {
            return Err(AuraError::permission_denied(
                "conditional transport peer differs from retained roster",
            ));
        }
        let participant = if self.material.device == coordinator {
            peer
        } else {
            if peer != coordinator {
                return Err(AuraError::permission_denied(
                    "conditional participant route lacks original coordinator",
                ));
            }
            self.material.device
        };
        Ok(aura_core::hash::hash(
            &aura_core::util::serialization::to_vec(&(
                "aura.enrollment.conditional-transport-locator.v1",
                self.approval.canonical_intent_digest(),
                manifest.subject,
                &manifest.ceremony,
                &manifest.invitation,
                coordinator,
                participant,
            ))?,
        ))
    }
    pub(crate) fn validate_send(
        &self,
        destination: DeviceId,
        packet: &OriginalConditionalPacket,
    ) -> Result<(AuthorityId, [u8; 32], Vec<u8>), AuraError> {
        let locator = self.receive_locator(destination)?;
        match packet {
            OriginalConditionalPacket::Ready(actual) => {
                use aura_signature::SecurityTranscript;
                let disposal = self.readiness.ok_or_else(|| {
                    AuraError::permission_denied("readiness send lacks actual native disposal")
                })?;
                disposal.require_original(self.approval)?;
                if let Some(source) = disposal.original_failure() {
                    return Err(source.clone());
                }
                let manifest = self.approval.manifest();
                let statement = &actual.statement;
                if destination != manifest.initiator_device
                    || self.material.device == destination
                    || statement.version != 1
                    || statement.approved_intent_digest != self.approval.canonical_intent_digest()
                    || statement.manifest_digest
                        != aura_core::hash::hash(&manifest.required_transcript_bytes().map_err(
                            |source| {
                                AuraError::crypto_with_source(
                                    "encode original readiness binding",
                                    Arc::new(source),
                                )
                            },
                        )?)
                    || statement.coordinator != destination
                    || statement.participant != self.material.device
                    || statement.locator != locator
                {
                    return Err(AuraError::permission_denied(
                        "readiness packet differs from actual disposed original participant",
                    ));
                }
                let expected = enrollment_transcript_wire::original_participant_verifier(
                    &self.material.ordered_devices,
                    &self.material.public_package,
                    self.material.device,
                )?;
                actual.verify_exact_participant(&expected)?;
            }
            _ => self
                .request
                .ok_or_else(|| {
                    AuraError::permission_denied("conditional send lacks actual admitted decision")
                })?
                .require_packet(self.material, packet, destination)?,
        }
        let bytes = aura_core::util::serialization::to_vec(packet)?;
        if bytes.len() > aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES {
            return Err(AuraError::invalid("conditional packet exceeds original wire bound"));
        }
        Ok((self.approval.manifest().subject, locator, bytes))
    }
    /// Decode bounded canonical data. Remote decision admission remains separate.
    pub(crate) fn decode_received(
        &self,
        bytes: &[u8],
    ) -> Result<OriginalConditionalPacket, AuraError> {
        self.require_effects(self.material.effects.as_ref())?;
        if bytes.len() > aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES {
            return Err(AuraError::invalid("conditional packet exceeds original wire bound"));
        }
        let packet: OriginalConditionalPacket = aura_core::util::serialization::from_slice(bytes)?;
        if aura_core::util::serialization::to_vec(&packet)? != bytes {
            return Err(AuraError::invalid("conditional packet is not canonical"));
        }
        match &packet {
            OriginalConditionalPacket::RetirementRequest(actual) => {
                aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest::decode_bounded(&aura_core::util::serialization::to_vec(actual)?)?;
            }
            OriginalConditionalPacket::RetirementAcknowledgment(actual) => {
                aura_protocol::transcript_round_packet::ParticipantProvenConditionalRetirementAcknowledgment::decode_bounded(&aura_core::util::serialization::to_vec(actual)?)?;
            }
            OriginalConditionalPacket::Ready(actual) => {
                aura_protocol::transcript_round_packet::ParticipantProvenConditionalReadiness::decode_bounded(&aura_core::util::serialization::to_vec(actual)?)?;
            }
            OriginalConditionalPacket::Decision(actual) => {
                aura_protocol::transcript_round_packet::CoordinatorProvenNegativeDecision::decode_bounded(&aura_core::util::serialization::to_vec(actual)?)?;
            }
            OriginalConditionalPacket::Round(actual) => {
                aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket::decode_bounded(&aura_core::util::serialization::to_vec(actual)?)?;
            }
        }
        Ok(packet)
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalConditionalTransport", receiver_type = OriginalConditionalTransport, family = "runtime_helper")]
    pub(crate) async fn execute<'owner, T, F, Fut>(
        &'owner self,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &'owner OriginalConditionalTransport<'material>,
            T,
            AuraError,
        >,
        AuraError,
    >
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        let actual = self
            .child
            .execute_original_negative_terminal(self.material, self.approval, operation)
            .await?;
        Ok(aura_core::time::timeout::NegativeCleanupExecution {
            primary: self,
            operation: actual.operation,
            validation: actual.validation,
        })
    }
}
impl ValidatedLocalEnrollmentSigningMaterial {
    pub(super) fn transfer_original_negative(
        self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        completion: Arc<OriginalParticipantCompletionCustody>,
        original_window: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<OriginalNegativeTerminalSigningMaterial, Box<(Self, AuraError)>> {
        let validation = (|| {
            self.custody
                .require_manifest(self.effects.as_ref(), approval.manifest())?;
            completion.original()?.require_original(approval)?;
            approval
                .negative_terminal_policy()
                .require_manifest(approval.manifest())?;
            if !Arc::ptr_eq(&self.effects, approval.effects())
                || self.approved_intent_digest != approval.canonical_intent_digest()
                || self.device != approval.effects().device_id()
            {
                return Err(AuraError::permission_denied(
                    "original negative material transfer owner mismatch",
                ));
            }
            Ok(())
        })();
        if let Err(source) = validation {
            return Err(Box::new((self, source)));
        }
        let original_window = match original_window.retain_original_conditional_identity(approval) {
            Ok(actual) => actual,
            Err(source) => return Err(Box::new((self, source))),
        };
        let Self {
            effects,
            custody,
            device,
            approved_intent_digest,
            index,
            threshold,
            ordered_devices,
            public_package,
            verifying_key,
            local_share,
        } = self;
        let actual = OriginalNegativeTerminalSigningMaterial {
            effects,
            device,
            approved_intent_digest,
            index,
            threshold,
            ordered_devices,
            public_package,
            verifying_key,
            local_share,
            negative_terminal_policy: approval.negative_terminal_policy().clone(),
            completion,
            original_window,
        };
        drop(custody);
        Ok(actual)
    }
}
impl OriginalNegativeTerminalSigningMaterial {
    pub(super) fn require_original(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
            || self.device != approval.effects().device_id()
            || self.negative_terminal_policy != *approval.negative_terminal_policy()
        {
            return Err(AuraError::permission_denied(
                "original negative signing material owner mismatch",
            ));
        }
        self.completion.original()?.require_original(approval)
    }
    pub(super) fn require_original_window(
        &self,
        child: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        self.original_window.require_child(child)
    }
    pub(super) fn retire_original(
        mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<RetiredOriginalSigningMaterial, Box<(Self, AuraError)>> {
        if let Err(source) = self.require_original(approval) {
            return Err(Box::new((self, source)));
        }
        use zeroize::Zeroize;
        self.local_share.zeroize();
        Ok(RetiredOriginalSigningMaterial {
            effects: Arc::clone(&self.effects),
            approved_intent_digest: self.approved_intent_digest,
            device: self.device,
        })
    }
}

/// A locally admitted conditional request from the actual protected first decision.
/// Public transcript shape alone cannot construct this signing authority.
pub(super) struct OriginalNegativeTerminalRequest {
    original: std::sync::Weak<super::ceremony_tracker::RegisteredEnrollmentWindowCapability>,
    issued:
        std::sync::Weak<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    effects: Arc<AuraEffectSystem>,
    approved_intent_digest: [u8; 32],
    policy: aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalPolicy,
    kind: aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind,
    decision_digest: [u8; 32],
    transcript: aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalTranscript,
    message: Vec<u8>,
    retirement_location: SecureStorageLocation,
    approval_digest: [u8; 32],
}
impl OriginalNegativeTerminalRequest {
    fn from_original_decision(
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        decision: &super::ceremony_tracker::OriginalEnrollmentNegativeDecisionReceipt,
    ) -> Result<Self, AuraError> {
        use aura_signature::SecurityTranscript;
        issued.require_runtime_owner(approval.effects().as_ref())?;
        approval
            .negative_terminal_policy()
            .require_manifest(issued.manifest())?;
        if issued
            .manifest()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode actual negative control manifest",
                    Arc::new(source),
                )
            })?
            != approval
                .manifest()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode original approved negative manifest",
                        Arc::new(source),
                    )
                })?
        {
            return Err(AuraError::permission_denied(
                "negative request differs from original approved manifest",
            ));
        }
        let original = decision.original_owner_for_control(&issued)?;
        let kind = decision.kind();
        if !approval
            .negative_terminal_policy()
            .outcomes()
            .contains(&kind)
        {
            return Err(AuraError::permission_denied(
                "original decision is outside approved negative policy",
            ));
        }
        let transcript = aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalTranscript::from_manifest(approval.manifest(), kind)?;
        let message = transcript.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode canonical original negative control",
                Arc::new(source),
            )
        })?;
        let decision_digest = decision.decision_digest();
        let (approval_digest, retirement_location) =
            enrollment_transcript_wire::negative_domain_binding(
                &approval,
                decision_digest,
                kind,
                &message,
            )?;
        Ok(Self {
            original,
            issued: Arc::downgrade(&issued),
            effects: Arc::clone(approval.effects()),
            approved_intent_digest: approval.canonical_intent_digest(),
            policy: approval.negative_terminal_policy().clone(),
            kind,
            decision_digest,
            transcript,
            message,
            retirement_location,
            approval_digest,
            approval,
        })
    }
    fn require_original_decision(&self) -> Result<(), AuraError> {
        let original = self.original.upgrade().ok_or_else(|| {
            AuraError::permission_denied("original negative decision owner was lost")
        })?;
        let issued = self.issued.upgrade().ok_or_else(|| {
            AuraError::permission_denied("original negative control owner was lost")
        })?;
        let retained = original.original_negative_decision(&issued)?;
        if retained.kind() != self.kind || retained.decision_digest() != self.decision_digest {
            return Err(AuraError::permission_denied(
                "negative request original decision changed",
            ));
        }
        issued.require_runtime_owner(self.effects.as_ref())?;
        self.policy.require_manifest(issued.manifest())
    }
    pub(super) fn require_material(
        &self,
        material: &OriginalNegativeTerminalSigningMaterial,
    ) -> Result<(), AuraError> {
        self.require_original_decision()?;
        material.require_original(&self.approval)?;
        if !Arc::ptr_eq(&self.effects, &material.effects)
            || self.approved_intent_digest != material.approved_intent_digest
            || self.policy != material.negative_terminal_policy
        {
            return Err(AuraError::permission_denied(
                "negative request differs from retained signing material",
            ));
        }
        Ok(())
    }
    pub(super) fn message(&self) -> Result<&[u8], AuraError> {
        self.require_original_decision()?;
        Ok(&self.message)
    }
    pub(super) fn retirement_location(&self) -> &SecureStorageLocation {
        &self.retirement_location
    }
    pub(super) fn approval_digest(&self) -> [u8; 32] {
        self.approval_digest
    }
}

/// Scoped observation only: no enrollment authority or synthetic material.
#[cfg(test)]
pub(crate) struct OriginalPeerCommitPostreadTest {
    clock: aura_testkit::time::ManualPhysicalClock,
    armed: std::sync::atomic::AtomicBool,
    armed_once: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    completed: std::sync::atomic::AtomicBool,
    cleanup: bool,
    cleanup_source: Arc<OriginalPeerCleanupCancellation>,
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    disposal: std::sync::Mutex<Option<crate::task_registry::OriginalRequiredTaskDisposalObserver>>,
    cleanup_completed: tokio::sync::Notify,
}
#[cfg(test)]
impl OriginalPeerCommitPostreadTest {
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    pub(crate) fn new(clock: aura_testkit::time::ManualPhysicalClock, cleanup: bool) -> Self {
        Self {
            clock,
            armed: false.into(),
            armed_once: false.into(),
            entered: tokio::sync::Notify::default(),
            release: tokio::sync::Notify::default(),
            completed: false.into(),
            cleanup,
            cleanup_source: Arc::new(OriginalPeerCleanupCancellation),
            disposal: std::sync::Mutex::default(),
            cleanup_completed: tokio::sync::Notify::default(),
        }
    }
    fn arm(&self) {
        if !self
            .armed_once
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    pub(crate) async fn await_cleanup_completed(&self) {
        self.cleanup_completed.notified().await;
    }
    pub(crate) fn assert_completed(&self) {
        assert!(self.completed.load(std::sync::atomic::Ordering::SeqCst));
    }
}
#[cfg(test)]
#[async_trait::async_trait]
impl aura_core::effects::PhysicalTimeEffects for OriginalPeerCommitPostreadTest {
    async fn physical_time(
        &self,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.clock.physical_time().await
    }
    async fn sleep_ms(&self, duration: u64) -> Result<(), aura_core::effects::TimeError> {
        self.clock.sleep_ms(duration).await
    }
    async fn wait_until_physical_deadline(
        &self,
        endpoint: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        self.clock.wait_until_physical_deadline(endpoint).await
    }
}

#[cfg(test)]
#[derive(Debug, thiserror::Error)]
#[error("original peer cleanup injected cancellation")]
struct OriginalPeerCleanupCancellation;

/// Actual old signing-secret retirement, independent of a control signature.
pub(crate) struct OriginalIssuerSigningResourceRetirement {
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    identity: super::enrollment_window::OriginalConditionalSigningWindowIdentity,
}
impl OriginalIssuerSigningResourceRetirement {
    pub(super) fn require_original(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        issued.require_runtime_owner(self.approval.effects().as_ref())?;
        self.identity.require_child(child)?;
        self.approval
            .negative_terminal_policy()
            .require_manifest(issued.manifest())
    }
}
