//! FROST protocol lifecycle adapter using the unified protocol core traits.
//!
//! This implements FROST threshold signatures (DKG and signing) using the
//! session-typed protocol architecture consistent with other coordination protocols.

use crate::core::{
    lifecycle::ProtocolDescriptor,
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::{ParticipantId, ThresholdSignature};
use aura_crypto::frost::FrostKeyShare;
use aura_crypto::{Ed25519Signature, Effects};
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AuraError, DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use uuid::Uuid;

/// Protocol result for FROST DKG operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostDkgResult {
    pub session_id: JournalSessionId,
    pub key_share: FrostKeyShare,
    pub threshold: u16,
    pub participants: Vec<DeviceId>,
    pub capability_proof: crate::protocol_results::CapabilityProof,
}

/// Protocol result for FROST signing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSigningResult {
    pub session_id: JournalSessionId,
    pub signature: Ed25519Signature,
    pub message: Vec<u8>,
    pub threshold_signature: ThresholdSignature,
    pub participants: Vec<DeviceId>,
}

// ========== FROST DKG Lifecycle ==========

/// Typestate marker for the FROST DKG lifecycle
#[derive(Debug, Clone)]
pub struct FrostDkgLifecycleState;

impl SessionState for FrostDkgLifecycleState {
    const NAME: &'static str = "FrostDkgLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST DKG lifecycle implementation
#[derive(Debug, Clone)]
pub struct FrostDkgLifecycle {
    descriptor: ProtocolDescriptor,
    state: FrostDkgLifecycleState,
    finished: bool,
    output: Option<FrostDkgResult>,
    participants: Vec<DeviceId>,
    threshold: u16,
}

impl FrostDkgLifecycle {
    /// Create a new FROST DKG lifecycle instance
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        participants: Vec<DeviceId>,
        threshold: u16,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::FrostDkg,
        )
        .with_operation_type(OperationType::Dkd)
        .with_priority(ProtocolPriority::High)
        .with_mode(ProtocolMode::Interactive);

        info!(
            "Creating FROST DKG lifecycle for device {} with {}-of-{} threshold",
            device_id,
            threshold,
            participants.len()
        );

        Self {
            descriptor,
            state: FrostDkgLifecycleState,
            finished: false,
            output: None,
            participants,
            threshold,
        }
    }

    /// Execute the DKG protocol (simplified for MVP)
    pub fn execute(&mut self, effects: &Effects) -> Result<(), AuraError> {
        debug!("Executing FROST DKG protocol");

        // Generate FROST key shares using trusted dealer via FrostKeyManager
        // NOTE: True distributed DKG would be implemented here using the session-typed
        // protocol states from crates/coordination/src/session_types/frost.rs
        use crate::protocols::FrostKeyManager;

        let key_manager = FrostKeyManager::new(self.descriptor.device_id, effects);
        let (key_share, _pubkey_package) =
            key_manager.generate_key_share(self.threshold, &self.participants)?;

        // Create capability proof
        let capability_proof = self.create_capability_proof(effects)?;

        self.output = Some(FrostDkgResult {
            session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
            key_share,
            threshold: self.threshold,
            participants: self.participants.clone(),
            capability_proof,
        });

        self.finished = true;
        info!("FROST DKG completed successfully");

        Ok(())
    }

    fn create_capability_proof(
        &self,
        _effects: &Effects,
    ) -> Result<crate::protocol_results::CapabilityProof, AuraError> {
        use crate::protocols::CapabilityProofBuilder;
        CapabilityProofBuilder::new(self.descriptor.device_id, "frost")
            .create_proof("frost_key_shares", "frost_dkg")
    }

    pub fn get_result(&self) -> Option<&FrostDkgResult> {
        self.output.as_ref()
    }
}

// ========== FROST Signing Lifecycle ==========

/// Typestate marker for the FROST signing lifecycle
#[derive(Debug, Clone)]
pub struct FrostSigningLifecycleState;

impl SessionState for FrostSigningLifecycleState {
    const NAME: &'static str = "FrostSigningLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST signing lifecycle implementation
#[derive(Debug, Clone)]
pub struct FrostSigningLifecycle {
    descriptor: ProtocolDescriptor,
    state: FrostSigningLifecycleState,
    finished: bool,
    output: Option<FrostSigningResult>,
    participants: Vec<DeviceId>,
    message: Vec<u8>,
    key_share: FrostKeyShare,
    threshold: u16,
}

impl FrostSigningLifecycle {
    /// Create a new FROST signing lifecycle instance
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        participants: Vec<DeviceId>,
        message: Vec<u8>,
        key_share: FrostKeyShare,
        threshold: u16,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::FrostSigning,
        )
        .with_operation_type(OperationType::Signing)
        .with_priority(ProtocolPriority::High)
        .with_mode(ProtocolMode::Interactive);

        info!(
            "Creating FROST signing lifecycle for device {} with {}-of-{} threshold",
            device_id,
            threshold,
            participants.len()
        );

        Self {
            descriptor,
            state: FrostSigningLifecycleState,
            finished: false,
            output: None,
            participants,
            message,
            key_share,
            threshold,
        }
    }

    /// Execute the signing protocol (simplified for MVP)
    pub fn execute(&mut self, effects: &Effects) -> Result<(), AuraError> {
        debug!("Executing FROST signing protocol");

        // Execute FROST signing ceremony using FrostKeyManager
        // NOTE: True distributed signing would be implemented here using the session-typed
        // protocol states from crates/coordination/src/session_types/frost.rs
        use crate::protocols::FrostKeyManager;

        let key_manager = FrostKeyManager::new(self.descriptor.device_id, effects);
        let (key_packages, pubkey_package) =
            key_manager.generate_signing_keys(self.threshold, self.participants.len() as u16)?;

        // Perform threshold signing
        let mut rng = effects.rng();
        let signature = aura_crypto::frost::FrostSigner::threshold_sign(
            &self.message,
            &key_packages,
            &pubkey_package,
            self.threshold,
            &mut rng,
        )
        .map_err(|e| {
            AuraError::protocol_invalid_instruction(&format!("Signing failed: {:?}", e))
        })?;

        // Convert to Ed25519 signature
        let ed25519_sig = Ed25519Signature::from_bytes(&signature.to_bytes());

        // Create threshold signature metadata
        let threshold_sig = ThresholdSignature {
            signature: ed25519_sig.clone(),
            signers: self
                .participants
                .iter()
                .enumerate()
                .filter_map(|(i, _)| {
                    std::num::NonZeroU16::new((i + 1) as u16).map(ParticipantId::new)
                })
                .collect(),
        };

        self.output = Some(FrostSigningResult {
            session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
            signature: ed25519_sig,
            message: self.message.clone(),
            threshold_signature: threshold_sig,
            participants: self.participants.clone(),
        });

        self.finished = true;
        info!("FROST signing completed successfully");

        Ok(())
    }

    pub fn get_result(&self) -> Option<&FrostSigningResult> {
        self.output.as_ref()
    }
}

// ========== Error Type ==========
pub type FrostLifecycleError = AuraError;
