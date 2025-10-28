//! Locking protocol lifecycle adapter built on protocol-core traits.

use crate::{protocol_results::LockingProtocolResult, ParticipantId, ThresholdSignature};
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AccountId, DeviceId, SessionId};
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

/// Error type for the locking lifecycle adapter.
#[derive(Debug, thiserror::Error)]
pub enum LockingLifecycleError {
    #[error("unsupported input for locking lifecycle: {0}")]
    Unsupported(&'static str),
}

/// Typestate marker representing the locking lifecycle.
#[derive(Debug, Clone)]
pub struct LockingLifecycleState;

impl SessionState for LockingLifecycleState {
    const NAME: &'static str = "LockingLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Minimal locking lifecycle implementation bridging to protocol-core.
#[derive(Debug, Clone)]
pub struct LockingLifecycle {
    descriptor: ProtocolDescriptor,
    state: LockingLifecycleState,
    operation_type: aura_journal::OperationType,
    contenders: Vec<DeviceId>,
    finished: bool,
    outcome: Option<LockingProtocolResult>,
}

impl LockingLifecycle {
    /// Construct a new locking lifecycle instance.
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        operation_type: aura_journal::OperationType,
        contenders: Vec<DeviceId>,
    ) -> Self {
        let descriptor =
            ProtocolDescriptor::new(Uuid::new_v4(), session_id, device_id, ProtocolType::Locking)
                .with_operation_type(OperationType::Locking)
                .with_priority(ProtocolPriority::High)
                .with_mode(ProtocolMode::Interactive);

        Self {
            descriptor,
            state: LockingLifecycleState,
            operation_type,
            contenders,
            finished: false,
            outcome: None,
        }
    }

    /// Convenience constructor for ephemeral sessions with auto-generated session ids.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        operation_type: aura_journal::OperationType,
        contenders: Vec<DeviceId>,
    ) -> Self {
        Self::new(device_id, SessionId::new(), operation_type, contenders)
    }

    fn complete(&mut self) -> Result<LockingProtocolResult, LockingLifecycleError> {
        let session_id = self.descriptor.session_id;
        let winner = self
            .contenders
            .first()
            .cloned()
            .unwrap_or(self.descriptor.device_id);

        let signature = Signature::from_bytes(&[0u8; 64]);

        let threshold_signature = ThresholdSignature {
            signature,
            signers: self
                .contenders
                .iter()
                .enumerate()
                .filter_map(|(idx, _)| {
                    std::num::NonZeroU16::new((idx + 1) as u16).map(ParticipantId::new)
                })
                .collect(),
        };

        let result = LockingProtocolResult {
            session_id: JournalSessionId::from_uuid(session_id.uuid()),
            operation_type: self.operation_type,
            winner,
            granted: true,
            threshold_signature,
            ledger_events: Vec::new(),
            participants: self.contenders.clone(),
        };

        Ok(result)
    }
}

impl ProtocolLifecycle for LockingLifecycle {
    type State = LockingLifecycleState;
    type Output = LockingProtocolResult;
    type Error = LockingLifecycleError;

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
                let outcome = self.complete();

                if let Ok(result) = &outcome {
                    self.outcome = Some(result.clone());
                }

                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        LockingLifecycleState::NAME,
                        "LockingGranted",
                        None,
                    )),
                    outcome,
                )
            }
            ProtocolInput::LocalSignal { signal, .. } if signal == "abort" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        LockingLifecycleState::NAME,
                        "LockingAborted",
                        None,
                    )),
                    Err(LockingLifecycleError::Unsupported("locking aborted")),
                )
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for LockingLifecycle {
    type Evidence = ();

    fn validate_evidence(_evidence: &Self::Evidence) -> bool {
        true
    }

    fn rehydrate(
        device_id: DeviceId,
        account_id: AccountId,
        _evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        let _ = account_id;
        Ok(Self::new_ephemeral(
            device_id,
            aura_journal::OperationType::Locking,
            Vec::new(),
        ))
    }
}
