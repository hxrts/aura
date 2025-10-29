//! Resharing protocol lifecycle adapter targeting the unified protocol core traits.

use super::ResharingLifecycleError;
use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::{protocol_results::ResharingProtocolResult, ParticipantId, ThresholdSignature};
use aura_crypto::Ed25519Signature;
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AccountId, AuraError, DeviceId, SessionId};
use uuid::Uuid;

/// Typestate marker for resharing lifecycle.
#[derive(Debug, Clone)]
pub struct ResharingLifecycleState;

impl SessionState for ResharingLifecycleState {
    const NAME: &'static str = "ResharingLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Minimal lifecycle implementation returning stubbed resharing output.
#[derive(Debug, Clone)]
pub struct ResharingLifecycle {
    descriptor: ProtocolDescriptor,
    state: ResharingLifecycleState,
    finished: bool,
    output: Option<ResharingProtocolResult>,
    old_participants: Vec<DeviceId>,
    new_participants: Vec<DeviceId>,
    threshold: u16,
}

impl ResharingLifecycle {
    /// Construct a new lifecycle instance.
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        threshold: u16,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::Resharing,
        )
        .with_operation_type(OperationType::Resharing)
        .with_priority(ProtocolPriority::High)
        .with_mode(ProtocolMode::Interactive);

        let signature = Ed25519Signature::default();

        let mut lifecycle = Self {
            descriptor,
            state: ResharingLifecycleState,
            finished: false,
            output: None,
            old_participants: old_participants.clone(),
            new_participants: new_participants.clone(),
            threshold,
        };

        // Create capability proof using unified builder
        use crate::protocols::CapabilityProofBuilder;
        let capability_proof = CapabilityProofBuilder::new(device_id, "resharing")
            .create_proof("threshold_shares", "threshold_resharing")
            .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

        lifecycle.output = Some(ResharingProtocolResult {
            session_id: JournalSessionId::from_uuid(session_id.uuid()),
            new_threshold: threshold,
            new_participants,
            old_participants: old_participants.clone(),
            new_shares: Vec::new(),
            approval_signature: ThresholdSignature {
                signature,
                signers: old_participants
                    .iter()
                    .enumerate()
                    .filter_map(|(i, _device)| {
                        std::num::NonZeroU16::new((i + 1) as u16).map(ParticipantId::new)
                    })
                    .collect(),
            },
            ledger_events: Vec::new(),
            capability_proof,
        });

        lifecycle
    }

    /// Convenience helper generating a fresh session identifier.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        threshold: u16,
    ) -> Self {
        Self::new(
            device_id,
            SessionId::new(),
            old_participants,
            new_participants,
            threshold,
        )
    }
}

impl ProtocolLifecycle for ResharingLifecycle {
    type State = ResharingLifecycleState;
    type Output = ResharingProtocolResult;
    type Error = ResharingLifecycleError;

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
                        ResharingLifecycleState::NAME,
                        "ResharingCompleted",
                        None,
                    )),
                    self.output
                        .clone()
                        .ok_or(AuraError::agent_invalid_state("missing resharing output")),
                )
            }
            ProtocolInput::LocalSignal { signal, .. } if signal == "abort" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        ResharingLifecycleState::NAME,
                        "ResharingAborted",
                        None,
                    )),
                    Err(AuraError::session_aborted("resharing aborted")),
                )
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for ResharingLifecycle {
    type Evidence = ();

    fn validate_evidence(_evidence: &Self::Evidence) -> bool {
        true
    }

    fn rehydrate(
        device_id: DeviceId,
        _account_id: AccountId,
        _evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        Ok(Self::new_ephemeral(device_id, Vec::new(), Vec::new(), 0))
    }
}
