use super::*;
use aura_app::ui::scenarios::UiOperationHandle;

impl UiModel {
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
            };
            self.operation_causalities.insert(operation_id, None);
            return;
        }
        self.operation_instance_key = self.operation_instance_key.saturating_add(1);
        self.operations.push(OperationSnapshot {
            id: operation_id.clone(),
            instance_id: OperationInstanceId(format!("op-{}", self.operation_instance_key)),
            state,
        });
        self.operation_causalities.insert(operation_id, None);
    }

    pub(super) fn set_authoritative_operation_state(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
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
                    self.operation_causalities.insert(operation_id, causality);
                    return;
                }
                Some(operation)
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
                    });
                    self.operation_causalities.insert(operation_id, causality);
                    return;
                }
            }
        }

        self.set_authoritative_operation_state_without_instance(operation_id, causality, state);
    }

    fn set_authoritative_operation_state_without_instance(
        &mut self,
        operation_id: OperationId,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
    ) {
        let needs_new_instance = state == OperationState::Submitting
            && self
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
                .is_some_and(|operation| {
                    matches!(
                        operation.state,
                        OperationState::Succeeded | OperationState::Failed
                    )
                });
        if needs_new_instance {
            self.set_operation_state(operation_id, state);
            return;
        }

        if let Some(operation) = self.operations.iter_mut().find(|op| op.id == operation_id) {
            operation.state = state;
            self.operation_causalities.insert(operation_id, causality);
            return;
        }

        self.set_operation_state(operation_id.clone(), state);
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
            SemanticOperationPhase::Failed | SemanticOperationPhase::Cancelled => {
                OperationState::Failed
            }
            _ => OperationState::Submitting,
        };
        let mut model = write_model(&self.model);
        model.set_authoritative_operation_state(operation_id, instance_id, causality, next_state);
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
