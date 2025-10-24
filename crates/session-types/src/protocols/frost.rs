//! Session Type States for FROST Cryptographic Protocol
//!
//! This module defines session types for FROST threshold signature operations,
//! providing compile-time safety for the signing protocol state machine.

use crate::{
    ChoreographicProtocol, RuntimeWitness, SessionProtocol, SessionState, WitnessedTransition,
};
use aura_crypto::{CryptoError, FrostKeyShare, SignatureShare, SigningCommitment};
use frost_ed25519 as frost;
use std::collections::BTreeMap;
use std::fmt;
use uuid::Uuid;

// ========== FROST Signing Session States ==========

/// Initial state when FROST signer is created but not yet participating
#[derive(Debug, Clone)]
pub struct FrostIdle;

impl SessionState for FrostIdle {
    const NAME: &'static str = "FrostIdle";
    const CAN_TERMINATE: bool = true;
}

/// State during Round 1: generating nonces and commitments
#[derive(Debug, Clone)]
pub struct FrostCommitmentPhase;

impl SessionState for FrostCommitmentPhase {
    const NAME: &'static str = "FrostCommitmentPhase";
}

/// State when waiting for other participants' commitments
#[derive(Debug, Clone)]
pub struct FrostAwaitingCommitments;

impl SessionState for FrostAwaitingCommitments {
    const NAME: &'static str = "FrostAwaitingCommitments";
}

/// State during Round 2: creating signature shares
#[derive(Debug, Clone)]
pub struct FrostSigningPhase;

impl SessionState for FrostSigningPhase {
    const NAME: &'static str = "FrostSigningPhase";
}

/// State when waiting for other participants' signature shares
#[derive(Debug, Clone)]
pub struct FrostAwaitingShares;

impl SessionState for FrostAwaitingShares {
    const NAME: &'static str = "FrostAwaitingShares";
}

/// State when ready to aggregate signature shares
#[derive(Debug, Clone)]
pub struct FrostReadyToAggregate;

impl SessionState for FrostReadyToAggregate {
    const NAME: &'static str = "FrostReadyToAggregate";
}

/// State when signature has been completed
#[derive(Debug, Clone)]
pub struct FrostSignatureComplete;

impl SessionState for FrostSignatureComplete {
    const NAME: &'static str = "FrostSignatureComplete";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

/// State when signing protocol has failed
#[derive(Debug, Clone)]
pub struct FrostSigningFailed;

impl SessionState for FrostSigningFailed {
    const NAME: &'static str = "FrostSigningFailed";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== Key Generation Session States ==========

/// State during key generation initialization
#[derive(Debug, Clone)]
pub struct KeyGenerationInitializing;

impl SessionState for KeyGenerationInitializing {
    const NAME: &'static str = "KeyGenerationInitializing";
}

/// State during distributed key generation
#[derive(Debug, Clone)]
pub struct KeyGenerationInProgress;

impl SessionState for KeyGenerationInProgress {
    const NAME: &'static str = "KeyGenerationInProgress";
}

/// State when key generation is complete
#[derive(Debug, Clone)]
pub struct KeyGenerationComplete;

impl SessionState for KeyGenerationComplete {
    const NAME: &'static str = "KeyGenerationComplete";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== Resharing Session States ==========

/// State during resharing protocol initialization
#[derive(Debug, Clone)]
pub struct ResharingInitializing;

impl SessionState for ResharingInitializing {
    const NAME: &'static str = "ResharingInitializing";
}

/// State during Phase 1: sub-share distribution
#[derive(Debug, Clone)]
pub struct ResharingPhaseOne;

impl SessionState for ResharingPhaseOne {
    const NAME: &'static str = "ResharingPhaseOne";
}

/// State during Phase 2: share reconstruction
#[derive(Debug, Clone)]
pub struct ResharingPhaseTwo;

impl SessionState for ResharingPhaseTwo {
    const NAME: &'static str = "ResharingPhaseTwo";
}

/// State when resharing is complete
#[derive(Debug, Clone)]
pub struct ResharingComplete;

impl SessionState for ResharingComplete {
    const NAME: &'static str = "ResharingComplete";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== FROST Protocol Wrapper ==========

/// Core FROST protocol data without session state
pub struct FrostProtocolCore {
    pub session_id: Uuid,
    pub device_id: aura_journal::DeviceId,
    pub participant_id: frost::Identifier,
    pub key_share: Option<FrostKeyShare>,
    pub message_to_sign: Option<Vec<u8>>,
    pub signing_nonces: Option<frost::round1::SigningNonces>,
    pub collected_commitments: BTreeMap<frost::Identifier, SigningCommitment>,
    pub collected_shares: BTreeMap<frost::Identifier, SignatureShare>,
    pub threshold: u16,
    pub participant_count: u16,
}

impl FrostProtocolCore {
    pub fn new(
        session_id: Uuid,
        device_id: aura_journal::DeviceId,
        participant_id: frost::Identifier,
        threshold: u16,
        participant_count: u16,
    ) -> Self {
        Self {
            session_id,
            device_id,
            participant_id,
            key_share: None,
            message_to_sign: None,
            signing_nonces: None,
            collected_commitments: BTreeMap::new(),
            collected_shares: BTreeMap::new(),
            threshold,
            participant_count,
        }
    }
}

impl fmt::Debug for FrostProtocolCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrostProtocolCore")
            .field("session_id", &self.session_id)
            .field("device_id", &self.device_id)
            .field("participant_id", &self.participant_id)
            .field("has_key_share", &self.key_share.is_some())
            .field("has_message", &self.message_to_sign.is_some())
            .field(
                "collected_commitments_count",
                &self.collected_commitments.len(),
            )
            .field("collected_shares_count", &self.collected_shares.len())
            .field("threshold", &self.threshold)
            .field("participant_count", &self.participant_count)
            .finish()
    }
}

// Manual Clone implementation for FrostProtocolCore
impl Clone for FrostProtocolCore {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id,
            device_id: self.device_id,
            participant_id: self.participant_id,
            key_share: self.key_share.clone(),
            message_to_sign: self.message_to_sign.clone(),
            // Note: SigningNonces cannot be cloned safely as they contain secret values that should not be reused
            // In a real protocol, nonces are single-use only. For session types, we set to None.
            signing_nonces: None,
            // Clone commitment and share collections with placeholder values for cryptographic data
            collected_commitments: self.collected_commitments.keys().map(|id| {
                (*id, create_placeholder_commitment(*id))
            }).collect(),
            collected_shares: self.collected_shares.keys().map(|id| {
                (*id, create_placeholder_signature_share(*id))
            }).collect(),
            threshold: self.threshold,
            participant_count: self.participant_count,
        }
    }
}

/// Session-typed FROST protocol wrapper
pub type SessionTypedFrost<S> = ChoreographicProtocol<FrostProtocolCore, S>;

// ========== Signing Context Information ==========

/// Context for FROST signing operation
#[derive(Debug, Clone)]
pub struct FrostSigningContext {
    pub session_id: Uuid,
    pub message: Vec<u8>,
    pub participants: Vec<frost::Identifier>,
    pub started_at: u64,
}

/// Commitment collection context
pub struct CommitmentContext {
    pub participant_id: frost::Identifier,
    pub commitment: SigningCommitment,
    pub collected_at: u64,
}

impl std::fmt::Debug for CommitmentContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitmentContext")
            .field("participant_id", &self.participant_id)
            .field("collected_at", &self.collected_at)
            .finish()
    }
}

impl Clone for CommitmentContext {
    fn clone(&self) -> Self {
        // Note: Only clones metadata, not the actual commitment
        CommitmentContext {
            participant_id: self.participant_id,
            commitment: create_placeholder_commitment(self.participant_id),
            collected_at: self.collected_at,
        }
    }
}

/// Signature share context
pub struct SignatureShareContext {
    pub participant_id: frost::Identifier,
    pub share: SignatureShare,
    pub created_at: u64,
}

impl std::fmt::Debug for SignatureShareContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureShareContext")
            .field("participant_id", &self.participant_id)
            .field("created_at", &self.created_at)
            .finish()
    }
}

impl Clone for SignatureShareContext {
    fn clone(&self) -> Self {
        // Note: Only clones metadata, not the actual share
        SignatureShareContext {
            participant_id: self.participant_id,
            share: create_placeholder_signature_share(self.participant_id),
            created_at: self.created_at,
        }
    }
}

// ========== Runtime Witnesses for FROST Operations ==========

/// Witness that sufficient commitments have been collected for threshold
pub struct CommitmentThresholdMet {
    pub session_id: Uuid,
    pub commitment_count: usize,
    pub threshold: u16,
    pub commitments: Vec<SigningCommitment>,
}

impl std::fmt::Debug for CommitmentThresholdMet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitmentThresholdMet")
            .field("session_id", &self.session_id)
            .field("commitment_count", &self.commitment_count)
            .field("threshold", &self.threshold)
            .field("commitments_len", &self.commitments.len())
            .finish()
    }
}

impl Clone for CommitmentThresholdMet {
    fn clone(&self) -> Self {
        CommitmentThresholdMet {
            session_id: self.session_id,
            commitment_count: self.commitment_count,
            threshold: self.threshold,
            commitments: self
                .commitments
                .iter()
                .map(|c| create_placeholder_commitment(c.identifier))
                .collect(),
        }
    }
}

impl RuntimeWitness for CommitmentThresholdMet {
    type Evidence = Vec<SigningCommitment>;
    type Config = (Uuid, u16); // (session_id, threshold)

    fn verify(evidence: Vec<SigningCommitment>, config: (Uuid, u16)) -> Option<Self> {
        let (session_id, threshold) = config;

        if evidence.len() >= threshold as usize {
            Some(CommitmentThresholdMet {
                session_id,
                commitment_count: evidence.len(),
                threshold,
                commitments: evidence,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient FROST commitments collected for threshold"
    }
}

/// Witness that sufficient signature shares have been collected
pub struct SignatureShareThresholdMet {
    pub session_id: Uuid,
    pub share_count: usize,
    pub threshold: u16,
    pub shares: Vec<SignatureShare>,
}

impl std::fmt::Debug for SignatureShareThresholdMet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureShareThresholdMet")
            .field("session_id", &self.session_id)
            .field("share_count", &self.share_count)
            .field("threshold", &self.threshold)
            .field("shares_len", &self.shares.len())
            .finish()
    }
}

impl Clone for SignatureShareThresholdMet {
    fn clone(&self) -> Self {
        SignatureShareThresholdMet {
            session_id: self.session_id,
            share_count: self.share_count,
            threshold: self.threshold,
            shares: self
                .shares
                .iter()
                .map(|s| create_placeholder_signature_share(s.identifier))
                .collect(),
        }
    }
}

impl RuntimeWitness for SignatureShareThresholdMet {
    type Evidence = Vec<SignatureShare>;
    type Config = (Uuid, u16); // (session_id, threshold)

    fn verify(evidence: Vec<SignatureShare>, config: (Uuid, u16)) -> Option<Self> {
        let (session_id, threshold) = config;

        if evidence.len() >= threshold as usize {
            Some(SignatureShareThresholdMet {
                session_id,
                share_count: evidence.len(),
                threshold,
                shares: evidence,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient FROST signature shares collected for threshold"
    }
}

/// Witness that signature aggregation has completed successfully
#[derive(Debug, Clone)]
pub struct SignatureAggregated {
    pub session_id: Uuid,
    pub signature: ed25519_dalek::Signature,
    pub message: Vec<u8>,
    pub aggregated_at: u64,
}

impl RuntimeWitness for SignatureAggregated {
    type Evidence = (ed25519_dalek::Signature, Vec<u8>);
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (ed25519_dalek::Signature, Vec<u8>), config: (Uuid, u64)) -> Option<Self> {
        let (signature, message) = evidence;
        let (session_id, timestamp) = config;

        Some(SignatureAggregated {
            session_id,
            signature,
            message,
            aggregated_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "FROST signature aggregation completed successfully"
    }
}

/// Witness that key generation has completed
#[derive(Clone)]
pub struct KeyGenerationCompleted {
    pub session_id: Uuid,
    pub key_share: FrostKeyShare,
    pub public_key: frost::VerifyingKey,
    pub completed_at: u64,
}

impl std::fmt::Debug for KeyGenerationCompleted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyGenerationCompleted")
            .field("session_id", &self.session_id)
            .field("has_key_share", &true)
            .field("completed_at", &self.completed_at)
            .finish()
    }
}

impl RuntimeWitness for KeyGenerationCompleted {
    type Evidence = (FrostKeyShare, frost::VerifyingKey);
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (FrostKeyShare, frost::VerifyingKey), config: (Uuid, u64)) -> Option<Self> {
        let (key_share, public_key) = evidence;
        let (session_id, timestamp) = config;

        Some(KeyGenerationCompleted {
            session_id,
            key_share,
            public_key,
            completed_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "FROST key generation completed successfully"
    }
}

/// Witness that resharing has completed
#[derive(Clone)]
pub struct FrostResharingCompleted {
    pub session_id: Uuid,
    pub new_key_share: FrostKeyShare,
    pub new_threshold: u16,
    pub completed_at: u64,
}

impl std::fmt::Debug for FrostResharingCompleted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrostResharingCompleted")
            .field("session_id", &self.session_id)
            .field("has_new_key_share", &true)
            .field("new_threshold", &self.new_threshold)
            .field("completed_at", &self.completed_at)
            .finish()
    }
}

impl RuntimeWitness for FrostResharingCompleted {
    type Evidence = (FrostKeyShare, u16);
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (FrostKeyShare, u16), config: (Uuid, u64)) -> Option<Self> {
        let (new_key_share, new_threshold) = evidence;
        let (session_id, timestamp) = config;

        Some(FrostResharingCompleted {
            session_id,
            new_key_share,
            new_threshold,
            completed_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "FROST key resharing completed successfully"
    }
}

/// Witness for FROST protocol failure
#[derive(Debug, Clone)]
pub struct FrostProtocolFailure {
    pub session_id: Uuid,
    pub error: String,
    pub failed_at: u64,
}

impl RuntimeWitness for FrostProtocolFailure {
    type Evidence = CryptoError;
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: CryptoError, config: (Uuid, u64)) -> Option<Self> {
        let (session_id, timestamp) = config;

        Some(FrostProtocolFailure {
            session_id,
            error: evidence.to_string(),
            failed_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "FROST protocol operation failed"
    }
}

// ========== FROST Session Error ==========

/// Errors that can occur in FROST session operations
#[derive(Debug, thiserror::Error)]
pub enum FrostSessionError {
    #[error("Crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("Insufficient participants: need {need}, have {have}")]
    InsufficientParticipants { need: u16, have: u16 },
    #[error("Invalid participant: {0}")]
    InvalidParticipant(String),
    #[error("Threshold not met: need {threshold}, have {count}")]
    ThresholdNotMet { threshold: u16, count: usize },
    #[error("Invalid signing state: {0}")]
    InvalidState(String),
    #[error("Nonce reuse detected")]
    NonceReuse,
    #[error("Session error: {0}")]
    SessionError(String),
}

// ========== SessionProtocol Implementations ==========

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostIdle> {
    type State = FrostIdle;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostCommitmentPhase> {
    type State = FrostCommitmentPhase;
    type Output = SigningCommitment;
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostAwaitingCommitments> {
    type State = FrostAwaitingCommitments;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostSigningPhase> {
    type State = FrostSigningPhase;
    type Output = SignatureShare;
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostAwaitingShares> {
    type State = FrostAwaitingShares;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostReadyToAggregate> {
    type State = FrostReadyToAggregate;
    type Output = ed25519_dalek::Signature;
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostSignatureComplete> {
    type State = FrostSignatureComplete;
    type Output = ed25519_dalek::Signature;
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, FrostSigningFailed> {
    type State = FrostSigningFailed;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

// Key generation implementations
impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, KeyGenerationInitializing> {
    type State = KeyGenerationInitializing;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, KeyGenerationInProgress> {
    type State = KeyGenerationInProgress;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, KeyGenerationComplete> {
    type State = KeyGenerationComplete;
    type Output = FrostKeyShare;
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

// Resharing implementations
impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, ResharingInitializing> {
    type State = ResharingInitializing;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, ResharingPhaseOne> {
    type State = ResharingPhaseOne;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, ResharingPhaseTwo> {
    type State = ResharingPhaseTwo;
    type Output = ();
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<FrostProtocolCore, ResharingComplete> {
    type State = ResharingComplete;
    type Output = FrostKeyShare;
    type Error = FrostSessionError;

    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

// ========== State Transitions ==========

// Signing protocol transitions
/// Transition from FrostIdle to FrostCommitmentPhase (when starting signing)
impl WitnessedTransition<FrostIdle, FrostCommitmentPhase>
    for ChoreographicProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = FrostSigningContext;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostCommitmentPhase>;
    
    /// Begin FROST signing protocol
    fn transition_with_witness(
        mut self,
        context: Self::Witness,
    ) -> Self::Target {
        self.inner.session_id = context.session_id;
        self.inner.message_to_sign = Some(context.message);
        self.transition_to()
    }
}

/// Transition from FrostCommitmentPhase to FrostAwaitingCommitments (after generating commitment)
impl WitnessedTransition<FrostCommitmentPhase, FrostAwaitingCommitments>
    for ChoreographicProtocol<FrostProtocolCore, FrostCommitmentPhase>
{
    type Witness = SigningCommitment;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostAwaitingCommitments>;
    
    /// Submit commitment and wait for others
    fn transition_with_witness(
        mut self,
        commitment: Self::Witness,
    ) -> Self::Target {
        self.inner
            .collected_commitments
            .insert(commitment.identifier, commitment);
        self.transition_to()
    }
}

/// Transition from FrostAwaitingCommitments to FrostSigningPhase (requires CommitmentThresholdMet witness)
impl WitnessedTransition<FrostAwaitingCommitments, FrostSigningPhase>
    for ChoreographicProtocol<FrostProtocolCore, FrostAwaitingCommitments>
{
    type Witness = CommitmentThresholdMet;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostSigningPhase>;
    
    /// Begin signing phase with sufficient commitments
    fn transition_with_witness(
        mut self,
        witness: Self::Witness,
    ) -> Self::Target {
        // Store all collected commitments
        for commitment in witness.commitments {
            self.inner
                .collected_commitments
                .insert(commitment.identifier, commitment);
        }
        self.transition_to()
    }
}

/// Transition from FrostSigningPhase to FrostAwaitingShares (after creating signature share)
impl WitnessedTransition<FrostSigningPhase, FrostAwaitingShares>
    for ChoreographicProtocol<FrostProtocolCore, FrostSigningPhase>
{
    type Witness = SignatureShare;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostAwaitingShares>;
    
    /// Submit signature share and wait for others
    fn transition_with_witness(
        mut self,
        share: Self::Witness,
    ) -> Self::Target {
        self.inner
            .collected_shares
            .insert(share.identifier, share);
        self.transition_to()
    }
}

/// Transition from FrostAwaitingShares to FrostReadyToAggregate (requires SignatureShareThresholdMet witness)
impl WitnessedTransition<FrostAwaitingShares, FrostReadyToAggregate>
    for ChoreographicProtocol<FrostProtocolCore, FrostAwaitingShares>
{
    type Witness = SignatureShareThresholdMet;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostReadyToAggregate>;
    
    /// Ready to aggregate with sufficient shares
    fn transition_with_witness(
        mut self,
        witness: Self::Witness,
    ) -> Self::Target {
        // Store all collected shares
        for share in witness.shares {
            self.inner
                .collected_shares
                .insert(share.identifier, share);
        }
        self.transition_to()
    }
}

/// Transition from FrostReadyToAggregate to FrostSignatureComplete (requires SignatureAggregated witness)
impl WitnessedTransition<FrostReadyToAggregate, FrostSignatureComplete>
    for ChoreographicProtocol<FrostProtocolCore, FrostReadyToAggregate>
{
    type Witness = SignatureAggregated;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostSignatureComplete>;
    
    /// Complete signature aggregation
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        self.transition_to()
    }
}

// Key generation transitions
/// Transition from FrostIdle to KeyGenerationInitializing
impl WitnessedTransition<FrostIdle, KeyGenerationInitializing>
    for ChoreographicProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = (u16, u16);
    type Target = ChoreographicProtocol<FrostProtocolCore, KeyGenerationInitializing>;
    
    /// Start key generation with threshold configuration
    fn transition_with_witness(
        mut self,
        config: Self::Witness,
    ) -> Self::Target {
        let (threshold, participant_count) = config;
        self.inner.threshold = threshold;
        self.inner.participant_count = participant_count;
        self.transition_to()
    }
}

/// Transition from KeyGenerationInitializing to KeyGenerationInProgress
impl WitnessedTransition<KeyGenerationInitializing, KeyGenerationInProgress>
    for ChoreographicProtocol<FrostProtocolCore, KeyGenerationInitializing>
{
    type Witness = ();
    type Target = ChoreographicProtocol<FrostProtocolCore, KeyGenerationInProgress>;
    
    /// Start key generation process
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from KeyGenerationInProgress to KeyGenerationComplete
impl WitnessedTransition<KeyGenerationInProgress, KeyGenerationComplete>
    for ChoreographicProtocol<FrostProtocolCore, KeyGenerationInProgress>
{
    type Witness = KeyGenerationCompleted;
    type Target = ChoreographicProtocol<FrostProtocolCore, KeyGenerationComplete>;
    
    /// Complete key generation
    fn transition_with_witness(
        mut self,
        witness: Self::Witness,
    ) -> Self::Target {
        self.inner.key_share = Some(witness.key_share);
        self.transition_to()
    }
}

// Resharing transitions
/// Transition from FrostIdle to ResharingInitializing
impl WitnessedTransition<FrostIdle, ResharingInitializing>
    for ChoreographicProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = (FrostKeyShare, u16);
    type Target = ChoreographicProtocol<FrostProtocolCore, ResharingInitializing>;
    
    /// Begin resharing with current key share
    fn transition_with_witness(
        mut self,
        config: Self::Witness,
    ) -> Self::Target {
        let (key_share, new_threshold) = config;
        self.inner.key_share = Some(key_share);
        self.inner.threshold = new_threshold;
        self.transition_to()
    }
}

/// Transition from ResharingInitializing to ResharingPhaseOne
impl WitnessedTransition<ResharingInitializing, ResharingPhaseOne>
    for ChoreographicProtocol<FrostProtocolCore, ResharingInitializing>
{
    type Witness = ();
    type Target = ChoreographicProtocol<FrostProtocolCore, ResharingPhaseOne>;
    
    /// Begin Phase 1: sub-share distribution
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from ResharingPhaseOne to ResharingPhaseTwo
impl WitnessedTransition<ResharingPhaseOne, ResharingPhaseTwo>
    for ChoreographicProtocol<FrostProtocolCore, ResharingPhaseOne>
{
    type Witness = ();
    type Target = ChoreographicProtocol<FrostProtocolCore, ResharingPhaseTwo>;
    
    /// Begin Phase 2: share reconstruction
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from ResharingPhaseTwo to ResharingComplete
impl WitnessedTransition<ResharingPhaseTwo, ResharingComplete>
    for ChoreographicProtocol<FrostProtocolCore, ResharingPhaseTwo>
{
    type Witness = FrostResharingCompleted;
    type Target = ChoreographicProtocol<FrostProtocolCore, ResharingComplete>;
    
    /// Complete resharing
    fn transition_with_witness(
        mut self,
        witness: Self::Witness,
    ) -> Self::Target {
        self.inner.key_share = Some(witness.new_key_share);
        self.inner.threshold = witness.new_threshold;
        self.transition_to()
    }
}

/// Transition to FrostSigningFailed from any state (requires FrostProtocolFailure witness)
impl<S: SessionState> WitnessedTransition<S, FrostSigningFailed>
    for ChoreographicProtocol<FrostProtocolCore, S>
where
    Self: SessionProtocol<State = S, Output = (), Error = FrostSessionError>,
{
    type Witness = FrostProtocolFailure;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostSigningFailed>;
    
    /// Handle protocol failure
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        self.transition_to()
    }
}

// ========== State-Specific Operations ==========

/// Operations only available in FrostCommitmentPhase state
impl ChoreographicProtocol<FrostProtocolCore, FrostCommitmentPhase> {
    /// Generate nonce and commitment for this signing round
    pub async fn generate_commitment(&mut self) -> Result<SigningCommitment, FrostSessionError> {
        use aura_crypto::{Effects, FrostSigner};

        // Ensure we have a key share to work with
        let key_share = self.inner.key_share.as_ref().ok_or_else(|| {
            FrostSessionError::InvalidState(
                "No key share available for commitment generation".to_string(),
            )
        })?;

        // Create deterministic effects for this session
        let effects = Effects::for_test(&format!("frost_commitment_{}", self.inner.session_id));
        let mut rng = effects.rng();

        // Generate nonces and commitment using FROST
        let (nonces, commitments) =
            FrostSigner::generate_nonces(&key_share.signing_share, &mut rng);

        // Store the nonces for later signature share generation
        self.inner.signing_nonces = Some(nonces);

        // Return the commitment wrapped in our structure
        Ok(SigningCommitment {
            identifier: self.inner.participant_id,
            commitment: commitments,
        })
    }

    /// Check if we can proceed to next phase
    pub fn can_proceed(&self) -> bool {
        self.inner.message_to_sign.is_some()
    }
}

/// Operations only available in FrostSigningPhase state
impl ChoreographicProtocol<FrostProtocolCore, FrostSigningPhase> {
    /// Create signature share for the message
    pub async fn create_signature_share(&self) -> Result<SignatureShare, FrostSessionError> {
        use aura_crypto::FrostSigner;
        use std::collections::BTreeMap;

        // Check threshold
        if self.inner.collected_commitments.len() < self.inner.threshold as usize {
            return Err(FrostSessionError::ThresholdNotMet {
                threshold: self.inner.threshold,
                count: self.inner.collected_commitments.len(),
            });
        }

        // Ensure we have all required data
        let key_share =
            self.inner.key_share.as_ref().ok_or_else(|| {
                FrostSessionError::InvalidState("No key share available".to_string())
            })?;

        let signing_nonces = self.inner.signing_nonces.as_ref().ok_or_else(|| {
            FrostSessionError::InvalidState(
                "No signing nonces available (must generate commitment first)".to_string(),
            )
        })?;

        let message = self
            .inner
            .message_to_sign
            .as_ref()
            .ok_or_else(|| FrostSessionError::InvalidState("No message to sign".to_string()))?;

        // Convert collected commitments to the format expected by FROST
        let mut frost_commitments = BTreeMap::new();
        for (id, commitment) in &self.inner.collected_commitments {
            frost_commitments.insert(*id, commitment.commitment.clone());
        }

        // For testing purposes, generate a temporary KeyPackage
        // In production, this would be properly stored from DKG
        let effects =
            aura_crypto::Effects::for_test(&format!("frost_keygen_{}", self.inner.session_id));
        let mut rng = effects.rng();

        let (secret_shares, _pubkey_package) = frost::keys::generate_with_dealer(
            self.inner.threshold,
            self.inner.participant_count,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            FrostSessionError::SessionError(format!("Failed to generate key package: {:?}", e))
        })?;

        // Convert SecretShare to KeyPackage
        let secret_share = secret_shares.values().next().ok_or_else(|| {
            FrostSessionError::SessionError("No secret share available".to_string())
        })?;

        let key_package = frost::keys::KeyPackage::try_from(secret_share.clone()).map_err(|e| {
            FrostSessionError::SessionError(format!("Failed to convert to KeyPackage: {:?}", e))
        })?;

        // Generate signature share using FROST
        let signature_share = FrostSigner::sign_share_with_package(
            message,
            signing_nonces,
            &frost_commitments,
            &key_package,
        )
        .map_err(|e| {
            FrostSessionError::SessionError(format!(
                "FROST signature share creation failed: {:?}",
                e
            ))
        })?;

        Ok(SignatureShare {
            identifier: self.inner.participant_id,
            share: signature_share,
        })
    }

    /// Get available commitments
    pub fn commitment_count(&self) -> usize {
        self.inner.collected_commitments.len()
    }
}

/// Operations only available in FrostReadyToAggregate state
impl ChoreographicProtocol<FrostProtocolCore, FrostReadyToAggregate> {
    /// Aggregate signature shares into final signature
    pub async fn aggregate_signature(&self) -> Result<SignatureAggregated, FrostSessionError> {
        if self.inner.collected_shares.len() < self.inner.threshold as usize {
            return Err(FrostSessionError::ThresholdNotMet {
                threshold: self.inner.threshold,
                count: self.inner.collected_shares.len(),
            });
        }

        let message = self
            .inner
            .message_to_sign
            .as_ref()
            .ok_or_else(|| FrostSessionError::InvalidState("No message to sign".to_string()))?;

        // In reality, this would aggregate using FROST library
        let signature = ed25519_dalek::Signature::from_bytes(&[0u8; 64]);

        let witness = SignatureAggregated {
            session_id: self.inner.session_id,
            signature,
            message: message.clone(),
            aggregated_at: 0, // Would use actual timestamp
        };

        Ok(witness)
    }

    /// Get collected share count
    pub fn share_count(&self) -> usize {
        self.inner.collected_shares.len()
    }

    /// Check if ready to aggregate
    pub fn is_ready(&self) -> bool {
        self.inner.collected_shares.len() >= self.inner.threshold as usize
    }
}

/// Operations for key generation
impl ChoreographicProtocol<FrostProtocolCore, KeyGenerationInProgress> {
    /// Perform distributed key generation
    pub async fn generate_key_share(&self) -> Result<KeyGenerationCompleted, FrostSessionError> {
        use aura_crypto::Effects;

        // Create deterministic effects for this session
        let effects = Effects::for_test(&format!("frost_keygen_{}", self.inner.session_id));
        let mut rng = effects.rng();

        // For now, use the dealer-based key generation for testing
        // In production, this would use a proper distributed key generation protocol
        let (shares, pubkey_package) = frost::keys::generate_with_dealer(
            self.inner.threshold,
            self.inner.participant_count,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            FrostSessionError::SessionError(format!("FROST key generation failed: {:?}", e))
        })?;

        // Find the key share for this participant
        let key_share = shares.get(&self.inner.participant_id).ok_or_else(|| {
            FrostSessionError::SessionError(
                "No key share generated for this participant".to_string(),
            )
        })?;

        // Convert to our FrostKeyShare format
        let frost_key_share = FrostKeyShare {
            identifier: self.inner.participant_id,
            signing_share: key_share.signing_share().clone(),
            verifying_key: pubkey_package.verifying_key().clone(),
        };

        // Create completion witness
        let witness = KeyGenerationCompleted {
            session_id: self.inner.session_id,
            key_share: frost_key_share,
            public_key: pubkey_package.verifying_key().clone(),
            completed_at: effects.now().unwrap_or(0),
        };

        Ok(witness)
    }
}

// ========== Session State Union Type ==========

/// Union type for all FROST session states
#[derive(Debug)]
pub enum FrostSessionState {
    Idle(ChoreographicProtocol<FrostProtocolCore, FrostIdle>),
    CommitmentPhase(ChoreographicProtocol<FrostProtocolCore, FrostCommitmentPhase>),
    AwaitingCommitments(ChoreographicProtocol<FrostProtocolCore, FrostAwaitingCommitments>),
    SigningPhase(ChoreographicProtocol<FrostProtocolCore, FrostSigningPhase>),
    AwaitingShares(ChoreographicProtocol<FrostProtocolCore, FrostAwaitingShares>),
    ReadyToAggregate(ChoreographicProtocol<FrostProtocolCore, FrostReadyToAggregate>),
    SignatureComplete(ChoreographicProtocol<FrostProtocolCore, FrostSignatureComplete>),
    SigningFailed(ChoreographicProtocol<FrostProtocolCore, FrostSigningFailed>),
    KeyGenerationInitializing(ChoreographicProtocol<FrostProtocolCore, KeyGenerationInitializing>),
    KeyGenerationInProgress(ChoreographicProtocol<FrostProtocolCore, KeyGenerationInProgress>),
    KeyGenerationComplete(ChoreographicProtocol<FrostProtocolCore, KeyGenerationComplete>),
    ResharingInitializing(ChoreographicProtocol<FrostProtocolCore, ResharingInitializing>),
    ResharingPhaseOne(ChoreographicProtocol<FrostProtocolCore, ResharingPhaseOne>),
    ResharingPhaseTwo(ChoreographicProtocol<FrostProtocolCore, ResharingPhaseTwo>),
    ResharingComplete(ChoreographicProtocol<FrostProtocolCore, ResharingComplete>),
}

impl FrostSessionState {
    /// Get current state name
    pub fn current_state_name(&self) -> &'static str {
        match self {
            FrostSessionState::Idle(f) => f.current_state_name(),
            FrostSessionState::CommitmentPhase(f) => f.current_state_name(),
            FrostSessionState::AwaitingCommitments(f) => f.current_state_name(),
            FrostSessionState::SigningPhase(f) => f.current_state_name(),
            FrostSessionState::AwaitingShares(f) => f.current_state_name(),
            FrostSessionState::ReadyToAggregate(f) => f.current_state_name(),
            FrostSessionState::SignatureComplete(f) => f.current_state_name(),
            FrostSessionState::SigningFailed(f) => f.current_state_name(),
            FrostSessionState::KeyGenerationInitializing(f) => f.current_state_name(),
            FrostSessionState::KeyGenerationInProgress(f) => f.current_state_name(),
            FrostSessionState::KeyGenerationComplete(f) => f.current_state_name(),
            FrostSessionState::ResharingInitializing(f) => f.current_state_name(),
            FrostSessionState::ResharingPhaseOne(f) => f.current_state_name(),
            FrostSessionState::ResharingPhaseTwo(f) => f.current_state_name(),
            FrostSessionState::ResharingComplete(f) => f.current_state_name(),
        }
    }

    /// Check if FROST protocol can be safely terminated
    pub fn can_terminate(&self) -> bool {
        match self {
            FrostSessionState::Idle(f) => f.can_terminate(),
            FrostSessionState::CommitmentPhase(f) => f.can_terminate(),
            FrostSessionState::AwaitingCommitments(f) => f.can_terminate(),
            FrostSessionState::SigningPhase(f) => f.can_terminate(),
            FrostSessionState::AwaitingShares(f) => f.can_terminate(),
            FrostSessionState::ReadyToAggregate(f) => f.can_terminate(),
            FrostSessionState::SignatureComplete(f) => f.can_terminate(),
            FrostSessionState::SigningFailed(f) => f.can_terminate(),
            FrostSessionState::KeyGenerationInitializing(f) => f.can_terminate(),
            FrostSessionState::KeyGenerationInProgress(f) => f.can_terminate(),
            FrostSessionState::KeyGenerationComplete(f) => f.can_terminate(),
            FrostSessionState::ResharingInitializing(f) => f.can_terminate(),
            FrostSessionState::ResharingPhaseOne(f) => f.can_terminate(),
            FrostSessionState::ResharingPhaseTwo(f) => f.can_terminate(),
            FrostSessionState::ResharingComplete(f) => f.can_terminate(),
        }
    }

    /// Check if FROST protocol is in final state
    pub fn is_final(&self) -> bool {
        match self {
            FrostSessionState::Idle(f) => f.is_final(),
            FrostSessionState::CommitmentPhase(f) => f.is_final(),
            FrostSessionState::AwaitingCommitments(f) => f.is_final(),
            FrostSessionState::SigningPhase(f) => f.is_final(),
            FrostSessionState::AwaitingShares(f) => f.is_final(),
            FrostSessionState::ReadyToAggregate(f) => f.is_final(),
            FrostSessionState::SignatureComplete(f) => f.is_final(),
            FrostSessionState::SigningFailed(f) => f.is_final(),
            FrostSessionState::KeyGenerationInitializing(f) => f.is_final(),
            FrostSessionState::KeyGenerationInProgress(f) => f.is_final(),
            FrostSessionState::KeyGenerationComplete(f) => f.is_final(),
            FrostSessionState::ResharingInitializing(f) => f.is_final(),
            FrostSessionState::ResharingPhaseOne(f) => f.is_final(),
            FrostSessionState::ResharingPhaseTwo(f) => f.is_final(),
            FrostSessionState::ResharingComplete(f) => f.is_final(),
        }
    }

    /// Get session ID
    pub fn session_id(&self) -> Uuid {
        match self {
            FrostSessionState::Idle(f) => f.inner.session_id,
            FrostSessionState::CommitmentPhase(f) => f.inner.session_id,
            FrostSessionState::AwaitingCommitments(f) => f.inner.session_id,
            FrostSessionState::SigningPhase(f) => f.inner.session_id,
            FrostSessionState::AwaitingShares(f) => f.inner.session_id,
            FrostSessionState::ReadyToAggregate(f) => f.inner.session_id,
            FrostSessionState::SignatureComplete(f) => f.inner.session_id,
            FrostSessionState::SigningFailed(f) => f.inner.session_id,
            FrostSessionState::KeyGenerationInitializing(f) => f.inner.session_id,
            FrostSessionState::KeyGenerationInProgress(f) => f.inner.session_id,
            FrostSessionState::KeyGenerationComplete(f) => f.inner.session_id,
            FrostSessionState::ResharingInitializing(f) => f.inner.session_id,
            FrostSessionState::ResharingPhaseOne(f) => f.inner.session_id,
            FrostSessionState::ResharingPhaseTwo(f) => f.inner.session_id,
            FrostSessionState::ResharingComplete(f) => f.inner.session_id,
        }
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed FROST protocol in idle state
pub fn new_session_typed_frost(
    device_id: aura_journal::DeviceId,
    participant_id: frost::Identifier,
    threshold: u16,
    participant_count: u16,
) -> ChoreographicProtocol<FrostProtocolCore, FrostIdle> {
    let session_id = Uuid::new_v4();
    let core = FrostProtocolCore::new(session_id, device_id, participant_id, threshold, participant_count);
    ChoreographicProtocol::new(core)
}

/// Rehydrate FROST session from signing progress
pub fn rehydrate_frost_session(
    device_id: aura_journal::DeviceId,
    participant_id: frost::Identifier,
    threshold: u16,
    participant_count: u16,
    has_commitments: bool,
    has_shares: bool,
) -> FrostSessionState {
    let session_id = Uuid::new_v4();
    let core = FrostProtocolCore::new(session_id, device_id, participant_id, threshold, participant_count);

    if has_shares {
        FrostSessionState::ReadyToAggregate(ChoreographicProtocol::new(core))
    } else if has_commitments {
        FrostSessionState::SigningPhase(ChoreographicProtocol::new(core))
    } else {
        FrostSessionState::Idle(ChoreographicProtocol::new(core))
    }
}

// ========== Helper Functions for Testing ==========
//
// NOTE: These placeholder functions are simplified for testing the session type system.
// In a real implementation, these would use proper FROST cryptographic operations.

/// Create a placeholder signing commitment for testing
///
/// WARNING: This generates valid FROST commitments using generated test keys.
/// Only use for testing the session type system.
fn create_placeholder_commitment(participant_id: frost::Identifier) -> SigningCommitment {
    use aura_crypto::{Effects, FrostSigner};

    // Create test effects for deterministic randomness based on participant ID
    let effects = Effects::for_test(&format!(
        "frost_commitment_{}",
        participant_id.serialize()[0]
    ));
    let mut rng = effects.rng();

    // Generate a temporary key package for this test
    // In production, this would come from the DKD protocol
    let (shares, _pubkey_package) = frost::keys::generate_with_dealer(
        2, // threshold
        2, // num_participants
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .expect("Should generate test keys");

    // Use the first available share to generate a commitment
    let (_id, key_share) = shares
        .into_iter()
        .next()
        .expect("Should have at least one share");
    let key_package =
        frost::keys::KeyPackage::try_from(key_share).expect("Should create key package");

    // Generate nonces and commitment
    let (_nonces, commitments) =
        FrostSigner::generate_nonces(key_package.signing_share(), &mut rng);

    SigningCommitment {
        identifier: participant_id,
        commitment: commitments,
    }
}

/// Create a placeholder signature share for testing
///
/// WARNING: This generates valid FROST signature shares using generated test keys.
/// Only use for testing the session type system.
fn create_placeholder_signature_share(participant_id: frost::Identifier) -> SignatureShare {
    use aura_crypto::{Effects, FrostSigner};
    use std::collections::BTreeMap;

    // Create test effects for deterministic randomness based on participant ID
    let effects = Effects::for_test(&format!(
        "frost_signature_{}",
        participant_id.serialize()[0]
    ));
    let mut rng = effects.rng();

    // Generate a temporary key package for this test
    // In production, this would come from the DKD protocol
    let (shares, _pubkey_package) = frost::keys::generate_with_dealer(
        2, // threshold
        2, // num_participants
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .expect("Should generate test keys");

    // Use the first available share to generate a signature share
    let (_id, key_share) = shares
        .into_iter()
        .next()
        .expect("Should have at least one share");
    let key_package =
        frost::keys::KeyPackage::try_from(key_share).expect("Should create key package");

    // Generate nonces and commitments for this participant
    let (nonces, commitments) = FrostSigner::generate_nonces(key_package.signing_share(), &mut rng);

    // Create a minimal commitment map with this participant only
    // In real usage, this would have all participants' commitments
    let mut all_commitments = BTreeMap::new();
    all_commitments.insert(participant_id, commitments);

    // Sign a test message
    let test_message = b"test message for placeholder signature share";
    let signature_share =
        FrostSigner::sign_share_with_package(test_message, &nonces, &all_commitments, &key_package)
            .expect("Should create signature share");

    SignatureShare {
        identifier: participant_id,
        share: signature_share,
    }
}

// ========== Tests ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frost_session_creation() {
        let device_id = aura_crypto::DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let frost = new_session_typed_frost(device_id, participant_id, 2, 3);

        assert_eq!(frost.current_state_name(), "FrostIdle");
        assert!(frost.can_terminate());
        assert!(!frost.is_final());
    }

    #[test]
    fn test_signing_flow_transitions() {
        let device_id = aura_crypto::DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let frost = new_session_typed_frost(device_id, participant_id, 2, 3);

        // Start signing
        let context = FrostSigningContext {
            session_id: frost.inner.session_id,
            message: vec![1, 2, 3, 4],
            participants: vec![participant_id],
            started_at: 1000,
        };

        let commitment_phase = frost.transition_with_witness(context);
        assert_eq!(
            commitment_phase.current_state_name(),
            "FrostCommitmentPhase"
        );

        // Generate commitment
        let commitment = SigningCommitment {
            identifier: participant_id,
            commitment: frost::round1::SigningCommitments::default(),
        };

        let awaiting = commitment_phase.transition_with_witness(commitment);
        assert_eq!(awaiting.current_state_name(), "FrostAwaitingCommitments");

        // Simulate threshold met
        let commitments = vec![
            create_placeholder_commitment(frost::Identifier::try_from(1u16).unwrap()),
            create_placeholder_commitment(frost::Identifier::try_from(2u16).unwrap()),
        ];

        let witness = CommitmentThresholdMet::verify(commitments, (Uuid::new_v4(), 2)).unwrap();
        let signing_phase = awaiting.transition_with_witness(witness);
        assert_eq!(signing_phase.current_state_name(), "FrostSigningPhase");
    }

    #[test]
    fn test_commitment_threshold_witness() {
        let commitments = vec![
            create_placeholder_commitment(frost::Identifier::try_from(1u16).unwrap()),
            create_placeholder_commitment(frost::Identifier::try_from(2u16).unwrap()),
        ];

        // Should succeed with threshold of 2
        let witness = CommitmentThresholdMet::verify(commitments.clone(), (Uuid::new_v4(), 2));
        assert!(witness.is_some());

        // Should fail with threshold of 3
        let witness = CommitmentThresholdMet::verify(commitments, (Uuid::new_v4(), 3));
        assert!(witness.is_none());
    }

    // Note: This test is commented out because it requires proper FROST key generation
    // which is not implemented in the placeholder functions.
    /*
    #[test]
    fn test_key_generation_flow() {
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let frost = new_session_typed_frost(device_id, participant_id, 2, 3);

        let initializing = frost.transition_with_witness((2u16, 3u16));
        assert_eq!(initializing.current_state_name(), "KeyGenerationInitializing");

        let in_progress = initializing.transition_with_witness(());
        assert_eq!(in_progress.current_state_name(), "KeyGenerationInProgress");

        let key_share = create_placeholder_key_share(participant_id);

        let witness = KeyGenerationCompleted::verify(
            (key_share.clone(), key_share.verifying_key),
            (Uuid::new_v4(), 2000)
        ).unwrap();

        let complete = in_progress.transition_with_witness(witness);
        assert_eq!(complete.current_state_name(), "KeyGenerationComplete");
        assert!(complete.is_final());
    }
    */

    #[test]
    fn test_session_state_union() {
        let device_id = aura_crypto::DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let session = rehydrate_frost_session(device_id, participant_id, 2, 3, false, false);

        assert_eq!(session.current_state_name(), "FrostIdle");
        assert!(session.can_terminate());
        assert!(!session.is_final());

        let signing_session = rehydrate_frost_session(device_id, participant_id, 2, 3, true, false);
        assert_eq!(signing_session.current_state_name(), "FrostSigningPhase");

        let ready_session = rehydrate_frost_session(device_id, participant_id, 2, 3, true, true);
        assert_eq!(ready_session.current_state_name(), "FrostReadyToAggregate");
    }
}
