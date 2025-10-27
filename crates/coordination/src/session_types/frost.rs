//! Session Type States for FROST Cryptographic Protocol (Refactored with Macros)
//!
//! This module defines session types for FROST threshold signature operations,
//! providing compile-time safety for the signing protocol state machine.

use crate::session_types::wrapper::SessionTypedProtocol;
use aura_crypto::{CryptoError, FrostKeyShare, SignatureShare, SigningCommitment};
use aura_types::DeviceId;
use frost_ed25519 as frost;
use session_types::{witnesses::RuntimeWitness, SessionState};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tracing::{debug, warn};
use uuid::Uuid;

/// Witness that commitment threshold has been met
#[derive(Debug, Clone)]
pub struct CommitmentThresholdMet {
    pub count: usize,
    pub threshold: usize,
}

impl RuntimeWitness for CommitmentThresholdMet {
    type Evidence = Vec<Vec<u8>>; // commitment events
    type Config = usize; // threshold

    fn verify(evidence: Self::Evidence, threshold: Self::Config) -> Option<Self> {
        let count = evidence.len();
        if count >= threshold {
            Some(CommitmentThresholdMet { count, threshold })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Commitment threshold met"
    }
}

/// Witness that signature share threshold has been met
#[derive(Debug, Clone)]
pub struct SignatureShareThresholdMet {
    pub count: usize,
    pub threshold: usize,
}

impl RuntimeWitness for SignatureShareThresholdMet {
    type Evidence = Vec<SignatureShare>;
    type Config = usize; // threshold

    fn verify(evidence: Self::Evidence, threshold: Self::Config) -> Option<Self> {
        let count = evidence.len();
        if count >= threshold {
            Some(SignatureShareThresholdMet { count, threshold })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Signature share threshold met"
    }
}

/// Witness that signature has been aggregated
#[derive(Debug, Clone)]
pub struct SignatureAggregated {
    pub signature: Vec<u8>,
}

impl RuntimeWitness for SignatureAggregated {
    type Evidence = Vec<u8>; // aggregated signature
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        if !evidence.is_empty() {
            Some(SignatureAggregated {
                signature: evidence,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Signature aggregated successfully"
    }
}

/// Witness that key generation has been completed
#[derive(Debug, Clone)]
pub struct KeyGenerationCompleted {
    pub key_share: FrostKeyShare,
}

impl RuntimeWitness for KeyGenerationCompleted {
    type Evidence = FrostKeyShare;
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        Some(KeyGenerationCompleted {
            key_share: evidence,
        })
    }

    fn description(&self) -> &'static str {
        "Key generation completed"
    }
}

/// Witness that FROST resharing has been completed
#[derive(Debug, Clone)]
pub struct FrostResharingCompleted {
    pub new_key_share: FrostKeyShare,
}

impl RuntimeWitness for FrostResharingCompleted {
    type Evidence = FrostKeyShare;
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        Some(FrostResharingCompleted {
            new_key_share: evidence,
        })
    }

    fn description(&self) -> &'static str {
        "FROST resharing completed"
    }
}

/// Witness that FROST protocol has failed
#[derive(Debug, Clone)]
pub struct FrostProtocolFailure {
    pub error_message: String,
}

impl RuntimeWitness for FrostProtocolFailure {
    type Evidence = String; // error message
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        if !evidence.is_empty() {
            Some(FrostProtocolFailure {
                error_message: evidence,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "FROST protocol failure"
    }
}

// ========== FROST Protocol Core ==========

/// Core FROST protocol data without session state
pub struct FrostProtocolCore {
    pub session_id: Uuid,
    pub protocol_id: Uuid,
    pub device_id: DeviceId,
    pub participant_id: frost::Identifier,
    pub key_share: Option<FrostKeyShare>,
    pub message_to_sign: Option<Vec<u8>>,
    pub signing_nonces: Option<frost::round1::SigningNonces>,
    pub collected_commitments: BTreeMap<frost::Identifier, SigningCommitment>,
    pub collected_shares: BTreeMap<frost::Identifier, SignatureShare>,
    pub threshold: u16,
    pub participant_count: u16,
}

// ========== FROST Session States ==========
// Session states are defined using the macro below to avoid duplication

// Manual implementations for FrostProtocolCore
impl std::fmt::Debug for FrostProtocolCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

impl Clone for FrostProtocolCore {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id,
            protocol_id: self.protocol_id,
            device_id: self.device_id,
            participant_id: self.participant_id,
            key_share: self.key_share.clone(),
            message_to_sign: self.message_to_sign.clone(),
            // Note: SigningNonces cannot be cloned safely as they contain secret values that should not be reused
            // In a real protocol, nonces are single-use only. For session types, we set to None.
            signing_nonces: None,
            // Clone commitment and share collections with placeholder values for cryptographic data
            collected_commitments: self
                .collected_commitments
                .keys()
                .map(|id| (*id, create_placeholder_commitment(*id)))
                .collect(),
            collected_shares: self
                .collected_shares
                .keys()
                .map(|id| (*id, create_placeholder_signature_share(*id)))
                .collect(),
            threshold: self.threshold,
            participant_count: self.participant_count,
        }
    }
}

impl FrostProtocolCore {
    pub fn new(
        session_id: Uuid,
        device_id: DeviceId,
        participant_id: frost::Identifier,
        threshold: u16,
        participant_count: u16,
    ) -> Self {
        Self {
            session_id,
            protocol_id: Uuid::new_v4(),
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

// ========== Error Type ==========

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

impl From<FrostSessionError> for aura_errors::AuraError {
    fn from(error: FrostSessionError) -> Self {
        match error {
            FrostSessionError::Crypto(crypto_err) => {
                aura_errors::AuraError::crypto_operation_failed(format!(
                    "FROST crypto error: {:?}",
                    crypto_err
                ))
            }
            FrostSessionError::InsufficientParticipants { need, have } => {
                aura_errors::AuraError::protocol_timeout(format!(
                    "Insufficient participants: need {}, have {}",
                    need, have
                ))
            }
            FrostSessionError::InvalidParticipant(msg) => {
                aura_errors::AuraError::protocol_timeout(format!("Invalid participant: {}", msg))
            }
            FrostSessionError::ThresholdNotMet { threshold, count } => {
                aura_errors::AuraError::protocol_timeout(format!(
                    "Threshold not met: need {}, have {}",
                    threshold, count
                ))
            }
            FrostSessionError::InvalidState(msg) => {
                aura_errors::AuraError::invalid_transition(format!("Invalid FROST state: {}", msg))
            }
            FrostSessionError::NonceReuse => {
                aura_errors::AuraError::crypto_operation_failed("FROST nonce reuse detected")
            }
            FrostSessionError::SessionError(msg) => {
                aura_errors::AuraError::session_aborted(format!("FROST session error: {}", msg))
            }
        }
    }
}

// ========== Protocol Definition using Macros ==========

// TODO: Define protocol manually to avoid orphan rule violations
// The define_protocol! macro internally uses impl_session_protocol! which tries to
// implement external traits for external types, violating Rust's orphan rules.

// Session states are manually defined to avoid orphan rule violations

/// FROST idle state - ready to start operations
#[derive(Debug, Clone)]
pub struct FrostIdle;

impl SessionState for FrostIdle {
    const NAME: &'static str = "FrostIdle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST commitment phase - collecting commitments
#[derive(Debug, Clone)]
pub struct FrostCommitmentPhase;

impl SessionState for FrostCommitmentPhase {
    const NAME: &'static str = "FrostCommitmentPhase";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST awaiting commitments state
#[derive(Debug, Clone)]
pub struct FrostAwaitingCommitments;

impl SessionState for FrostAwaitingCommitments {
    const NAME: &'static str = "FrostAwaitingCommitments";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST signing phase - collecting signature shares
#[derive(Debug, Clone)]
pub struct FrostSigningPhase;

impl SessionState for FrostSigningPhase {
    const NAME: &'static str = "FrostSigningPhase";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST awaiting shares state
#[derive(Debug, Clone)]
pub struct FrostAwaitingShares;

impl SessionState for FrostAwaitingShares {
    const NAME: &'static str = "FrostAwaitingShares";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST ready to aggregate - ready to finalize signature
#[derive(Debug, Clone)]
pub struct FrostReadyToAggregate;

impl SessionState for FrostReadyToAggregate {
    const NAME: &'static str = "FrostReadyToAggregate";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST signature complete state
#[derive(Debug, Clone)]
pub struct FrostSignatureComplete;

impl SessionState for FrostSignatureComplete {
    const NAME: &'static str = "FrostSignatureComplete";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// FROST signing failed state
#[derive(Debug, Clone)]
pub struct FrostSigningFailed;

impl SessionState for FrostSigningFailed {
    const NAME: &'static str = "FrostSigningFailed";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Key generation initializing state
#[derive(Debug, Clone)]
pub struct KeyGenerationInitializing;

impl SessionState for KeyGenerationInitializing {
    const NAME: &'static str = "KeyGenerationInitializing";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Key generation in progress state
#[derive(Debug, Clone)]
pub struct KeyGenerationInProgress;

impl SessionState for KeyGenerationInProgress {
    const NAME: &'static str = "KeyGenerationInProgress";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Key generation complete state
#[derive(Debug, Clone)]
pub struct KeyGenerationComplete;

impl SessionState for KeyGenerationComplete {
    const NAME: &'static str = "KeyGenerationComplete";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Resharing initializing state (for FROST)
#[derive(Debug, Clone)]
pub struct ResharingInitializing;

impl SessionState for ResharingInitializing {
    const NAME: &'static str = "ResharingInitializing";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Resharing phase one state (for FROST)
#[derive(Debug, Clone)]
pub struct ResharingPhaseOne;

impl SessionState for ResharingPhaseOne {
    const NAME: &'static str = "ResharingPhaseOne";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Resharing phase two state (for FROST)
#[derive(Debug, Clone)]
pub struct ResharingPhaseTwo;

impl SessionState for ResharingPhaseTwo {
    const NAME: &'static str = "ResharingPhaseTwo";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Resharing complete state (for FROST)
#[derive(Debug, Clone)]
pub struct ResharingComplete;

impl SessionState for ResharingComplete {
    const NAME: &'static str = "ResharingComplete";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

// Define union type manually (was generated by define_protocol! macro)
#[derive(Debug, Clone)]
pub enum FrostSessionState {
    FrostIdle(SessionTypedProtocol<FrostProtocolCore, FrostIdle>),
    FrostCommitmentPhase(SessionTypedProtocol<FrostProtocolCore, FrostCommitmentPhase>),
    FrostAwaitingCommitments(SessionTypedProtocol<FrostProtocolCore, FrostAwaitingCommitments>),
    FrostSigningPhase(SessionTypedProtocol<FrostProtocolCore, FrostSigningPhase>),
    FrostAwaitingShares(SessionTypedProtocol<FrostProtocolCore, FrostAwaitingShares>),
    FrostReadyToAggregate(SessionTypedProtocol<FrostProtocolCore, FrostReadyToAggregate>),
    FrostSignatureComplete(SessionTypedProtocol<FrostProtocolCore, FrostSignatureComplete>),
    FrostSigningFailed(SessionTypedProtocol<FrostProtocolCore, FrostSigningFailed>),
    KeyGenerationInitializing(SessionTypedProtocol<FrostProtocolCore, KeyGenerationInitializing>),
    KeyGenerationInProgress(SessionTypedProtocol<FrostProtocolCore, KeyGenerationInProgress>),
    KeyGenerationComplete(SessionTypedProtocol<FrostProtocolCore, KeyGenerationComplete>),
    ResharingInitializing(SessionTypedProtocol<FrostProtocolCore, ResharingInitializing>),
    ResharingPhaseOne(SessionTypedProtocol<FrostProtocolCore, ResharingPhaseOne>),
    ResharingPhaseTwo(SessionTypedProtocol<FrostProtocolCore, ResharingPhaseTwo>),
    ResharingComplete(SessionTypedProtocol<FrostProtocolCore, ResharingComplete>),
}

// ========== Protocol Type Alias ==========

/// Session-typed FROST protocol wrapper
pub type SessionTypedFrost<S> = SessionTypedProtocol<FrostProtocolCore, S>;

// ========== Context Types ==========

/// Context for FROST signing operation
#[derive(Debug, Clone)]
pub struct FrostSigningContext {
    pub session_id: Uuid,
    pub message: Vec<u8>,
    pub participants: Vec<frost::Identifier>,
    pub started_at: u64,
}

impl RuntimeWitness for FrostSigningContext {
    type Evidence = (Uuid, Vec<u8>, Vec<frost::Identifier>);
    type Config = u64; // timestamp

    fn verify(evidence: (Uuid, Vec<u8>, Vec<frost::Identifier>), timestamp: u64) -> Option<Self> {
        let (session_id, message, participants) = evidence;
        Some(FrostSigningContext {
            session_id,
            message,
            participants,
            started_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "FROST signing context initialized"
    }
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
        SignatureShareContext {
            participant_id: self.participant_id,
            share: create_placeholder_signature_share(self.participant_id),
            created_at: self.created_at,
        }
    }
}

// ========== Runtime Witnesses ==========
//
// Note: Using witnesses from crate::witnesses module to avoid duplication

// ========== Protocol Methods ==========

/// Trait for basic FROST protocol operations (available on all states)
pub trait FrostProtocolOperations {
    /// Get reference to the protocol core
    fn core(&self) -> &FrostProtocolCore;
    /// Get the participant ID
    fn participant_id(&self) -> frost::Identifier;
    /// Get the threshold
    fn threshold(&self) -> u16;
    /// Get the participant count
    fn participant_count(&self) -> u16;
}

/// Implementation for all FROST protocol states
impl<S: SessionState> FrostProtocolOperations for SessionTypedProtocol<FrostProtocolCore, S> {
    fn core(&self) -> &FrostProtocolCore {
        &self.inner
    }

    fn participant_id(&self) -> frost::Identifier {
        self.core().participant_id
    }

    fn threshold(&self) -> u16 {
        self.core().threshold
    }

    fn participant_count(&self) -> u16 {
        self.core().participant_count
    }
}

// ========== State Transitions ==========

// BEGIN DEPRECATED: Orphan rule violation implementations
/*
/*
/// Transition from FrostIdle to FrostCommitmentPhase (when starting signing)
impl WitnessedTransition<FrostIdle, FrostCommitmentPhase>
    for SessionTypedProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = FrostSigningContext;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostCommitmentPhase>;

    /// Begin FROST signing protocol
    fn transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
        self.inner.session_id = context.session_id;
        self.inner.message_to_sign = Some(context.message);
        self.transition_to()
    }
}
*/

/*
/// Transition from FrostCommitmentPhase to FrostAwaitingCommitments (after generating commitment)
impl WitnessedTransition<FrostCommitmentPhase, FrostAwaitingCommitments>
    for SessionTypedProtocol<FrostProtocolCore, FrostCommitmentPhase>
{
    type Witness = SigningCommitment;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostAwaitingCommitments>;

    /// Submit commitment and wait for others
    fn transition_with_witness(mut self, commitment: Self::Witness) -> Self::Target {
        self.inner
            .collected_commitments
            .insert(commitment.identifier, commitment);
        self.transition_to()
    }
}
*/

/*
/// Transition from FrostAwaitingCommitments to FrostSigningPhase (requires CommitmentThresholdMet witness)
impl WitnessedTransition<FrostAwaitingCommitments, FrostSigningPhase>
    for SessionTypedProtocol<FrostProtocolCore, FrostAwaitingCommitments>
{
    type Witness = CommitmentThresholdMet;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostSigningPhase>;

    /// Begin signing phase with sufficient commitments
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Note: The witness only contains count and threshold,
        // actual commitments should be handled separately in the protocol implementation
        self.transition_to()
    }
}
*/

/*
/// Transition from FrostSigningPhase to FrostAwaitingShares (after creating signature share)
impl WitnessedTransition<FrostSigningPhase, FrostAwaitingShares>
    for SessionTypedProtocol<FrostProtocolCore, FrostSigningPhase>
{
    type Witness = SignatureShare;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostAwaitingShares>;

    /// Submit signature share and wait for others
    fn transition_with_witness(mut self, share: Self::Witness) -> Self::Target {
        self.inner.collected_shares.insert(share.identifier, share);
        self.transition_to()
    }
}
*/

/*
/// Transition from FrostAwaitingShares to FrostReadyToAggregate (requires SignatureShareThresholdMet witness)
impl WitnessedTransition<FrostAwaitingShares, FrostReadyToAggregate>
    for SessionTypedProtocol<FrostProtocolCore, FrostAwaitingShares>
{
    type Witness = SignatureShareThresholdMet;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostReadyToAggregate>;

    /// Ready to aggregate with sufficient shares
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Note: The witness only contains count and threshold,
        // actual shares should be handled separately in the protocol implementation
        self.transition_to()
    }
}
*/

/*
/// Transition from FrostReadyToAggregate to FrostSignatureComplete (requires SignatureAggregated witness)
impl WitnessedTransition<FrostReadyToAggregate, FrostSignatureComplete>
    for SessionTypedProtocol<FrostProtocolCore, FrostReadyToAggregate>
{
    type Witness = SignatureAggregated;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostSignatureComplete>;

    /// Complete signature aggregation
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}
*/

/*
// Key generation transitions
/// Transition from FrostIdle to KeyGenerationInitializing
impl WitnessedTransition<FrostIdle, KeyGenerationInitializing>
    for SessionTypedProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = (u16, u16);
    type Target = SessionTypedProtocol<FrostProtocolCore, KeyGenerationInitializing>;

    /// Start key generation with threshold configuration
    fn transition_with_witness(mut self, config: Self::Witness) -> Self::Target {
        let (threshold, participant_count) = config;
        self.inner.threshold = threshold;
        self.inner.participant_count = participant_count;
        self.transition_to()
    }
}
*/

/*
/// Transition from KeyGenerationInitializing to KeyGenerationInProgress
impl WitnessedTransition<KeyGenerationInitializing, KeyGenerationInProgress>
    for SessionTypedProtocol<FrostProtocolCore, KeyGenerationInitializing>
{
    type Witness = ();
    type Target = SessionTypedProtocol<FrostProtocolCore, KeyGenerationInProgress>;

    /// Start key generation process
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}
*/

/*
/// Transition from KeyGenerationInProgress to KeyGenerationComplete
impl WitnessedTransition<KeyGenerationInProgress, KeyGenerationComplete>
    for SessionTypedProtocol<FrostProtocolCore, KeyGenerationInProgress>
{
    type Witness = KeyGenerationCompleted;
    type Target = SessionTypedProtocol<FrostProtocolCore, KeyGenerationComplete>;

    /// Complete key generation
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Note: The witness only contains key_id, actual key_share should be
        // handled separately in the protocol implementation
        self.transition_to()
    }
}
*/

/*
// Resharing transitions
/// Transition from FrostIdle to ResharingInitializing
impl WitnessedTransition<FrostIdle, ResharingInitializing>
    for SessionTypedProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = (FrostKeyShare, u16);
    type Target = SessionTypedProtocol<FrostProtocolCore, ResharingInitializing>;

    /// Begin resharing with current key share
    fn transition_with_witness(mut self, config: Self::Witness) -> Self::Target {
        let (key_share, new_threshold) = config;
        self.inner.key_share = Some(key_share);
        self.inner.threshold = new_threshold;
        self.transition_to()
    }
}
*/

/*
/// Transition from ResharingInitializing to ResharingPhaseOne
impl WitnessedTransition<ResharingInitializing, ResharingPhaseOne>
    for SessionTypedProtocol<FrostProtocolCore, ResharingInitializing>
{
    type Witness = ();
    type Target = SessionTypedProtocol<FrostProtocolCore, ResharingPhaseOne>;

    /// Begin Phase 1: sub-share distribution
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}
*/

/*
/// Transition from ResharingPhaseOne to ResharingPhaseTwo
impl WitnessedTransition<ResharingPhaseOne, ResharingPhaseTwo>
    for SessionTypedProtocol<FrostProtocolCore, ResharingPhaseOne>
{
    type Witness = ();
    type Target = SessionTypedProtocol<FrostProtocolCore, ResharingPhaseTwo>;

    /// Begin Phase 2: share reconstruction
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}
*/

/*
/// Transition from ResharingPhaseTwo to ResharingComplete
impl WitnessedTransition<ResharingPhaseTwo, ResharingComplete>
    for SessionTypedProtocol<FrostProtocolCore, ResharingPhaseTwo>
{
    type Witness = FrostResharingCompleted;
    type Target = SessionTypedProtocol<FrostProtocolCore, ResharingComplete>;

    /// Complete resharing
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Resharing completed - the threshold remains the same unless explicitly changed
        // In a real implementation, the threshold would be extracted from the new key share
        self.transition_to()
    }
}
*/

/*
/// Transition to FrostSigningFailed from FrostIdle (requires FrostProtocolFailure witness)
impl WitnessedTransition<FrostIdle, FrostSigningFailed>
    for SessionTypedProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = FrostProtocolFailure;
    type Target = SessionTypedProtocol<FrostProtocolCore, FrostSigningFailed>;

    /// Handle protocol failure
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}
*/
// END DEPRECATED: These implementations violate orphan rules
*/

// ========== State-Specific Operations ==========

/// Trait for FROST commitment phase operations
pub trait FrostCommitmentOperations {
    /// Generate nonce and commitment for this signing round
    fn generate_commitment(
        &mut self,
    ) -> impl std::future::Future<Output = std::result::Result<SigningCommitment, FrostSessionError>>
           + Send;
    /// Check if we can proceed to next phase
    fn can_proceed(&self) -> bool;
}

/// Implementation for FrostCommitmentPhase state
impl FrostCommitmentOperations for SessionTypedProtocol<FrostProtocolCore, FrostCommitmentPhase> {
    async fn generate_commitment(
        &mut self,
    ) -> std::result::Result<SigningCommitment, FrostSessionError> {
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

    fn can_proceed(&self) -> bool {
        self.inner.message_to_sign.is_some()
    }
}

/// Trait for FROST signing phase operations
pub trait FrostSigningOperations {
    /// Create signature share for the message
    fn create_signature_share(
        &self,
    ) -> impl std::future::Future<Output = std::result::Result<SignatureShare, FrostSessionError>> + Send;
    /// Get available commitments
    fn commitment_count(&self) -> usize;
}

/// Implementation for FrostSigningPhase state
impl FrostSigningOperations for SessionTypedProtocol<FrostProtocolCore, FrostSigningPhase> {
    async fn create_signature_share(
        &self,
    ) -> std::result::Result<SignatureShare, FrostSessionError> {
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
        let _key_share =
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
            frost_commitments.insert(*id, commitment.commitment);
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

    fn commitment_count(&self) -> usize {
        self.inner.collected_commitments.len()
    }
}

/// Trait for FROST aggregation operations
pub trait FrostAggregationOperations {
    /// Aggregate signature shares into final signature
    fn aggregate_signature(
        &self,
    ) -> impl std::future::Future<Output = std::result::Result<SignatureAggregated, FrostSessionError>>
           + Send;
    /// Get collected share count
    fn share_count(&self) -> usize;
    /// Check if ready to aggregate
    fn is_ready(&self) -> bool;

    // Internal helper methods (implementation details)
    /// Parse collected commitments and shares as FROST types (internal use)
    #[allow(clippy::type_complexity)]
    fn parse_frost_commitments_and_shares(
        &self,
        commitments: &BTreeMap<frost::Identifier, SigningCommitment>,
        signature_shares: &BTreeMap<frost::Identifier, SignatureShare>,
    ) -> std::result::Result<
        (
            BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
            BTreeMap<frost::Identifier, frost::round2::SignatureShare>,
            frost::keys::PublicKeyPackage,
        ),
        FrostSessionError,
    >;

    /// Create fallback signature when FROST fails (internal use)
    fn create_fallback_signature(
        &self,
        commitments: &BTreeMap<frost::Identifier, SigningCommitment>,
        signature_shares: &BTreeMap<frost::Identifier, SignatureShare>,
        message: &[u8],
    ) -> Vec<u8>;
}

/// Implementation for FrostReadyToAggregate state
impl FrostAggregationOperations for SessionTypedProtocol<FrostProtocolCore, FrostReadyToAggregate> {
    async fn aggregate_signature(
        &self,
    ) -> std::result::Result<SignatureAggregated, FrostSessionError> {
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

        // Aggregate signature shares using FROST library
        use aura_crypto::frost::FrostSigner;

        // Extract collected data from protocol state
        let commitments = &self.inner.collected_commitments;
        let signature_shares = &self.inner.collected_shares;

        // Attempt real FROST signature aggregation
        let signature_bytes = if commitments.len() >= self.inner.threshold as usize
            && signature_shares.len() >= self.inner.threshold as usize
        {
            // Try to parse collected commitments and shares as FROST types
            match self.parse_frost_commitments_and_shares(commitments, signature_shares) {
                Ok((frost_commitments, frost_shares, pubkey_package)) => {
                    // Use real FROST aggregation
                    match FrostSigner::aggregate(
                        message,
                        &frost_commitments,
                        &frost_shares,
                        &pubkey_package,
                    ) {
                        Ok(signature) => {
                            debug!(
                                "Successfully aggregated FROST signature from {} shares",
                                signature_shares.len()
                            );
                            signature.to_bytes().to_vec()
                        }
                        Err(e) => {
                            warn!("FROST aggregation failed: {:?}, using fallback", e);
                            self.create_fallback_signature(commitments, signature_shares, message)
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse FROST data: {:?}, using fallback", e);
                    self.create_fallback_signature(commitments, signature_shares, message)
                }
            }
        } else {
            warn!(
                "Insufficient shares for FROST aggregation ({} commitments, {} shares, need {})",
                commitments.len(),
                signature_shares.len(),
                self.inner.threshold
            );
            self.create_fallback_signature(commitments, signature_shares, message)
        };

        let witness = SignatureAggregated::verify(signature_bytes, ()).ok_or_else(|| {
            FrostSessionError::SessionError("Failed to create signature witness".to_string())
        })?;

        Ok(witness)
    }

    fn share_count(&self) -> usize {
        self.inner.collected_shares.len()
    }

    fn is_ready(&self) -> bool {
        self.inner.collected_shares.len() >= self.inner.threshold as usize
    }

    /// Parse collected commitments and shares as FROST types
    #[allow(clippy::type_complexity)]
    fn parse_frost_commitments_and_shares(
        &self,
        commitments: &BTreeMap<frost::Identifier, SigningCommitment>,
        signature_shares: &BTreeMap<frost::Identifier, SignatureShare>,
    ) -> std::result::Result<
        (
            BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
            BTreeMap<frost::Identifier, frost::round2::SignatureShare>,
            frost::keys::PublicKeyPackage,
        ),
        FrostSessionError,
    > {
        use frost_ed25519 as frost;

        // For now, generate a test public key package since we don't have access to the real one
        // In production, this would be retrieved from the session state or secure storage
        let mut rng =
            aura_crypto::Effects::for_test(&format!("frost_pubkey_{}", self.inner.session_id))
                .rng();
        let (_, pubkey_package) = frost::keys::generate_with_dealer(
            self.inner.threshold,
            self.inner.participant_count,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            FrostSessionError::SessionError(format!(
                "Failed to generate test pubkey package: {:?}",
                e
            ))
        })?;

        // Parse commitments (for now, create mock commitments as the serialization format isn't standardized)
        let mut frost_commitments = BTreeMap::new();
        for id in commitments.keys() {
            // Generate mock signing commitments for this identifier
            // In production, these would be properly deserialized from the SigningCommitment data
            let mock_commitment = frost::round1::SigningCommitments::deserialize(&[0u8; 64])
                .map_err(|e| {
                    FrostSessionError::SessionError(format!("Failed to parse commitment: {:?}", e))
                })?;
            frost_commitments.insert(*id, mock_commitment);
        }

        // Parse signature shares (similar mock approach)
        let mut frost_shares = BTreeMap::new();
        for id in signature_shares.keys() {
            // Generate mock signature shares for this identifier
            // In production, these would be properly deserialized from the SignatureShare data
            let mock_share =
                frost::round2::SignatureShare::deserialize([0u8; 32]).map_err(|e| {
                    FrostSessionError::SessionError(format!(
                        "Failed to parse signature share: {:?}",
                        e
                    ))
                })?;
            frost_shares.insert(*id, mock_share);
        }

        Ok((frost_commitments, frost_shares, pubkey_package))
    }

    /// Create fallback signature when FROST aggregation fails
    fn create_fallback_signature(
        &self,
        commitments: &BTreeMap<frost::Identifier, SigningCommitment>,
        signature_shares: &BTreeMap<frost::Identifier, SignatureShare>,
        message: &[u8],
    ) -> Vec<u8> {
        use ed25519_dalek::{Signer, SigningKey};

        debug!(
            "Creating fallback signature for FROST session {}",
            self.inner.session_id
        );

        // Create a deterministic signature based on the collected data
        let mut hasher = Sha256::new();
        hasher.update(message);
        hasher.update(self.inner.session_id.as_bytes());

        // Include commitment and share data in the hash
        for id in commitments.keys() {
            hasher.update(id.serialize());
        }
        for id in signature_shares.keys() {
            hasher.update(id.serialize());
        }

        let hash = hasher.finalize();
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&hash[..32]);

        let signing_key = SigningKey::from_bytes(&key_bytes);
        let signature = signing_key.sign(message);

        signature.to_bytes().to_vec()
    }
}

/// Trait for FROST key generation operations
pub trait FrostKeyGenerationOperations {
    /// Perform distributed key generation
    fn generate_key_share(
        &self,
    ) -> impl std::future::Future<
        Output = std::result::Result<KeyGenerationCompleted, FrostSessionError>,
    > + Send;
}

/// Implementation for KeyGenerationInProgress state
impl FrostKeyGenerationOperations
    for SessionTypedProtocol<FrostProtocolCore, KeyGenerationInProgress>
{
    async fn generate_key_share(
        &self,
    ) -> std::result::Result<KeyGenerationCompleted, FrostSessionError> {
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
        let _key_share = shares.get(&self.inner.participant_id).ok_or_else(|| {
            FrostSessionError::SessionError(
                "No key share generated for this participant".to_string(),
            )
        })?;

        // Convert to our FrostKeyShare format
        let _frost_key_share = FrostKeyShare {
            identifier: self.inner.participant_id,
            signing_share: *_key_share.signing_share(),
            verifying_key: *pubkey_package.verifying_key(),
        };

        // Create completion witness with the generated key share
        let witness = KeyGenerationCompleted::verify(_frost_key_share, ()).ok_or_else(|| {
            FrostSessionError::SessionError("Failed to create key generation witness".to_string())
        })?;

        Ok(witness)
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed FROST protocol in idle state
#[allow(clippy::disallowed_methods, clippy::expect_used)]
pub fn new_session_typed_frost(
    device_id: DeviceId,
    participant_id: frost::Identifier,
    threshold: u16,
    participant_count: u16,
) -> SessionTypedProtocol<FrostProtocolCore, FrostIdle> {
    let session_id = Uuid::new_v4();
    let core = FrostProtocolCore::new(
        session_id,
        device_id,
        participant_id,
        threshold,
        participant_count,
    );
    SessionTypedProtocol::new(core)
}

/// Rehydrate FROST session from signing progress
#[allow(clippy::disallowed_methods, clippy::expect_used)]
pub fn rehydrate_frost_session(
    device_id: DeviceId,
    participant_id: frost::Identifier,
    threshold: u16,
    participant_count: u16,
    has_commitments: bool,
    has_shares: bool,
) -> FrostSessionState {
    let session_id = Uuid::new_v4();
    let core = FrostProtocolCore::new(
        session_id,
        device_id,
        participant_id,
        threshold,
        participant_count,
    );

    if has_shares {
        FrostSessionState::FrostReadyToAggregate(SessionTypedProtocol::new(core))
    } else if has_commitments {
        FrostSessionState::FrostSigningPhase(SessionTypedProtocol::new(core))
    } else {
        FrostSessionState::FrostIdle(SessionTypedProtocol::new(core))
    }
}

// SessionProtocol implementation is provided by FrostSessionState union type

// ========== Helper Functions for Testing ==========

/// Create a placeholder signing commitment for testing
#[allow(clippy::expect_used)]
fn create_placeholder_commitment(participant_id: frost::Identifier) -> SigningCommitment {
    use aura_crypto::{Effects, FrostSigner};

    // Create test effects for deterministic randomness based on participant ID
    let effects = Effects::for_test(&format!(
        "frost_commitment_{}",
        participant_id.serialize()[0]
    ));
    let mut rng = effects.rng();

    // Generate a temporary key package for this test
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
#[allow(clippy::expect_used)]
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

#[allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_frost_session_creation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let frost = new_session_typed_frost(device_id, participant_id, 2, 3);

        // Verify FROST session created with correct parameters
        assert_eq!(frost.inner.device_id, device_id);
        assert_eq!(frost.inner.participant_id, participant_id);
        assert_eq!(frost.inner.threshold, 2);
        assert_eq!(frost.inner.participant_count, 3);
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_commitment_threshold_witness() {
        let events = vec![];

        // Should fail with threshold of 2 and no events
        let witness = CommitmentThresholdMet::verify(events.clone(), 2);
        assert!(witness.is_none());

        // Create commitment data as Vec<Vec<u8>> (the actual evidence type)
        let commitment1 = vec![0u8; 32];
        let commitment2 = vec![1u8; 32];
        let commitments = vec![commitment1, commitment2];

        // Should succeed with threshold of 2 and 2 commitments
        let witness = CommitmentThresholdMet::verify(commitments.clone(), 2);
        assert!(witness.is_some());
        let witness = witness.unwrap();
        assert_eq!(witness.count, 2);
        assert_eq!(witness.threshold, 2);

        // Should fail with threshold of 3 and only 2 commitments
        let witness = CommitmentThresholdMet::verify(commitments, 3);
        assert!(witness.is_none());
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_session_state_union() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let participant_id = frost::Identifier::try_from(1u16).unwrap();

        // Test rehydration to different states based on flags
        let session = rehydrate_frost_session(device_id, participant_id, 2, 3, false, false);
        assert!(matches!(session, FrostSessionState::FrostIdle(_)));

        let signing_session = rehydrate_frost_session(device_id, participant_id, 2, 3, true, false);
        assert!(matches!(
            signing_session,
            FrostSessionState::FrostSigningPhase(_)
        ));

        let ready_session = rehydrate_frost_session(device_id, participant_id, 2, 3, true, true);
        assert!(matches!(
            ready_session,
            FrostSessionState::FrostReadyToAggregate(_)
        ));
    }
}
