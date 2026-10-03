use super::*;
use aura_app::ui::scenarios::UiOperationHandle;
use aura_app::ui_contract::SemanticOperationError;
use aura_core::types::identifiers::CeremonyId;

impl UiModel {
    pub(crate) fn device_enrollment_completion_state(
        &self,
        ceremony_id: Option<&CeremonyId>,
    ) -> Option<OperationState> {
        let operation_id = OperationId::device_enrollment_completion_for(ceremony_id?);
        self.operations
            .iter()
            .find(|operation| operation.id == operation_id)
            .map(|operation| operation.state)
    }

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

    pub(super) fn set_operation_state(&mut self, operation_id: OperationId, state: OperationState) {
        if let Some(index) = self.operations.iter().position(|op| op.id == operation_id) {
            let instance_id = if state == OperationState::Submitting {
                self.operation_instance_key = self.operation_instance_key.saturating_add(1);
                OperationInstanceId(format!("op-{}", self.operation_instance_key))
            } else {
                self.operations[index].instance_id.clone()
            };
            self.operations[index] = OperationSnapshot {
                id: operation_id.clone(),
                instance_id,
                state,
                failure: None,
            };
            self.operation_causalities.insert(operation_id, None);
            return;
        }
        self.operation_instance_key = self.operation_instance_key.saturating_add(1);
        self.operations.push(OperationSnapshot {
            id: operation_id.clone(),
            instance_id: OperationInstanceId(format!("op-{}", self.operation_instance_key)),
            state,
            failure: None,
        });
        self.operation_causalities.insert(operation_id, None);
    }

    #[cfg(test)]
    pub(super) fn set_authoritative_operation_state(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
    ) {
        self.set_authoritative_operation_state_with_failure(
            operation_id,
            instance_id,
            causality,
            state,
            None,
        );
    }

    fn set_authoritative_operation_state_with_failure(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
        failure: Option<SemanticOperationError>,
    ) {
        if let Some(instance_id) = instance_id {
            let current_causality = self
                .operation_causalities
                .get(&operation_id)
                .cloned()
                .flatten();
            match self.operations.iter_mut().find(|op| op.id == operation_id) {
                Some(operation) if operation.instance_id == instance_id => {
                    if Self::incoming_causality_is_older(current_causality, causality) {
                        return;
                    }
                    if Self::terminal_transition_requires_new_instance(operation.state, state) {
                        return;
                    }
                    operation.state = state;
                    operation.failure = failure;
                    self.operation_causalities.insert(operation_id, causality);
                    return;
                }
                Some(_operation)
                    if Self::incoming_causality_is_older(current_causality, causality) =>
                {
                    return;
                }
                Some(operation)
                    if Self::incoming_instance_is_older(&operation.instance_id, &instance_id) =>
                {
                    return;
                }
                _ => {
                    self.operations
                        .retain(|operation| operation.id != operation_id);
                    self.operations.push(OperationSnapshot {
                        id: operation_id.clone(),
                        instance_id,
                        state,
                        failure,
                    });
                    self.operation_causalities.insert(operation_id, causality);
                    return;
                }
            }
        }

        self.set_authoritative_operation_state_without_instance(
            operation_id,
            causality,
            state,
            failure,
        );
    }

    fn set_authoritative_operation_state_without_instance(
        &mut self,
        operation_id: OperationId,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
        failure: Option<SemanticOperationError>,
    ) {
        let needs_new_instance = state == OperationState::Submitting
            && self
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
                .is_some_and(|operation| {
                    matches!(
                        operation.state,
                        OperationState::Succeeded
                            | OperationState::Failed
                            | OperationState::Cancelled
                    )
                });
        if needs_new_instance {
            self.set_operation_state(operation_id, state);
            return;
        }

        if let Some(operation) = self.operations.iter_mut().find(|op| op.id == operation_id) {
            operation.state = state;
            operation.failure = failure;
            self.operation_causalities.insert(operation_id, causality);
            return;
        }

        self.set_operation_state(operation_id.clone(), state);
        if let Some(operation) = self.operations.iter_mut().find(|op| op.id == operation_id) {
            operation.failure = failure;
        }
        self.operation_causalities.insert(operation_id, causality);
    }

    pub(super) fn clear_operation(&mut self, operation_id: &OperationId) {
        self.operations
            .retain(|operation| &operation.id != operation_id);
        self.operation_causalities.remove(operation_id);
    }
}

impl UiController {
    /// Apply an authoritative semantic operation status onto the currently
    /// materialized UI operation snapshot for the given operation id.
    pub fn apply_authoritative_operation_status(
        &self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    ) {
        let next_state = match status.phase {
            SemanticOperationPhase::Succeeded => OperationState::Succeeded,
            SemanticOperationPhase::Failed => OperationState::Failed,
            SemanticOperationPhase::Cancelled => OperationState::Cancelled,
            _ => OperationState::Submitting,
        };
        let mut model = write_model(&self.model);
        model.set_authoritative_operation_state_with_failure(
            operation_id,
            instance_id,
            causality,
            next_state,
            status.error,
        );
        let snapshot = model.semantic_snapshot();
        drop(model);
        self.publish_ui_snapshot(snapshot);
        self.request_rerender();
    }

    /// Seed an exact submitted operation instance before handing ownership to a
    /// shared workflow so downstream semantic status publication can bind to
    /// the same UI-visible instance id.
    pub fn begin_exact_operation_submission(
        &self,
        operation_id: OperationId,
    ) -> OperationInstanceId {
        let mut model = write_model(&self.model);
        model.set_operation_state(operation_id.clone(), OperationState::Submitting);
        let instance_id = model
            .operations
            .iter()
            .rev()
            .find(|operation| operation.id == operation_id)
            .map(|operation| operation.instance_id.clone())
            .unwrap_or_else(|| {
                panic!("begin_exact_operation_submission must materialize an operation snapshot")
            });
        drop(model);
        self.request_rerender();
        instance_id
    }

    /// Materialize the exact UI operation handle for a submission that must
    /// preserve the same visible instance id across the frontend handoff.
    pub fn begin_exact_operation_handle_submission(
        &self,
        operation_id: OperationId,
    ) -> UiOperationHandle {
        let instance_id = self.begin_exact_operation_submission(operation_id.clone());
        UiOperationHandle::new(operation_id, instance_id)
    }

    pub(crate) fn complete_runtime_modal_success(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', message);
        dismiss_modal(&mut model);
        drop(model);
        self.request_rerender();
    }
}

#[cfg(test)]
mod failure_snapshot_tests {
    use super::*;
    use aura_app::ui_contract::{SemanticFailureCode, SemanticFailureDomain};

    #[test]
    fn authoritative_failure_and_cancellation_survive_snapshot_export() {
        let mut model = UiModel::new("authority-local".to_string());
        let operation_id =
            OperationId::device_enrollment_completion_for(&CeremonyId::new("completion-42"));
        let instance_id = OperationInstanceId("op-42".to_string());
        let failure = SemanticOperationError::new(
            SemanticFailureDomain::Invitation,
            SemanticFailureCode::OperationTimedOut,
        );
        model.set_authoritative_operation_state_with_failure(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            OperationState::Failed,
            Some(failure.clone()),
        );
        let snapshot = model.semantic_snapshot();
        let exported = snapshot
            .operations
            .iter()
            .find(|op| op.id == operation_id)
            .unwrap();
        assert_eq!(exported.instance_id, instance_id);
        assert_eq!(exported.failure, Some(failure));
        let encoded = serde_json::to_value(exported).expect("serializable operation");
        assert_eq!(encoded["failure"]["domain"], "invitation");
        assert_eq!(encoded["failure"]["code"], "operation_timed_out");

        let cancelled = OperationInstanceId("op-43".to_string());
        model.set_authoritative_operation_state_with_failure(
            operation_id.clone(),
            Some(cancelled.clone()),
            None,
            OperationState::Cancelled,
            None,
        );
        let exported = model
            .semantic_snapshot()
            .operations
            .into_iter()
            .find(|op| op.id == operation_id)
            .unwrap();
        assert_eq!(exported.instance_id, cancelled);
        assert_eq!(exported.state, OperationState::Cancelled);
        assert!(exported.failure.is_none());
    }

    #[test]
    fn older_ceremony_replay_cannot_replace_newer_completion() {
        let mut model = UiModel::new("authority-local".to_string());
        let old = CeremonyId::new("old");
        let new = CeremonyId::new("new");
        let old_id = OperationId::device_enrollment_completion_for(&old);
        let new_id = OperationId::device_enrollment_completion_for(&new);
        model.set_authoritative_operation_state(
            new_id.clone(),
            Some(OperationInstanceId("completion-new".to_string())),
            None,
            OperationState::Succeeded,
        );
        model.set_authoritative_operation_state(
            old_id.clone(),
            Some(OperationInstanceId("completion-old".to_string())),
            None,
            OperationState::Failed,
        );
        assert_eq!(
            model.device_enrollment_completion_state(Some(&new)),
            Some(OperationState::Succeeded)
        );
        assert_eq!(
            model.device_enrollment_completion_state(Some(&old)),
            Some(OperationState::Failed)
        );
        assert_eq!(
            model
                .semantic_snapshot()
                .operations
                .iter()
                .filter(|operation| { operation.id == old_id || operation.id == new_id })
                .count(),
            2
        );
    }
}
