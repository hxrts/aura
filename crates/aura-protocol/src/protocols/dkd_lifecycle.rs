//! DKD protocol lifecycle adapter using the unified protocol core traits.

use super::DkdLifecycleError;
use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::{protocol_results::DkdProtocolResult, ParticipantId, ThresholdSignature};
use aura_crypto::Ed25519Signature;
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AccountId, AuraError, DeviceId, SessionId};
use uuid::Uuid;

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
    participants: Vec<DeviceId>,
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

        let signing_key = aura_crypto::ed25519_key_from_bytes(&[7u8; 32]).unwrap();
        let derived_public_key = aura_crypto::ed25519_verifying_key(&signing_key);

        let signature = Ed25519Signature::default();

        let mut lifecycle = Self {
            descriptor,
            state: DkdLifecycleState,
            finished: false,
            output: None,
            participants: participants.clone(),
        };

        // Create capability proof using unified builder
        use crate::protocols::CapabilityProofBuilder;
        let capability_proof = CapabilityProofBuilder::new(device_id, "dkd")
            .create_proof("dkd_derived_keys", "dkd_key_derivation")
            .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

        lifecycle.output = Some(DkdProtocolResult {
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
            capability_proof,
        });

        lifecycle
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
                        .ok_or(AuraError::agent_invalid_state("missing DKD output")),
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
                    Err(AuraError::session_aborted("DKD aborted")),
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
