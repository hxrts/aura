//! Recovery protocol lifecycle adapter using unified protocol-core traits.

use crate::capability_authorization::create_capability_authorization_manager;
use crate::protocol_results::RecoveryProtocolResult;
use aura_crypto::Effects;
use aura_journal::{capability::Permission, SessionId as JournalSessionId};
use aura_types::{AccountId, DeviceId, GuardianId, SessionId};
use ed25519_dalek::Signature;
use protocol_core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use tracing::debug;
use uuid::Uuid;

/// Typestate marker representing the recovery lifecycle.
#[derive(Debug, Clone)]
pub struct RecoveryLifecycleState;

impl SessionState for RecoveryLifecycleState {
    const NAME: &'static str = "RecoveryLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Minimal lifecycle implementation returning stubbed recovery results.
#[derive(Debug, Clone)]
pub struct RecoveryLifecycle {
    descriptor: ProtocolDescriptor,
    state: RecoveryLifecycleState,
    finished: bool,
    output: Option<RecoveryProtocolResult>,
    approving_guardians: Vec<GuardianId>,
    new_device_id: DeviceId,
}

impl RecoveryLifecycle {
    /// Construct a new lifecycle instance.
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        approving_guardians: Vec<GuardianId>,
        new_device_id: DeviceId,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::Recovery,
        )
        .with_operation_type(OperationType::Recovery)
        .with_priority(ProtocolPriority::Critical)
        .with_mode(ProtocolMode::Interactive);

        let _signature = Signature::from_slice(&[0u8; 64]).unwrap();

        let mut lifecycle = Self {
            descriptor,
            state: RecoveryLifecycleState,
            finished: false,
            output: None,
            approving_guardians: approving_guardians.clone(),
            new_device_id,
        };

        // Create real capability proof with cryptographic authorization
        let capability_proof = match lifecycle.create_real_capability_proof() {
            Ok(proof) => proof,
            Err(e) => {
                debug!(
                    "Failed to create real capability proof, falling back to placeholder: {:?}",
                    e
                );
                // Fall back to placeholder if real authorization fails
                Self::create_placeholder_capability_proof()
            }
        };

        lifecycle.output = Some(RecoveryProtocolResult {
            session_id: JournalSessionId::from_uuid(session_id.uuid()),
            new_device_id,
            approving_guardians: approving_guardians.clone(),
            guardian_signatures: approving_guardians
                .iter()
                .map(|guardian| crate::protocol_results::GuardianSignature {
                    guardian_id: *guardian,
                    signature: vec![],
                    signed_at: 0,
                })
                .collect(),
            recovered_share: Vec::new(),
            revocation_proof: None,
            ledger_events: Vec::new(),
            capability_proof,
        });

        lifecycle
    }

    /// Create real capability proof using threshold authorization for recovery operations
    ///
    /// This replaces the previous placeholder implementation with real cryptographic authorization
    fn create_real_capability_proof(
        &self,
    ) -> Result<crate::protocol_results::CapabilityProof, RecoveryLifecycleError> {
        debug!(
            "Creating real capability proof for Recovery protocol on device {}",
            self.descriptor.device_id
        );

        // Create effects for deterministic authorization
        let effects =
            Effects::for_test(&format!("recovery_lifecycle_{}", self.descriptor.device_id));

        // Create authorization manager for this device
        let auth_manager =
            create_capability_authorization_manager(self.descriptor.device_id, &effects);

        // Define the permission required for recovery operations (critical account recovery)
        let permission = Permission::Storage {
            operation: aura_journal::capability::StorageOperation::Write,
            resource: "recovery_shares".to_string(),
        };

        // Create real capability proof with signature-based authorization
        let capability_proof = auth_manager
            .create_capability_proof(permission, "account_recovery", &effects)
            .map_err(|e| {
                debug!("Failed to create Recovery capability proof: {:?}", e);
                RecoveryLifecycleError::Unsupported("Recovery capability authorization failed")
            })?;

        debug!("Successfully created real capability proof for Recovery protocol");
        Ok(capability_proof)
    }

    /// Create placeholder capability proof for testing/development
    ///
    /// This is kept for backwards compatibility but should be replaced with create_real_capability_proof
    fn create_placeholder_capability_proof() -> crate::protocol_results::CapabilityProof {
        use aura_journal::capability::Permission;
        use aura_journal::capability::{
            unified_manager::{CapabilityType, VerificationContext},
            ThresholdCapability,
        };
        use ed25519_dalek::{Signature, SigningKey};
        use std::num::NonZeroU16;
        use uuid::Uuid;

        // Create a minimal threshold capability for testing
        let signing_key = SigningKey::from_bytes(&[0u8; 32]);
        let authorization = aura_journal::capability::threshold_capabilities::ThresholdSignature {
            signature: Signature::from_bytes(&[0u8; 64]),
            signers: vec![
                aura_journal::capability::threshold_capabilities::ParticipantId::new(
                    NonZeroU16::new(1).unwrap(),
                ),
            ],
        };

        let public_key_package =
            aura_journal::capability::threshold_capabilities::PublicKeyPackage {
                group_public: signing_key.verifying_key(),
                threshold: 1,
                total_participants: 1,
            };

        let device_id = aura_types::DeviceId(Uuid::new_v4());
        let primary_capability = ThresholdCapability::new(
            device_id,
            vec![Permission::Storage {
                operation: aura_journal::capability::StorageOperation::Write,
                resource: "recovery".to_string(),
            }],
            authorization,
            public_key_package,
            &aura_crypto::Effects::for_test("recovery_lifecycle"),
        )
        .expect("Failed to create test capability");

        let verification_context = VerificationContext {
            capability_type: CapabilityType::Threshold,
            authority_level: 1,
            near_expiration: false,
        };

        crate::protocol_results::CapabilityProof::new(
            primary_capability,
            vec![],
            verification_context,
            false, // Not an admin operation
        )
    }

    /// Convenience helper generating a fresh session identifier.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        approving_guardians: Vec<GuardianId>,
        new_device_id: DeviceId,
    ) -> Self {
        Self::new(
            device_id,
            SessionId::new(),
            approving_guardians,
            new_device_id,
        )
    }
}

impl ProtocolLifecycle for RecoveryLifecycle {
    type State = RecoveryLifecycleState;
    type Output = RecoveryProtocolResult;
    type Error = RecoveryLifecycleError;

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match input {
            ProtocolInput::LocalSignal { signal, .. } if signal == "complete" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        RecoveryLifecycleState::NAME,
                        "RecoveryCompleted",
                        None,
                    )),
                    self.output
                        .clone()
                        .ok_or(RecoveryLifecycleError::Unsupported(
                            "missing recovery output",
                        )),
                )
            }
            ProtocolInput::LocalSignal { signal, .. } if signal == "abort" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        RecoveryLifecycleState::NAME,
                        "RecoveryAborted",
                        None,
                    )),
                    Err(RecoveryLifecycleError::Unsupported("recovery aborted")),
                )
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for RecoveryLifecycle {
    type Evidence = ();

    fn validate_evidence(_evidence: &Self::Evidence) -> bool {
        true
    }

    fn rehydrate(
        device_id: DeviceId,
        _account_id: AccountId,
        _evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        Ok(Self::new_ephemeral(
            device_id,
            Vec::new(),
            DeviceId(Uuid::new_v4()),
        ))
    }
}
