//! FROST protocol lifecycle (stub implementation)
//!
//! TODO: Implement proper distributed FROST signing protocol with:
//! 1. Round 1: All participants generate and broadcast commitments
//! 2. Round 2: Participants create and broadcast signature shares
//! 3. Aggregation: Any participant can aggregate shares into final signature
//!
//! Current implementation is a simplified stub using local operations for testing.

use crate::core::{
    capabilities::ProtocolCapabilities,
    lifecycle::{ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolStep},
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use aura_crypto::frost::FrostKeyShare;
use aura_messages::FrostSigningResult;
use aura_types::{AuraError, DeviceId, SessionId};
use frost_ed25519 as frost;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Typestate marker for the FROST signing lifecycle
#[derive(Debug, Clone)]
pub struct FrostSigningLifecycleState;

impl SessionState for FrostSigningLifecycleState {
    const NAME: &'static str = "FrostSigningLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// FROST signing lifecycle (stub implementation)
pub struct FrostSigningLifecycle {
    descriptor: ProtocolDescriptor,
    key_share: FrostKeyShare,
    key_package: frost::keys::KeyPackage,
    pubkey_package: frost::keys::PublicKeyPackage,
    participants: Vec<DeviceId>,
    threshold: u16,
    finished: bool,
    result: Option<FrostSigningResult>,
}

impl FrostSigningLifecycle {
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        participants: Vec<DeviceId>,
        key_share: FrostKeyShare,
        key_package: frost::keys::KeyPackage,
        pubkey_package: frost::keys::PublicKeyPackage,
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

        Self {
            descriptor,
            key_share,
            key_package,
            pubkey_package,
            participants,
            threshold,
            finished: false,
            result: None,
        }
    }

    /// Execute simplified local signing (for testing)
    pub fn execute_local(&mut self, message: &[u8], _caps: &mut ProtocolCapabilities) -> Result<(), AuraError> {
        // Simplified threshold signing using all participants locally
        let mut rng = rand::rngs::OsRng;

        // Create key packages map for threshold participants
        let mut key_packages = BTreeMap::new();
        key_packages.insert(self.key_share.identifier, self.key_package.clone());

        // Perform local threshold signing
        let signature = aura_crypto::frost::FrostSigner::threshold_sign(
            message,
            &key_packages,
            &self.pubkey_package,
            self.threshold,
            &mut rng,
        )
        .map_err(|e| AuraError::coordination_failed(&format!("FROST signing failed: {:?}", e)))?;

        let sig_bytes = signature.to_bytes();

        // Create result
        let result = FrostSigningResult {
            session_id: self.descriptor.session_id,
            signature: sig_bytes.to_vec(),
            message: message.to_vec(),
            signing_participants: self.participants.clone(),
            verification: aura_messages::FrostSignatureVerification {
                is_valid: true,
                verification_details: vec![],
                group_verification: true,
            },
        };

        self.result = Some(result);
        self.finished = true;

        Ok(())
    }
}

impl ProtocolLifecycle for FrostSigningLifecycle {
    type State = FrostSigningLifecycleState;
    type Output = FrostSigningResult;
    type Error = AuraError;

    fn step(
        &mut self,
        _input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        // Stub implementation - return result if available
        if let Some(result) = &self.result {
            ProtocolStep::completed(vec![], None, Ok(result.clone()))
        } else {
            ProtocolStep::progress(vec![], None)
        }
    }

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

// ========== Error Type ==========
pub type FrostLifecycleError = AuraError;
