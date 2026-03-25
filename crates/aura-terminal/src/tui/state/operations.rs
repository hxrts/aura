use aura_app::ui::contract::{OperationId, OperationInstanceId, OperationSnapshot, OperationState};
use aura_app::ui_contract::SemanticOperationCausality;
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct TrackedOperation {
    instance_id: OperationInstanceId,
    causality: Option<SemanticOperationCausality>,
    state: OperationState,
}

#[derive(Clone, Debug, Default)]
pub(super) struct OperationTracker {
    next_instance_nonce: u64,
    entries: HashMap<OperationId, TrackedOperation>,
}

impl OperationTracker {
    fn instance_generation(instance_id: &OperationInstanceId) -> Option<u64> {
        instance_id.0.rsplit('-').next()?.parse::<u64>().ok()
    }

    fn incoming_instance_is_older(
        current: &OperationInstanceId,
        incoming: &OperationInstanceId,
    ) -> bool {
        match (
            Self::instance_generation(current),
            Self::instance_generation(incoming),
        ) {
            (Some(current_generation), Some(incoming_generation)) => {
                incoming_generation < current_generation
            }
            _ => false,
        }
    }

    fn incoming_causality_is_older(
        current: Option<SemanticOperationCausality>,
        incoming: Option<SemanticOperationCausality>,
    ) -> bool {
        match (current, incoming) {
            (Some(current), Some(incoming)) => incoming.is_older_than(current),
            _ => false,
        }
    }

    fn terminal_transition_requires_new_instance(
        existing: OperationState,
        next: OperationState,
    ) -> bool {
        !existing.can_transition_to(next)
    }

    pub(super) fn set_state(&mut self, operation_id: OperationId, state: OperationState) {
        let needs_new_instance = self.entries.get(&operation_id).is_some_and(|entry| {
            Self::terminal_transition_requires_new_instance(entry.state, state)
        }) || matches!(state, OperationState::Submitting)
            || !self.entries.contains_key(&operation_id);
        if needs_new_instance {
            let instance_id = self.next_instance_id(&operation_id);
            self.entries.insert(
                operation_id,
                TrackedOperation {
                    instance_id,
                    causality: None,
                    state,
                },
            );
            return;
        }

        if let Some(entry) = self.entries.get_mut(&operation_id) {
            entry.state = state;
        }
    }

    pub(super) fn set_authoritative_state(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
    ) {
        if let Some(instance_id) = instance_id {
            match self.entries.get_mut(&operation_id) {
                Some(entry) if entry.instance_id == instance_id => {
                    if Self::incoming_causality_is_older(entry.causality, causality) {
                        return;
                    }
                    if Self::terminal_transition_requires_new_instance(entry.state, state) {
                        return;
                    }
                    entry.causality = causality;
                    entry.state = state;
                    return;
                }
                Some(entry) if Self::incoming_causality_is_older(entry.causality, causality) => {
                    return;
                }
                Some(entry)
                    if Self::incoming_instance_is_older(&entry.instance_id, &instance_id) =>
                {
                    return;
                }
                _ => {
                    self.entries.insert(
                        operation_id,
                        TrackedOperation {
                            instance_id,
                            causality,
                            state,
                        },
                    );
                    return;
                }
            }
        }
        let needs_new_instance = self.entries.get(&operation_id).is_some_and(|entry| {
            Self::terminal_transition_requires_new_instance(entry.state, state)
        });
        if needs_new_instance {
            self.set_state(operation_id, state);
            return;
        }
        if let Some(entry) = self.entries.get_mut(&operation_id) {
            entry.causality = causality;
            entry.state = state;
            return;
        }
        self.set_state(operation_id, state);
    }

    pub(super) fn state(&self, operation_id: &OperationId) -> Option<OperationState> {
        self.entries.get(operation_id).map(|entry| entry.state)
    }

    pub(super) fn exported_snapshots(&self) -> Vec<OperationSnapshot> {
        self.entries
            .iter()
            .map(|(id, tracked)| OperationSnapshot {
                id: id.clone(),
                instance_id: tracked.instance_id.clone(),
                state: tracked.state,
            })
            .collect()
    }

    fn next_instance_id(&mut self, operation_id: &OperationId) -> OperationInstanceId {
        self.next_instance_nonce += 1;
        OperationInstanceId(format!(
            "tui-op-{}-{}",
            operation_id.0, self.next_instance_nonce
        ))
    }
}
