//! Recovery protocol lifecycle adapter using unified protocol-core traits.

use crate::protocol_results::RecoveryProtocolResult;
use aura_journal::SessionId as JournalSessionId;
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
use uuid::Uuid;

/// Error surfaced by the recovery lifecycle adapter.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryLifecycleError {
    #[error("unsupported input for recovery lifecycle: {0}")]
    Unsupported(&'static str),
}

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

        Self {
            descriptor,
            state: RecoveryLifecycleState,
            finished: false,
            output: Some(RecoveryProtocolResult {
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
            }),
        }
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
