//! DKD protocol lifecycle adapter using the unified protocol core traits.

use crate::{protocol_results::DkdProtocolResult, ParticipantId, ThresholdSignature};
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AccountId, DeviceId, SessionId};
use ed25519_dalek::{Signature, SigningKey};
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

/// Error type surfaced by the DKD lifecycle adapter.
#[derive(Debug, thiserror::Error)]
pub enum DkdLifecycleError {
    #[error("unsupported input for DKD lifecycle: {0}")]
    Unsupported(&'static str),
}

/// Typestate marker for the DKD lifecycle.
#[derive(Debug, Clone)]
pub struct DkdLifecycleState;

impl SessionState for DkdLifecycleState {
    const NAME: &'static str = "DkdLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Minimal DKD lifecycle implementation bridging to the new protocol core.
#[derive(Debug, Clone)]
pub struct DkdLifecycle {
    descriptor: ProtocolDescriptor,
    state: DkdLifecycleState,
    finished: bool,
    output: Option<DkdProtocolResult>,
}

impl DkdLifecycle {
    /// Create a new lifecycle instance anchored to the provided session.
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        _context_id: Vec<u8>,
        participants: Vec<DeviceId>,
    ) -> Self {
        let descriptor =
            ProtocolDescriptor::new(Uuid::new_v4(), session_id, device_id, ProtocolType::Dkd)
                .with_operation_type(OperationType::Dkd)
                .with_priority(ProtocolPriority::High)
                .with_mode(ProtocolMode::Interactive);

        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let derived_public_key = signing_key.verifying_key();

        let signature = Signature::from_bytes(&[0u8; 64]);

        Self {
            descriptor,
            state: DkdLifecycleState,
            finished: false,
            output: Some(DkdProtocolResult {
                session_id: JournalSessionId::from_uuid(session_id.uuid()),
                derived_key: vec![0u8; 32],
                derived_public_key,
                transcript_hash: [0u8; 32],
                threshold_signature: ThresholdSignature {
                    signature,
                    signers: participants
                        .iter()
                        .enumerate()
                        .filter_map(|(i, _device)| {
                            std::num::NonZeroU16::new((i + 1) as u16).map(ParticipantId::new)
                        })
                        .collect(),
                },
                ledger_events: Vec::new(),
                participants,
            }),
        }
    }

    /// Convenience constructor for ephemeral sessions.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        context_id: Vec<u8>,
        participants: Vec<DeviceId>,
    ) -> Self {
        Self::new(device_id, SessionId::new(), context_id, participants)
    }
}

impl ProtocolLifecycle for DkdLifecycle {
    type State = DkdLifecycleState;
    type Output = DkdProtocolResult;
    type Error = DkdLifecycleError;

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
                        DkdLifecycleState::NAME,
                        "DkdCompleted",
                        None,
                    )),
                    self.output
                        .clone()
                        .ok_or(DkdLifecycleError::Unsupported("missing DKD output")),
                )
            }
            ProtocolInput::LocalSignal { signal, .. } if signal == "abort" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        DkdLifecycleState::NAME,
                        "DkdAborted",
                        None,
                    )),
                    Err(DkdLifecycleError::Unsupported("DKD aborted")),
                )
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for DkdLifecycle {
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
        Ok(Self::new_ephemeral(device_id, Vec::new(), Vec::new()))
    }
}
