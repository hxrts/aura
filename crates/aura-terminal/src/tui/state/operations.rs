use aura_app::ui::contract::{OperationId, OperationInstanceId, OperationSnapshot, OperationState};
use aura_app::ui_contract::{SemanticOperationCausality, SemanticOperationError};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct TrackedOperation {
    instance_id: OperationInstanceId,
    causality: Option<SemanticOperationCausality>,
    state: OperationState,
    failure: Option<SemanticOperationError>,
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
                    failure: None,
                },
            );
            return;
        }

        if let Some(entry) = self.entries.get_mut(&operation_id) {
            entry.state = state;
            entry.failure = None;
        }
    }

    pub(super) fn set_authoritative_state(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
    ) {
        self.set_authoritative_state_with_failure(
            operation_id,
            instance_id,
            causality,
            state,
            None,
        );
    }

    pub(super) fn set_authoritative_state_with_failure(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
        failure: Option<SemanticOperationError>,
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
                    entry.failure = failure;
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
                            failure,
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
            self.set_state(operation_id.clone(), state);
            if let Some(entry) = self.entries.get_mut(&operation_id) {
                entry.failure = failure;
            }
            return;
        }
        if let Some(entry) = self.entries.get_mut(&operation_id) {
            entry.causality = causality;
            entry.state = state;
            entry.failure = failure;
            return;
        }
        self.set_state(operation_id.clone(), state);
        if let Some(entry) = self.entries.get_mut(&operation_id) {
            entry.failure = failure;
        }
    }

    /// Whether this operation instance is already recorded as failed, so a
    /// re-published terminal status is not surfaced to the user again.
    pub(super) fn already_failed(
        &self,
        operation_id: &OperationId,
        instance_id: Option<&OperationInstanceId>,
    ) -> bool {
        self.entries.get(operation_id).is_some_and(|entry| {
            entry.state == OperationState::Failed
                && instance_id.map_or(true, |instance| *instance == entry.instance_id)
        })
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
                failure: tracked.failure.clone(),
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

#[cfg(test)]
mod failure_snapshot_tests {
    use super::*;
    use aura_app::ui_contract::{SemanticFailureCode, SemanticFailureDomain};

    #[test]
    fn authoritative_failure_and_cancellation_export_with_their_instance() {
        let mut tracker = OperationTracker::default();
        let operation_id = OperationId::device_enrollment_completion_for(
            &aura_core::types::identifiers::CeremonyId::new("completion-1"),
        );
        let failure = SemanticOperationError::new(
            SemanticFailureDomain::Invitation,
            SemanticFailureCode::OperationTimedOut,
        );
        tracker.set_authoritative_state_with_failure(
            operation_id.clone(),
            Some(OperationInstanceId("tui-op-1".to_string())),
            None,
            OperationState::Failed,
            Some(failure.clone()),
        );
        let exported = tracker.exported_snapshots().pop().unwrap();
        assert_eq!(exported.failure, Some(failure));
        let encoded = serde_json::to_value(&exported).expect("serializable operation");
        assert_eq!(encoded["failure"]["domain"], "invitation");
        assert_eq!(encoded["failure"]["code"], "operation_timed_out");

        tracker.set_authoritative_state_with_failure(
            operation_id,
            Some(OperationInstanceId("tui-op-2".to_string())),
            None,
            OperationState::Cancelled,
            None,
        );
        let exported = tracker.exported_snapshots().pop().unwrap();
        assert_eq!(exported.state, OperationState::Cancelled);
        assert!(exported.failure.is_none());
    }

    #[test]
    fn replayed_older_ceremony_keeps_newer_completion_snapshot() {
        let mut tracker = OperationTracker::default();
        let new_id = OperationId::device_enrollment_completion_for(
            &aura_core::types::identifiers::CeremonyId::new("new"),
        );
        let old_id = OperationId::device_enrollment_completion_for(
            &aura_core::types::identifiers::CeremonyId::new("old"),
        );
        tracker.set_authoritative_state(
            new_id.clone(),
            Some(OperationInstanceId("completion-new".to_string())),
            None,
            OperationState::Succeeded,
        );
        tracker.set_authoritative_state(
            old_id.clone(),
            Some(OperationInstanceId("completion-old".to_string())),
            None,
            OperationState::Failed,
        );
        let snapshots = tracker.exported_snapshots();
        assert_eq!(snapshots.len(), 2);
        assert!(snapshots
            .iter()
            .any(|op| op.id == new_id && op.state == OperationState::Succeeded));
        assert!(snapshots
            .iter()
            .any(|op| op.id == old_id && op.state == OperationState::Failed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (work/8.md task 13): a re-published terminal failure for the
    /// same instance must be recognisable so it is not toasted again.
    #[test]
    fn already_failed_tracks_instance_failures() {
        let mut tracker = OperationTracker::default();
        let op = OperationId::send_message();
        let first = OperationInstanceId("tui-op-send_message-1".to_string());
        let second = OperationInstanceId("tui-op-send_message-2".to_string());

        assert!(!tracker.already_failed(&op, Some(&first)));
        tracker.set_authoritative_state(
            op.clone(),
            Some(first.clone()),
            None,
            OperationState::Failed,
        );
        assert!(tracker.already_failed(&op, Some(&first)));
        // A new attempt is a new instance and must still be reported.
        assert!(!tracker.already_failed(&op, Some(&second)));
    }
}
