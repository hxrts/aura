use crate::UiController;
use async_trait::async_trait;
use aura_app::frontend_primitives::{
    run_frontend_sync_boundary, CeremonyMonitorHandoffSubmission, LocalTerminalSubmission,
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
use std::future::Future;
use std::sync::Arc;

fn observe_controller_publication<T>(
    boundary: &'static str,
    observation: impl Future<Output = Result<T, aura_core::AuraError>>,
) {
    if let Err(source) = run_frontend_sync_boundary(boundary, observation) {
        tracing::error!(%source, "controller terminal publication failed");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiOperationTransferScope {
    StartDeviceEnrollment,
    ImportDeviceEnrollment,
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

    fn publish_terminal(
        &self,
        intent: aura_app::frontend_primitives::SubmittedTerminalIntent,
    ) -> aura_app::frontend_primitives::SubmittedTerminalObservation {
        let acknowledgment = aura_app::frontend_primitives::SubmittedTerminalAcknowledgment {
            operation_id: intent.operation_id.clone(),
            instance_id: intent.instance_id.clone(),
            scope: aura_app::frontend_primitives::SubmittedTerminalAcknowledgmentScope::ControllerApplied,
        };
        self.controller.apply_authoritative_operation_status(
            intent.operation_id,
            Some(intent.instance_id),
            intent.causality,
            intent.status,
        );
        drop(intent.retained);
        Box::pin(async move { Ok(acknowledgment) })
    }
    fn relinquish(&self, _: &OperationId, _: &OperationInstanceId, _: SemanticOperationKind) {}
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
        || ((operation_id == OperationId::invitation_accept_contact()
            || operation_id == OperationId::invitation_accept_channel())
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

/// Admit required import custody before relinquishing the exact local owner.
/// The admitted task cannot execute until its actual release arms it.
pub fn begin_exact_enrollment_import(
    controller: Arc<UiController>,
    code: String,
    transfer_input: Option<aura_app::ui::contract::EnrollmentManifestTransferInput>,
    host: aura_app::ui::workflows::invitation::EnrollmentImportHostPolicy,
) -> Result<
    (
        UiOperationHandle,
        UiOperationTransfer,
        aura_app::ui::workflows::invitation::EnrollmentImportObservation,
    ),
    String,
> {
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportAdmissionRefusal,
    };
    let owner = UiWorkflowHandoffOwner::submit(
        controller.clone(),
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
    );
    let handle = owner.operation_handle();
    #[cfg(target_arch = "wasm32")]
    if matches!(
        &host,
        aura_app::ui::workflows::invitation::EnrollmentImportHostPolicy::RuntimeOnly
    ) {
        let refused = EnrollmentImportAdmissionRefusal::browser_host_unavailable();
        let detail = refused.source_error().to_string();
        owner.fail_with_custody(refused.semantic_error(), Box::new(refused));
        return Err(detail);
    }
    let admitted = match controller.enrollment_import_admission() {
        Some(admission) => import_device_enrollment_with_terminal_status(
            controller.app_core(),
            admission,
            code,
            transfer_input,
            owner.workflow_instance_id(),
            host,
        ),
        None => Err(EnrollmentImportAdmissionRefusal::unavailable()),
    };
    let mut observer = match admitted {
        Ok(observer) => observer,
        Err(refused) => {
            let detail = refused.source_error().to_string();
            owner.fail_with_custody(refused.semantic_error(), Box::new(refused));
            return Err(detail);
        }
    };
    let transfer = match owner.handoff_enrollment_import(&mut observer) {
        Ok(transfer) => transfer,
        Err((owner, error)) => {
            let detail = error.to_string();
            owner.fail_with_custody(error.semantic_error(), Box::new(error));
            return Err(detail);
        }
    };
    Ok((handle, transfer, observer))
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
        Self(run_frontend_sync_boundary(
            "UiLocalOperationOwner::submit_with_instance",
            LocalTerminalSubmission::submit_with_instance(
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

    fn succeed(self, causality: Option<SemanticOperationCausality>) {
        observe_controller_publication("UiLocalOperationOwner::succeed", self.0.succeed(causality));
    }

    fn fail(self, error: SemanticOperationError) {
        observe_controller_publication("UiLocalOperationOwner::fail", self.0.fail(error));
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
        Self(run_frontend_sync_boundary(
            "UiWorkflowHandoffOwner::submit_with_instance",
            WorkflowHandoffSubmission::submit_with_instance(
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
        observe_controller_publication("UiWorkflowHandoffOwner::fail", self.0.fail(error));
    }

    fn handoff_enrollment_import(
        self,
        observer: &mut aura_app::ui::workflows::invitation::EnrollmentImportObservation,
    ) -> Result<
        UiOperationTransfer,
        (
            Self,
            aura_app::ui::workflows::invitation::EnrollmentImportHandoffError,
        ),
    > {
        match observer.accept_handoff(self.0) {
            Ok(release) => Ok(Self::transfer_with_release(
                release,
                UiOperationTransferScope::ImportDeviceEnrollment,
            )),
            Err((submission, error)) => Err((Self(submission), error)),
        }
    }

    fn handoff_to_app_workflow(self, scope: UiOperationTransferScope) -> UiOperationTransfer {
        let release = self.0.handoff_to_workflow();
        Self::transfer_with_release(release, scope)
    }

    fn transfer_with_release(
        release: WorkflowHandoffRelease,
        scope: UiOperationTransferScope,
    ) -> UiOperationTransfer {
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

    fn handoff_enrollment_import(
        self,
        observer: &mut aura_app::ui::workflows::invitation::EnrollmentImportObservation,
    ) -> Result<
        UiOperationTransfer,
        (
            Self,
            aura_app::ui::workflows::invitation::EnrollmentImportHandoffError,
        ),
    > {
        self.0
            .handoff_enrollment_import(observer)
            .map_err(|(owner, error)| (Self(owner), error))
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
    pub fn fail_with_custody(
        self,
        error: SemanticOperationError,
        retained: aura_app::frontend_primitives::SubmittedTerminalCustody,
    ) {
        observe_controller_publication(
            "UiWorkflowHandoffOwner::fail_with_custody",
            self.0 .0.fail_with_custody(error, Some(retained)),
        );
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
        Self(run_frontend_sync_boundary(
            "UiCeremonySubmissionOwner::submit_with_instance",
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
        observe_controller_publication("UiCeremonySubmissionOwner::fail", self.0.fail(error));
    }

    fn monitor_started(self) {
        observe_controller_publication(
            "UiCeremonySubmissionOwner::monitor_started",
            self.0.monitor_started(None),
        );
    }

    fn cancel(self) {
        observe_controller_publication("UiCeremonySubmissionOwner::cancel", self.0.cancel());
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
    fn synchronous_terminal_receipt_proves_controller_application() {
        use aura_app::frontend_primitives::{
            SubmittedTerminalAcknowledgmentScope, SubmittedTerminalIntent,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Retained(Arc<AtomicUsize>);
        impl Drop for Retained {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::AcqRel);
            }
        }

        let controller = controller();
        let publisher = UiSubmittedOperationPublisher {
            controller: controller.clone(),
        };
        let operation_id = OperationId::ban_actor();
        let instance_id = OperationInstanceId("controller-original-publication".into());
        let released = Arc::new(AtomicUsize::new(0));
        let observation = publisher.publish_terminal(SubmittedTerminalIntent {
            operation_id: operation_id.clone(),
            instance_id: instance_id.clone(),
            causality: None,
            status: SemanticOperationStatus::failed(
                SemanticOperationKind::BanActor,
                SemanticOperationError::new(
                    SemanticFailureDomain::Command,
                    SemanticFailureCode::PermissionDenied,
                ),
            ),
            retained: Some(Box::new(Retained(released.clone()))),
        });

        // Application precedes polling the receipt; the receipt only observes it.
        let snapshot = controller.semantic_model_snapshot();
        let operation = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == operation_id)
            .expect("actual controller applied terminal intent");
        assert_eq!(operation.instance_id, instance_id);
        assert_eq!(
            operation.state,
            aura_app::ui::contract::OperationState::Failed
        );
        assert_eq!(
            operation.failure.as_ref().map(|failure| failure.code),
            Some(SemanticFailureCode::PermissionDenied)
        );
        assert_eq!(released.load(Ordering::Acquire), 1);

        let acknowledgment = futures::executor::block_on(observation).unwrap();
        assert_eq!(acknowledgment.operation_id, operation_id);
        assert_eq!(acknowledgment.instance_id, instance_id);
        assert_eq!(
            acknowledgment.scope,
            SubmittedTerminalAcknowledgmentScope::ControllerApplied
        );
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
    fn failed_owner_exports_failure_code() {
        let controller = controller();
        let owner = UiLocalOperationOwner::submit(
            controller.clone(),
            OperationId::ban_actor(),
            SemanticOperationKind::BanActor,
        );
        owner.fail_with(SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::PermissionDenied,
        ));

        let snapshot = controller.semantic_model_snapshot();
        let operation = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::ban_actor())
            .expect("ban operation exported");
        assert_eq!(
            operation.state,
            aura_app::ui::contract::OperationState::Failed
        );
        assert_eq!(
            operation.failure.as_ref().map(|failure| failure.code),
            Some(SemanticFailureCode::PermissionDenied)
        );
        let json = serde_json::to_string(operation).unwrap();
        assert!(json.contains("\"code\":\"permission_denied\""));
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
            OperationId::invitation_accept_contact(),
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
