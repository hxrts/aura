//! Recovery protocol lifecycle adapter using unified protocol-core traits.

use super::RecoveryLifecycleError;
use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::protocol_results::RecoveryProtocolResult;
use aura_crypto::Ed25519Signature;
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AccountId, AuraError, DeviceId, GuardianId, SessionId};
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

        let _signature = Ed25519Signature::default();

        let mut lifecycle = Self {
            descriptor,
            state: RecoveryLifecycleState,
            finished: false,
            output: None,
            approving_guardians: approving_guardians.clone(),
            new_device_id,
        };

        // Create capability proof using unified builder
        use crate::protocols::CapabilityProofBuilder;
        let capability_proof = CapabilityProofBuilder::new(device_id, "recovery")
            .create_proof("recovery_shares", "account_recovery")
            .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

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
                        .ok_or(AuraError::agent_invalid_state("missing recovery output")),
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
                    Err(AuraError::session_aborted("recovery aborted")),
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
