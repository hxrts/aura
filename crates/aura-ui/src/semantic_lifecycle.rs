use crate::UiController;
use async_trait::async_trait;
use aura_app::frontend_primitives::{
    dropped_owner_error, CeremonyMonitorHandoffSubmission, LocalTerminalSubmission,
    SubmittedOperationPublisher, SubmittedOperationWorkflowError, WorkflowHandoffRelease,
    WorkflowHandoffSubmission,
};
use aura_app::ui::scenarios::UiOperationHandle;
use aura_app::ui_contract::{
    OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
    SemanticOperationCausality, SemanticOperationError, SemanticOperationKind,
    SemanticOperationPhase, SemanticOperationStatus, WorkflowTerminalOutcome,
    WorkflowTerminalStatus,
};
use futures::executor::block_on;
use std::future::Future;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiOperationTransferScope {
    StartDeviceEnrollment,
    SendChatMessage,
    JoinChannel,
    CreateInvitation,
    ExportInvitation,
    InvitationImport,
    AcceptInvitation,
    DeclineInvitation,
    RevokeInvitation,
    InviteActorToChannel,
    AcceptPendingChannelInvitation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiOperationTransferResult {
    Relinquished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiOperationTransfer {
    release: WorkflowHandoffRelease,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    scope: UiOperationTransferScope,
    result: UiOperationTransferResult,
}

#[derive(Clone)]
struct UiSubmittedOperationPublisher {
    controller: Arc<UiController>,
}

#[async_trait]
impl SubmittedOperationPublisher for UiSubmittedOperationPublisher {
    async fn publish_dispatched(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        kind: SemanticOperationKind,
    ) {
        self.controller.apply_authoritative_operation_status(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            SemanticOperationStatus::new(kind, SemanticOperationPhase::WorkflowDispatched),
        );
    }

    async fn publish_terminal(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    ) {
        self.controller.apply_authoritative_operation_status(
            operation_id.clone(),
            Some(instance_id.clone()),
            causality,
            status,
        );
    }

    fn publish_drop_failure(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        kind: SemanticOperationKind,
    ) {
        self.controller.apply_authoritative_operation_status(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            dropped_owner_error(kind),
        );
    }
}

struct SubmittedUiLocalOwner(LocalTerminalSubmission<UiSubmittedOperationPublisher>);
struct SubmittedUiHandoffOwner(WorkflowHandoffSubmission<UiSubmittedOperationPublisher>);
struct SubmittedUiCeremonyOwner(CeremonyMonitorHandoffSubmission<UiSubmittedOperationPublisher>);

#[must_use]
pub struct UiLocalOperationOwner(SubmittedUiLocalOwner);

#[must_use]
pub struct UiWorkflowHandoffOwner(SubmittedUiHandoffOwner);

#[must_use]
pub struct UiCeremonySubmissionOwner(SubmittedUiCeremonyOwner);

pub fn handoff_protocol_failure_terminal(
    kind: SemanticOperationKind,
    detail: impl Into<String>,
) -> WorkflowTerminalStatus {
    WorkflowTerminalStatus {
        causality: None,
        status: SemanticOperationStatus::failed(
            kind,
            SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::InternalError,
            )
            .with_detail(detail.into()),
        ),
    }
}

pub fn apply_handed_off_terminal_status(
    controller: Arc<UiController>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    terminal: Option<WorkflowTerminalStatus>,
) -> Result<(), String> {
    let terminal = terminal.ok_or_else(|| {
        format!(
            "workflow handoff completed without terminal authoritative status for {}",
            operation_id.0
        )
    })?;
    let kind_matches = terminal.status.kind == kind
        || (operation_id == OperationId::invitation_accept()
            && matches!(
                kind,
                SemanticOperationKind::AcceptContactInvitation
                    | SemanticOperationKind::AcceptPendingChannelInvitation
                    | SemanticOperationKind::ImportDeviceEnrollmentCode
            )
            && matches!(
                terminal.status.kind,
                SemanticOperationKind::AcceptContactInvitation
                    | SemanticOperationKind::AcceptPendingChannelInvitation
                    | SemanticOperationKind::ImportDeviceEnrollmentCode
            ));
    if !kind_matches {
        return Err(format!(
            "workflow handoff returned mismatched semantic kind for {} (expected={kind:?} observed={:?})",
            operation_id.0, terminal.status.kind
        ));
    }
    if !terminal.status.phase.is_terminal() {
        return Err(format!(
            "workflow handoff returned non-terminal phase for {}: {:?}",
            operation_id.0, terminal.status.phase
        ));
    }
    controller.apply_authoritative_operation_status(
        operation_id,
        instance_id,
        terminal.causality,
        terminal.status,
    );
    Ok(())
}

#[must_use]
pub fn begin_exact_handoff_operation(
    controller: Arc<UiController>,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    scope: UiOperationTransferScope,
) -> (UiOperationHandle, UiOperationTransfer) {
    let instance_id = controller.begin_exact_operation_submission(operation_id.clone());
    let owner =
        UiWorkflowHandoffOwner::submit_with_instance(controller, operation_id, kind, instance_id);
    let handle = owner.operation_handle();
    let transfer = owner.handoff_to_app_workflow(scope);
    (handle, transfer)
}

impl SubmittedUiLocalOwner {
    fn submit(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let instance_id = controller.begin_exact_operation_submission(operation_id.clone());
        Self::submit_with_instance(controller, operation_id, kind, instance_id)
    }

    fn submit_with_instance(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(block_on(LocalTerminalSubmission::submit_with_instance(
            UiSubmittedOperationPublisher { controller },
            operation_id,
            kind,
            instance_id,
        )))
    }

    fn operation_handle(&self) -> UiOperationHandle {
        UiOperationHandle::new(self.0.operation_id().clone(), self.0.instance_id().clone())
    }

    fn succeed(self, causality: Option<SemanticOperationCausality>) {
        block_on(self.0.succeed(causality));
    }

    fn fail(self, error: SemanticOperationError) {
        block_on(self.0.fail(error));
    }
}

impl SubmittedUiHandoffOwner {
    fn submit(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let instance_id = controller.begin_exact_operation_submission(operation_id.clone());
        Self::submit_with_instance(controller, operation_id, kind, instance_id)
    }

    fn submit_with_instance(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(block_on(WorkflowHandoffSubmission::submit_with_instance(
            UiSubmittedOperationPublisher { controller },
            operation_id,
            kind,
            instance_id,
        )))
    }

    fn operation_handle(&self) -> UiOperationHandle {
        UiOperationHandle::new(self.0.operation_id().clone(), self.0.instance_id().clone())
    }

    fn fail(self, error: SemanticOperationError) {
        block_on(self.0.fail(error));
    }

    fn handoff_to_app_workflow(self, scope: UiOperationTransferScope) -> UiOperationTransfer {
        let release = self.0.handoff_to_workflow();
        let operation_id = release.operation_id().clone();
        let instance_id = release.instance_id().clone();
        let kind = release.kind();
        UiOperationTransfer {
            release,
            operation_id,
            instance_id,
            kind,
            scope,
            result: UiOperationTransferResult::Relinquished,
        }
    }
}

impl UiLocalOperationOwner {
    pub fn submit(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self(SubmittedUiLocalOwner::submit(
            controller,
            operation_id,
            kind,
        ))
    }

    pub fn submit_with_instance(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(SubmittedUiLocalOwner::submit_with_instance(
            controller,
            operation_id,
            kind,
            instance_id,
        ))
    }

    #[must_use]
    pub fn operation_handle(&self) -> UiOperationHandle {
        self.0.operation_handle()
    }

    #[must_use]
    pub fn instance_id(&self) -> &OperationInstanceId {
        self.0 .0.instance_id()
    }

    pub fn succeed(self, causality: Option<SemanticOperationCausality>) {
        self.0.succeed(causality);
    }

    pub fn fail_with(self, error: SemanticOperationError) {
        self.0.fail(error);
    }
}

impl UiWorkflowHandoffOwner {
    pub fn submit(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self(SubmittedUiHandoffOwner::submit(
            controller,
            operation_id,
            kind,
        ))
    }

    pub fn submit_with_instance(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(SubmittedUiHandoffOwner::submit_with_instance(
            controller,
            operation_id,
            kind,
            instance_id,
        ))
    }

    #[must_use]
    pub fn operation_handle(&self) -> UiOperationHandle {
        self.0.operation_handle()
    }

    #[must_use]
    pub fn workflow_instance_id(&self) -> Option<OperationInstanceId> {
        Some(self.0 .0.instance_id().clone())
    }

    pub fn fail_with(self, error: SemanticOperationError) {
        self.0.fail(error);
    }

    #[must_use]
    pub fn handoff_to_app_workflow(self, scope: UiOperationTransferScope) -> UiOperationTransfer {
        self.0.handoff_to_app_workflow(scope)
    }
}

impl SubmittedUiCeremonyOwner {
    fn submit(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let instance_id = controller.begin_exact_operation_submission(operation_id.clone());
        Self::submit_with_instance(controller, operation_id, kind, instance_id)
    }

    fn submit_with_instance(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(block_on(
            CeremonyMonitorHandoffSubmission::submit_with_instance(
                UiSubmittedOperationPublisher { controller },
                operation_id,
                kind,
                instance_id,
            ),
        ))
    }

    fn operation_handle(&self) -> UiOperationHandle {
        UiOperationHandle::new(self.0.operation_id().clone(), self.0.instance_id().clone())
    }

    fn fail(self, error: SemanticOperationError) {
        block_on(self.0.fail(error));
    }

    fn monitor_started(self) {
        let _ = block_on(self.0.monitor_started(None));
    }

    fn cancel(self) {
        let _ = block_on(self.0.cancel());
    }
}

impl UiCeremonySubmissionOwner {
    pub fn submit(
        controller: Arc<UiController>,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self(SubmittedUiCeremonyOwner::submit(
            controller,
            operation_id,
            kind,
        ))
    }

    #[must_use]
    pub fn operation_handle(&self) -> UiOperationHandle {
        self.0.operation_handle()
    }

    pub fn fail_with(self, error: SemanticOperationError) {
        self.0.fail(error);
    }

    pub fn monitor_started(self) {
        self.0.monitor_started();
    }

    pub fn cancel(self) {
        self.0.cancel();
    }
}

impl UiOperationTransfer {
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    #[must_use]
    pub fn instance_id(&self) -> &OperationInstanceId {
        &self.instance_id
    }

    #[must_use]
    pub fn kind(&self) -> SemanticOperationKind {
        self.kind
    }

    #[must_use]
    pub fn scope(&self) -> UiOperationTransferScope {
        self.scope
    }

    pub async fn run_workflow<T, Fut>(
        self,
        controller: Arc<UiController>,
        panic_context: &'static str,
        workflow: Fut,
    ) -> Result<T, SubmittedOperationWorkflowError>
    where
        Fut: Future<Output = WorkflowTerminalOutcome<T>>,
    {
        match self.release.run_workflow(panic_context, workflow).await {
            Ok(outcome) => {
                if let Err(detail) = apply_handed_off_terminal_status(
                    controller,
                    self.operation_id,
                    Some(self.instance_id),
                    self.kind,
                    outcome.terminal,
                ) {
                    return Err(SubmittedOperationWorkflowError::Protocol(detail));
                }
                outcome
                    .result
                    .map_err(SubmittedOperationWorkflowError::Workflow)
            }
            Err(detail) => {
                let _ = apply_handed_off_terminal_status(
                    controller,
                    self.operation_id,
                    Some(self.instance_id),
                    self.kind,
                    Some(handoff_protocol_failure_terminal(self.kind, detail.clone())),
                );
                Err(SubmittedOperationWorkflowError::Panicked(detail))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryClipboard;
    use aura_app::{AppConfig, AppCore};
    use std::sync::Arc;

    fn controller() -> Arc<UiController> {
        Arc::new(UiController::new(
            Arc::new(async_lock::RwLock::new(
                AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
            )),
            Arc::new(MemoryClipboard::default()),
        ))
    }

    #[test]
    fn local_owner_publishes_dispatched_then_success() {
        let controller = controller();
        let owner = UiLocalOperationOwner::submit(
            controller.clone(),
            OperationId::invitation_create(),
            SemanticOperationKind::CreateContactInvitation,
        );
        let submitted = controller.semantic_model_snapshot();
        assert_eq!(submitted.operations.len(), 1);
        assert_eq!(submitted.operations[0].id, OperationId::invitation_create());
        assert_eq!(
            submitted.operations[0].state,
            aura_app::ui::contract::OperationState::Submitting
        );

        owner.succeed(None);

        let settled = controller.semantic_model_snapshot();
        assert_eq!(
            settled.operations[0].state,
            aura_app::ui::contract::OperationState::Succeeded
        );
    }

    #[test]
    fn dropped_owner_publishes_failure() {
        let controller = controller();
        let _owner = UiLocalOperationOwner::submit(
            controller.clone(),
            OperationId::invitation_decline(),
            SemanticOperationKind::DeclineInvitation,
        );
        drop(_owner);

        let snapshot = controller.semantic_model_snapshot();
        assert_eq!(snapshot.operations.len(), 1);
        assert_eq!(
            snapshot.operations[0].state,
            aura_app::ui::contract::OperationState::Failed
        );
    }

    #[test]
    fn handoff_marks_owner_settled_and_retains_instance_id() {
        let controller = controller();
        let owner = UiWorkflowHandoffOwner::submit(
            controller,
            OperationId::invitation_accept(),
            SemanticOperationKind::AcceptContactInvitation,
        );
        let original_instance = owner
            .workflow_instance_id()
            .unwrap_or_else(|| panic!("handoff owner must retain instance id"));
        let transfer = owner.handoff_to_app_workflow(UiOperationTransferScope::InvitationImport);
        assert_eq!(transfer.instance_id(), &original_instance);
        assert_eq!(transfer.scope(), UiOperationTransferScope::InvitationImport);
    }
}
