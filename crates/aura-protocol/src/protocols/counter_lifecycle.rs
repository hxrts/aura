//! Counter reservation lifecycle adapter leveraging protocol-core traits.

use super::CounterLifecycleError;
use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::protocol_results::CounterProtocolResult;
use aura_journal::{events::RelationshipId, SessionId as JournalSessionId};
use aura_types::{AccountId, AuraError, DeviceId, SessionId};
use uuid::Uuid;

/// Typestate marker for the counter lifecycle.
#[derive(Debug, Clone)]
pub struct CounterLifecycleState;

impl SessionState for CounterLifecycleState {
    const NAME: &'static str = "CounterLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Lifecycle implementation for deterministic counter reservations.
#[derive(Debug, Clone)]
pub struct CounterLifecycle {
    descriptor: ProtocolDescriptor,
    relationship_id: RelationshipId,
    requesting_device: DeviceId,
    participants: Vec<DeviceId>,
    count: u64,
    ttl_epochs: u64,
    current_epoch: u64,
    base_counter: u64,
    finished: bool,
    outcome: Option<CounterProtocolResult>,
}

impl CounterLifecycle {
    /// Construct a new counter reservation lifecycle.
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        relationship_id: RelationshipId,
        requesting_device: DeviceId,
        participants: Vec<DeviceId>,
        count: u64,
        ttl_epochs: u64,
        current_epoch: u64,
        base_counter: u64,
    ) -> Self {
        let descriptor =
            ProtocolDescriptor::new(Uuid::new_v4(), session_id, device_id, ProtocolType::Counter)
                .with_operation_type(OperationType::Counter)
                .with_priority(ProtocolPriority::Normal)
                .with_mode(ProtocolMode::Interactive);

        Self {
            descriptor,
            relationship_id,
            requesting_device,
            participants,
            count,
            ttl_epochs,
            current_epoch,
            base_counter,
            finished: false,
            outcome: None,
        }
    }

    /// Convenience constructor for ephemeral sessions with zero-based counter hint.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        relationship_id: RelationshipId,
        requesting_device: DeviceId,
        count: u64,
        ttl_epochs: u64,
    ) -> Self {
        Self::new(
            device_id,
            SessionId::new(),
            relationship_id,
            requesting_device,
            Vec::new(),
            count,
            ttl_epochs,
            0,
            0,
        )
    }

    fn complete(&mut self) -> Result<CounterProtocolResult, CounterLifecycleError> {
        let session_id = self.descriptor.session_id;
        let mut reserved_values = Vec::with_capacity(self.count as usize);

        for offset in 0..self.count {
            reserved_values.push(self.base_counter + offset + 1);
        }

        // Create capability proof using unified builder
        use crate::protocols::CapabilityProofBuilder;
        let capability_proof = CapabilityProofBuilder::new(self.descriptor.device_id, "counter")
            .create_proof("counter_operations", "counter_reservation")
            .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

        let result = CounterProtocolResult {
            session_id: JournalSessionId::from_uuid(session_id.uuid()),
            relationship_id: self.relationship_id,
            requesting_device: self.requesting_device,
            reserved_values,
            ttl_epochs: self.ttl_epochs,
            ledger_events: Vec::new(),
            participants: self.participants.clone(),
            capability_proof,
        };

        Ok(result)
    }
}

impl ProtocolLifecycle for CounterLifecycle {
    type State = CounterLifecycleState;
    type Output = CounterProtocolResult;
    type Error = CounterLifecycleError;

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
                let mut effects = Vec::<ProtocolEffects>::new();
                if let Ok(result) = &outcome {
                    self.outcome = Some(result.clone());
                    effects.push(ProtocolEffects::UpdateCounter {
                        relationship_hash: self.relationship_id.0,
                        previous_value: self.base_counter,
                        reserved_values: result.reserved_values.clone(),
                        ttl_epochs: result.ttl_epochs,
                        requested_epoch: self.current_epoch,
                        requesting_device: self.requesting_device,
                    });
                }
                ProtocolStep::completed(
                    effects,
                    Some(transition_from_witness(
                        &self.descriptor,
                        CounterLifecycleState::NAME,
                        "CounterReserved",
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
                        CounterLifecycleState::NAME,
                        "CounterAborted",
                        None,
                    )),
                    Err(AuraError::session_aborted("counter reservation aborted")),
                )
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for CounterLifecycle {
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
            RelationshipId([0u8; 32]),
            device_id,
            1,
            100,
        ))
    }
}
