//! Session Type States for FROST Cryptographic Protocol (Refactored with Macros)
//!
//! This module defines session types for FROST threshold signature operations,
//! providing compile-time safety for the signing protocol state machine.

use crate::{
    core::{ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition},
    define_protocol,
    witnesses::{
        CommitmentThresholdMet, FrostProtocolFailure, FrostResharingCompleted,
        KeyGenerationCompleted, RuntimeWitness, SignatureAggregated, SignatureShareThresholdMet,
    },
};
use aura_crypto::{CryptoError, FrostKeyShare, SignatureShare, SigningCommitment};
use frost_ed25519 as frost;
use std::collections::BTreeMap;
use uuid::Uuid;

// ========== FROST Protocol Core ==========

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

// Manual implementations for FrostProtocolCore
impl std::fmt::Debug for FrostProtocolCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrostProtocolCore")
            .field("session_id", &self.session_id)
            .field("device_id", &self.device_id)
            .field("participant_id", &self.participant_id)
            .field("has_key_share", &self.key_share.is_some())
            .field("has_message", &self.message_to_sign.is_some())
            .field("collected_commitments_count", &self.collected_commitments.len())
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

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: FrostProtocol,
    Core: FrostProtocolCore,
    Error: FrostSessionError,
    Union: FrostSessionState,

    States {
        FrostIdle => (),
        FrostCommitmentPhase => SigningCommitment,
        FrostAwaitingCommitments => (),
        FrostSigningPhase => SignatureShare,
        FrostAwaitingShares => (),
        FrostReadyToAggregate => ed25519_dalek::Signature,
        FrostSignatureComplete @ final => ed25519_dalek::Signature,
        FrostSigningFailed @ final => (),
        KeyGenerationInitializing => (),
        KeyGenerationInProgress => (),
        KeyGenerationComplete @ final => FrostKeyShare,
        ResharingInitializing => (),
        ResharingPhaseOne => (),
        ResharingPhaseTwo => (),
        ResharingComplete @ final => FrostKeyShare,
    }

    Extract {
        session_id: |core| core.session_id,
        device_id: |core| core.device_id,
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed FROST protocol wrapper
pub type SessionTypedFrost<S> = ChoreographicProtocol<FrostProtocolCore, S>;

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

impl<S: SessionState> ChoreographicProtocol<FrostProtocolCore, S> {
    /// Get reference to the protocol core
    pub fn core(&self) -> &FrostProtocolCore {
        &self.inner
    }

    /// Get the participant ID
    pub fn participant_id(&self) -> frost::Identifier {
        self.core().participant_id
    }

    /// Get the threshold
    pub fn threshold(&self) -> u16 {
        self.core().threshold
    }

    /// Get the participant count
    pub fn participant_count(&self) -> u16 {
        self.core().participant_count
    }
}

// ========== State Transitions ==========

/// Transition from FrostIdle to FrostCommitmentPhase (when starting signing)
impl WitnessedTransition<FrostIdle, FrostCommitmentPhase>
    for ChoreographicProtocol<FrostProtocolCore, FrostIdle>
{
    type Witness = FrostSigningContext;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostCommitmentPhase>;

    /// Begin FROST signing protocol
    fn transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(mut self, commitment: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Note: The witness only contains count and threshold, 
        // actual commitments should be handled separately in the protocol implementation
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
    fn transition_with_witness(mut self, share: Self::Witness) -> Self::Target {
        self.inner.collected_shares.insert(share.identifier, share);
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Note: The witness only contains count and threshold, 
        // actual shares should be handled separately in the protocol implementation
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(mut self, config: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        // Note: The witness only contains key_id, actual key_share should be
        // handled separately in the protocol implementation
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
    fn transition_with_witness(mut self, config: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
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
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        // Update threshold based on witness
        self.inner.threshold = witness.new_threshold as u16;
        self.transition_to()
    }
}

/// Transition to FrostSigningFailed from any state (requires FrostProtocolFailure witness)
impl<S: SessionState> WitnessedTransition<S, FrostSigningFailed>
    for ChoreographicProtocol<FrostProtocolCore, S>
where
    Self: SessionProtocol<State = S, Error = FrostSessionError>,
{
    type Witness = FrostProtocolFailure;
    type Target = ChoreographicProtocol<FrostProtocolCore, FrostSigningFailed>;

    /// Handle protocol failure
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
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

        let _message = self
            .inner
            .message_to_sign
            .as_ref()
            .ok_or_else(|| FrostSessionError::InvalidState("No message to sign".to_string()))?;

        // In reality, this would aggregate using FROST library
        let signature_bytes = vec![0u8; 64]; // Placeholder signature

        let witness = SignatureAggregated::verify(signature_bytes, ()).ok_or_else(|| {
            FrostSessionError::SessionError("Failed to create signature witness".to_string())
        })?;

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
        let _key_share = shares.get(&self.inner.participant_id).ok_or_else(|| {
            FrostSessionError::SessionError(
                "No key share generated for this participant".to_string(),
            )
        })?;

        // Convert to our FrostKeyShare format
        let _frost_key_share = FrostKeyShare {
            identifier: self.inner.participant_id,
            signing_share: _key_share.signing_share().clone(),
            verifying_key: pubkey_package.verifying_key().clone(),
        };

        // Create completion witness using the correct signature
        let witness =
            KeyGenerationCompleted::verify(self.inner.session_id, ()).ok_or_else(|| {
                FrostSessionError::SessionError(
                    "Failed to create key generation witness".to_string(),
                )
            })?;

        Ok(witness)
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
    let core = FrostProtocolCore::new(
        session_id,
        device_id,
        participant_id,
        threshold,
        participant_count,
    );
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
    let core = FrostProtocolCore::new(
        session_id,
        device_id,
        participant_id,
        threshold,
        participant_count,
    );

    if has_shares {
        FrostSessionState::FrostReadyToAggregate(ChoreographicProtocol::new(core))
    } else if has_commitments {
        FrostSessionState::FrostSigningPhase(ChoreographicProtocol::new(core))
    } else {
        FrostSessionState::FrostIdle(ChoreographicProtocol::new(core))
    }
}

// ========== Helper Functions for Testing ==========

/// Create a placeholder signing commitment for testing
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;

    #[test]
    fn test_frost_session_creation() {
        let effects = Effects::test();
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let frost = new_session_typed_frost(device_id, participant_id, 2, 3);

        assert_eq!(frost.state_name(), "FrostIdle");
        assert!(frost.can_terminate());
    }

    #[test]
    fn test_signing_flow_transitions() {
        let effects = Effects::test();
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
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
        assert_eq!(commitment_phase.state_name(), "FrostCommitmentPhase");

        // Generate commitment
        let commitment = SigningCommitment {
            identifier: participant_id,
            commitment: frost::round1::SigningCommitments::default(),
        };

        let awaiting = commitment_phase.transition_with_witness(commitment);
        assert_eq!(awaiting.state_name(), "FrostAwaitingCommitments");

        // Simulate threshold met
        let events = vec![];
        let witness = CommitmentThresholdMet::verify(events, 2).unwrap();
        let signing_phase = awaiting.transition_with_witness(witness);
        assert_eq!(signing_phase.state_name(), "FrostSigningPhase");
    }

    #[test]
    fn test_commitment_threshold_witness() {
        let events = vec![];

        // Should fail with threshold of 2 and no events
        let witness = CommitmentThresholdMet::verify(events.clone(), 2);
        assert!(witness.is_none());

        // Create some dummy events
        let events_with_data = vec![
            aura_journal::Event::default(),
            aura_journal::Event::default(),
        ];

        // Should succeed with threshold of 2 and 2 events
        let witness = CommitmentThresholdMet::verify(events_with_data.clone(), 2);
        assert!(witness.is_some());

        // Should fail with threshold of 3 and only 2 events
        let witness = CommitmentThresholdMet::verify(events_with_data, 3);
        assert!(witness.is_none());
    }

    #[test]
    fn test_session_state_union() {
        let effects = Effects::test();
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
        let participant_id = frost::Identifier::try_from(1u16).unwrap();
        let session = rehydrate_frost_session(device_id, participant_id, 2, 3, false, false);

        assert_eq!(session.state_name(), "FrostIdle");
        assert!(session.can_terminate());

        let signing_session = rehydrate_frost_session(device_id, participant_id, 2, 3, true, false);
        assert_eq!(signing_session.state_name(), "FrostSigningPhase");

        let ready_session = rehydrate_frost_session(device_id, participant_id, 2, 3, true, true);
        assert_eq!(ready_session.state_name(), "FrostReadyToAggregate");
    }
}
