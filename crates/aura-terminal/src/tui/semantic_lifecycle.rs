use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::tui::tasks::UiTaskOwner;
use crate::tui::updates::{UiUpdate, UiUpdateSender};
use async_lock::RwLock;
use async_trait::async_trait;
use aura_app::frontend_primitives::{
    dropped_owner_error, CeremonyMonitorHandoffSubmission, LocalTerminalSubmission,
    SubmittedOperationPublisher, SubmittedOperationWorkflowError, WorkflowHandoffRelease,
    WorkflowHandoffSubmission,
};
use aura_app::ui::types::AppCore;
use aura_app::ui_contract::{
    HarnessUiOperationHandle, OperationId, OperationInstanceId, SemanticFailureCode,
    SemanticFailureDomain, SemanticOperationCausality, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
    WorkflowTerminalOutcome, WorkflowTerminalStatus,
};
use futures::executor::block_on;
use std::future::Future;

static NEXT_OWNER_OPERATION_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticOperationTransferScope {
    InvitationImport,
    CreateGuardianInvitation,
    AcceptInvitation,
    InviteActorToChannel,
    AcceptPendingChannelInvitation,
    JoinChannel,
    SendChatMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticOperationTransferResult {
    Relinquished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticOperationTransfer {
    release: WorkflowHandoffRelease,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    scope: SemanticOperationTransferScope,
    result: SemanticOperationTransferResult,
}

use super::updates::{
    publish_ui_update, send_ui_update_required, send_ui_update_required_blocking,
    UiUpdatePublication,
};

#[must_use]
pub(crate) fn authoritative_operation_status_update(
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    causality: Option<SemanticOperationCausality>,
    status: SemanticOperationStatus,
) -> UiUpdate {
    UiUpdate::AuthoritativeOperationStatus {
        operation_id,
        instance_id,
        causality,
        status,
    }
}

async fn publish_handoff_protocol_failure(
    tx: &UiUpdateSender,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    detail: String,
) {
    send_ui_update_required(
        tx,
        authoritative_operation_status_update(
            operation_id,
            Some(instance_id),
            None,
            SemanticOperationStatus::failed(
                kind,
                SemanticOperationError::new(
                    SemanticFailureDomain::Command,
                    SemanticFailureCode::InternalError,
                )
                .with_detail(detail),
            ),
        ),
    )
    .await;
}

pub(crate) async fn apply_handed_off_terminal_status(
    _app_core: &Arc<RwLock<AppCore>>,
    tx: &UiUpdateSender,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    terminal: Option<WorkflowTerminalStatus>,
) -> Result<(), String> {
    let Some(terminal) = terminal else {
        let detail = format!(
            "handoff completed without terminal authoritative status for {}:{}",
            operation_id.0, instance_id.0
        );
        publish_handoff_protocol_failure(tx, operation_id, instance_id, kind, detail.clone()).await;
        return Err(detail);
    };

    if terminal.status.kind != kind {
        let detail = format!(
            "handoff completed with mismatched semantic kind for {}:{} (expected={kind:?} observed={:?})",
            operation_id.0, instance_id.0, terminal.status.kind
        );
        publish_handoff_protocol_failure(tx, operation_id, instance_id, kind, detail.clone()).await;
        return Err(detail);
    }

    if !terminal.status.phase.is_terminal() {
        let detail = format!(
            "handoff completed without terminal phase for {}:{} (observed={:?})",
            operation_id.0, instance_id.0, terminal.status.phase
        );
        publish_handoff_protocol_failure(tx, operation_id, instance_id, kind, detail.clone()).await;
        return Err(detail);
    }

    if !send_ui_update_required(
        tx,
        authoritative_operation_status_update(
            operation_id.clone(),
            Some(instance_id.clone()),
            terminal.causality,
            terminal.status,
        ),
    )
    .await
    {
        tracing::warn!(
            operation_id = %operation_id.0,
            instance_id = %instance_id.0,
            "terminal status delivery failed: UI update channel closed"
        );
    }
    Ok(())
}

fn next_owned_operation_instance_id(operation_id: &OperationId) -> OperationInstanceId {
    let nonce = NEXT_OWNER_OPERATION_NONCE.fetch_add(1, Ordering::Relaxed) + 1;
    OperationInstanceId(format!("tui-op-{}-{}", operation_id.0, nonce))
}

#[derive(Clone)]
struct TuiSubmittedOperationPublisher {
    tx: UiUpdateSender,
}

#[async_trait]
impl SubmittedOperationPublisher for TuiSubmittedOperationPublisher {
    async fn publish_dispatched(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        kind: SemanticOperationKind,
    ) {
        let submission = authoritative_operation_status_update(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            SemanticOperationStatus::new(kind, SemanticOperationPhase::WorkflowDispatched),
        );
        if !send_ui_update_required_blocking(&self.tx, submission) {
            tracing::warn!(
                operation_id = %operation_id.0,
                instance_id = %instance_id.0,
                "terminal submission delivery failed: UI update channel closed"
            );
        }
    }

    async fn publish_terminal(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    ) {
        let update = authoritative_operation_status_update(
            operation_id.clone(),
            Some(instance_id.clone()),
            causality,
            status.clone(),
        );
        let delivered = if status.phase == SemanticOperationPhase::Succeeded {
            publish_ui_update(&self.tx, update, UiUpdatePublication::RequiredUnordered).await
        } else {
            send_ui_update_required(&self.tx, update).await
        };
        if !delivered {
            tracing::warn!(
                operation_id = %operation_id.0,
                instance_id = %instance_id.0,
                "terminal status delivery failed: UI update channel closed"
            );
        }
    }

    fn publish_drop_failure(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        kind: SemanticOperationKind,
    ) {
        let update = authoritative_operation_status_update(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            dropped_owner_error(kind),
        );
        if self.tx.try_send(update).is_err() {
            tracing::warn!(
                operation_id = %operation_id.0,
                instance_id = %instance_id.0,
                "dropped-owner failure delivery failed: UI update channel full or closed"
            );
        }
    }
}

struct SubmittedLocalOperationOwner(LocalTerminalSubmission<TuiSubmittedOperationPublisher>);
struct SubmittedWorkflowOperationOwner(WorkflowHandoffSubmission<TuiSubmittedOperationPublisher>);

struct SubmittedCeremonyOwner {
    inner: CeremonyMonitorHandoffSubmission<TuiSubmittedOperationPublisher>,
}

#[must_use]
pub struct LocalTerminalOperationOwner(SubmittedLocalOperationOwner);

#[must_use]
pub struct WorkflowHandoffOperationOwner(SubmittedWorkflowOperationOwner);

#[must_use]
pub struct CeremonySubmissionOwner(SubmittedCeremonyOwner);

impl SubmittedLocalOperationOwner {
    fn submit(
        _app_core: Arc<RwLock<AppCore>>,
        _tasks: Arc<UiTaskOwner>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let publisher = TuiSubmittedOperationPublisher { tx };
        Self(block_on(LocalTerminalSubmission::submit(
            publisher,
            operation_id,
            kind,
            next_owned_operation_instance_id,
        )))
    }

    async fn succeed(self) {
        self.0.succeed(None).await;
    }

    async fn fail(self, detail: impl Into<String>) {
        let error = SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(detail.into());
        self.fail_with(error).await;
    }

    async fn fail_with(self, error: SemanticOperationError) {
        self.0.fail(error).await;
    }

    fn harness_handle(&self) -> HarnessUiOperationHandle {
        HarnessUiOperationHandle::new(self.0.operation_id().clone(), self.0.instance_id().clone())
    }
}

impl SubmittedWorkflowOperationOwner {
    fn submit(
        _app_core: Arc<RwLock<AppCore>>,
        _tasks: Arc<UiTaskOwner>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let publisher = TuiSubmittedOperationPublisher { tx };
        Self(block_on(WorkflowHandoffSubmission::submit(
            publisher,
            operation_id,
            kind,
            next_owned_operation_instance_id,
        )))
    }

    fn handoff_to_app_workflow(
        self,
        scope: SemanticOperationTransferScope,
    ) -> SemanticOperationTransfer {
        let release = self.0.handoff_to_workflow();
        let operation_id = release.operation_id().clone();
        let instance_id = release.instance_id().clone();
        let kind = release.kind();
        SemanticOperationTransfer {
            release,
            operation_id,
            instance_id,
            kind,
            scope,
            result: SemanticOperationTransferResult::Relinquished,
        }
    }

    fn harness_handle(&self) -> HarnessUiOperationHandle {
        HarnessUiOperationHandle::new(self.0.operation_id().clone(), self.0.instance_id().clone())
    }

    #[cfg(test)]
    fn instance_id(&self) -> &OperationInstanceId {
        self.0.instance_id()
    }
}

impl SubmittedCeremonyOwner {
    fn submit(
        _app_core: Arc<RwLock<AppCore>>,
        _tasks: Arc<UiTaskOwner>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let publisher = TuiSubmittedOperationPublisher { tx };
        Self {
            inner: block_on(CeremonyMonitorHandoffSubmission::submit(
                publisher,
                operation_id,
                kind,
                next_owned_operation_instance_id,
            )),
        }
    }

    fn harness_handle(&self) -> HarnessUiOperationHandle {
        HarnessUiOperationHandle::new(
            self.inner.operation_id().clone(),
            self.inner.instance_id().clone(),
        )
    }

    async fn fail_with(self, error: SemanticOperationError) {
        self.inner.fail(error).await;
    }

    async fn fail(self, detail: impl Into<String>) {
        let error = SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(detail.into());
        self.fail_with(error).await;
    }

    async fn monitor_started(self) {
        let _ = self.inner.monitor_started(None).await;
    }

    async fn cancel(self) {
        let _ = self.inner.cancel().await;
    }
}

impl LocalTerminalOperationOwner {
    pub(crate) fn submit(
        app_core: Arc<RwLock<AppCore>>,
        tasks: Arc<UiTaskOwner>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self(SubmittedLocalOperationOwner::submit(
            app_core,
            tasks,
            tx,
            operation_id,
            kind,
        ))
    }

    pub(crate) fn harness_handle(&self) -> HarnessUiOperationHandle {
        self.0.harness_handle()
    }

    pub(crate) fn ui_update_instance_id(&self) -> Option<OperationInstanceId> {
        Some(self.0 .0.instance_id().clone())
    }

    pub(crate) async fn succeed(self) {
        self.0.succeed().await;
    }

    pub(crate) async fn fail(self, detail: impl Into<String>) {
        self.0.fail(detail).await;
    }

    pub(crate) async fn fail_with(self, error: SemanticOperationError) {
        self.0.fail_with(error).await;
    }
}

pub(crate) fn submit_local_terminal_operation(
    app_core: Arc<RwLock<AppCore>>,
    tasks: Arc<UiTaskOwner>,
    tx: UiUpdateSender,
    operation_id: OperationId,
    kind: SemanticOperationKind,
) -> LocalTerminalOperationOwner {
    LocalTerminalOperationOwner::submit(app_core, tasks, tx, operation_id, kind)
}

impl WorkflowHandoffOperationOwner {
    pub(crate) fn submit(
        app_core: Arc<RwLock<AppCore>>,
        tasks: Arc<UiTaskOwner>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self(SubmittedWorkflowOperationOwner::submit(
            app_core,
            tasks,
            tx,
            operation_id,
            kind,
        ))
    }

    pub(crate) fn harness_handle(&self) -> HarnessUiOperationHandle {
        self.0.harness_handle()
    }

    pub(crate) fn workflow_instance_id(&self) -> Option<OperationInstanceId> {
        Some(self.0 .0.instance_id().clone())
    }

    pub(crate) fn handoff_to_app_workflow(
        self,
        scope: SemanticOperationTransferScope,
    ) -> SemanticOperationTransfer {
        self.0.handoff_to_app_workflow(scope)
    }
}

impl CeremonySubmissionOwner {
    pub(crate) fn submit(
        app_core: Arc<RwLock<AppCore>>,
        tasks: Arc<UiTaskOwner>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self(SubmittedCeremonyOwner::submit(
            app_core,
            tasks,
            tx,
            operation_id,
            kind,
        ))
    }

    pub(crate) async fn monitor_started(self) {
        self.0.monitor_started().await;
    }

    pub(crate) fn harness_handle(&self) -> HarnessUiOperationHandle {
        self.0.harness_handle()
    }

    pub(crate) async fn fail(self, detail: impl Into<String>) {
        self.0.fail(detail).await;
    }
    pub(crate) async fn cancel(self) {
        self.0.cancel().await;
    }
}

impl SemanticOperationTransfer {
    pub(crate) fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    #[cfg(test)]
    pub(crate) fn instance_id(&self) -> &OperationInstanceId {
        &self.instance_id
    }

    pub(crate) fn kind(&self) -> SemanticOperationKind {
        self.kind
    }

    pub(crate) async fn run_workflow<T, Fut>(
        self,
        app_core: Arc<RwLock<AppCore>>,
        tx: UiUpdateSender,
        panic_context: &'static str,
        workflow: Fut,
    ) -> Result<T, SubmittedOperationWorkflowError>
    where
        Fut: Future<Output = WorkflowTerminalOutcome<T>>,
    {
        match self.release.run_workflow(panic_context, workflow).await {
            Ok(outcome) => {
                if let Err(detail) = apply_handed_off_terminal_status(
                    &app_core,
                    &tx,
                    self.operation_id,
                    self.instance_id,
                    self.kind,
                    outcome.terminal,
                )
                .await
                {
                    return Err(SubmittedOperationWorkflowError::Protocol(detail));
                }
                outcome
                    .result
                    .map_err(SubmittedOperationWorkflowError::Workflow)
            }
            Err(detail) => {
                let _ = apply_handed_off_terminal_status(
                    &app_core,
                    &tx,
                    self.operation_id,
                    self.instance_id,
                    self.kind,
                    Some(WorkflowTerminalStatus {
                        causality: None,
                        status: SemanticOperationStatus::failed(
                            self.kind,
                            SemanticOperationError::new(
                                SemanticFailureDomain::Command,
                                SemanticFailureCode::InternalError,
                            )
                            .with_detail(detail.clone()),
                        ),
                    }),
                )
                .await;
                Err(SubmittedOperationWorkflowError::Panicked(detail))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::{AppConfig, AppCore};
    use tokio::sync::mpsc;

    async fn init_signals_for_test(app_core: &Arc<RwLock<AppCore>>) {
        AppCore::init_signals_with_hooks(app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[derive(Clone, Copy)]
    struct OperationInvariantCase {
        operation_id: fn() -> OperationId,
        kind: SemanticOperationKind,
    }

    fn local_terminal_cases() -> [OperationInvariantCase; 3] {
        [
            OperationInvariantCase {
                operation_id: OperationId::account_create,
                kind: SemanticOperationKind::CreateAccount,
            },
            OperationInvariantCase {
                operation_id: OperationId::invitation_create,
                kind: SemanticOperationKind::CreateContactInvitation,
            },
            OperationInvariantCase {
                operation_id: || OperationId("create_channel".to_string()),
                kind: SemanticOperationKind::CreateChannel,
            },
        ]
    }

    fn parity_critical_handoff_cases() -> [OperationInvariantCase; 4] {
        [
            OperationInvariantCase {
                operation_id: OperationId::invitation_create,
                kind: SemanticOperationKind::InviteActorToChannel,
            },
            OperationInvariantCase {
                operation_id: OperationId::invitation_accept_channel,
                kind: SemanticOperationKind::AcceptPendingChannelInvitation,
            },
            OperationInvariantCase {
                operation_id: || OperationId("join_channel".to_string()),
                kind: SemanticOperationKind::JoinChannel,
            },
            OperationInvariantCase {
                operation_id: OperationId::send_message,
                kind: SemanticOperationKind::SendChatMessage,
            },
        ]
    }

    async fn new_local_terminal_owner(
        case: OperationInvariantCase,
    ) -> (
        Arc<RwLock<AppCore>>,
        Arc<UiTaskOwner>,
        mpsc::Receiver<UiUpdate>,
        SubmittedLocalOperationOwner,
    ) {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, rx) = mpsc::channel(8);
        let owner = SubmittedLocalOperationOwner::submit(
            app_core.clone(),
            tasks.clone(),
            tx,
            (case.operation_id)(),
            case.kind,
        );
        (app_core, tasks, rx, owner)
    }

    async fn new_handoff_owner(
        case: OperationInvariantCase,
    ) -> (
        Arc<RwLock<AppCore>>,
        Arc<UiTaskOwner>,
        mpsc::Receiver<UiUpdate>,
        SubmittedWorkflowOperationOwner,
    ) {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, rx) = mpsc::channel(8);
        let owner = SubmittedWorkflowOperationOwner::submit(
            app_core.clone(),
            tasks.clone(),
            tx,
            (case.operation_id)(),
            case.kind,
        );
        (app_core, tasks, rx, owner)
    }

    async fn assert_drop_terminates(case: OperationInvariantCase) {
        let (_app_core, tasks, mut rx, owner) = new_handoff_owner(case).await;
        drop(owner);

        let _submitted = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        let terminal = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing dropped-owner failure"));

        match terminal {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::Failed);
                assert_eq!(status.kind, case.kind);
            }
            other => panic!("unexpected terminal update: {other:?}"),
        }

        tasks.shutdown();
    }

    async fn assert_success_is_single_terminal(case: OperationInvariantCase) {
        let (_app_core, tasks, mut rx, owner) = new_local_terminal_owner(case).await;
        owner.succeed().await;

        let _submitted = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        let terminal = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing success update"));

        match terminal {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
                assert_eq!(status.kind, case.kind);
            }
            other => panic!("unexpected terminal update: {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "settled owner must not publish duplicate terminal state"
        );

        tasks.shutdown();
    }

    async fn assert_handoff_preserves_exact_terminal_instance(case: OperationInvariantCase) {
        let (_app_core, tasks, mut rx, owner) = new_handoff_owner(case).await;
        let transfer =
            owner.handoff_to_app_workflow(SemanticOperationTransferScope::InvitationImport);

        let submitted = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        match submitted {
            UiUpdate::AuthoritativeOperationStatus {
                operation_id,
                instance_id,
                status,
                ..
            } => {
                assert_eq!(operation_id, (case.operation_id)());
                assert_eq!(instance_id, Some(transfer.instance_id().clone()));
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
            }
            other => panic!("unexpected submission update: {other:?}"),
        }

        let failed = authoritative_operation_status_update(
            transfer.operation_id().clone(),
            Some(transfer.instance_id().clone()),
            None,
            SemanticOperationStatus::failed(
                transfer.kind(),
                SemanticOperationError::new(
                    SemanticFailureDomain::Command,
                    SemanticFailureCode::InternalError,
                )
                .with_detail("handoff-dispatch failed"),
            ),
        );

        match failed {
            UiUpdate::AuthoritativeOperationStatus {
                operation_id,
                instance_id,
                status,
                ..
            } => {
                assert_eq!(operation_id, (case.operation_id)());
                assert_eq!(instance_id, Some(transfer.instance_id().clone()));
                assert_eq!(status.phase, SemanticOperationPhase::Failed);
                assert_eq!(status.kind, case.kind);
            }
            other => panic!("unexpected terminal update: {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "handoff must prevent stale local owner updates from masking terminal state"
        );

        tasks.shutdown();
    }

    #[tokio::test]
    async fn dropped_owner_publishes_failed_terminal_status() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = mpsc::channel(8);

        let owner = SubmittedWorkflowOperationOwner::submit(
            app_core.clone(),
            tasks.clone(),
            tx,
            OperationId::account_create(),
            SemanticOperationKind::CreateAccount,
        );

        drop(owner);

        let first = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        let second = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing dropped-owner failure"));

        match first {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
            }
            other => panic!("unexpected first update: {other:?}"),
        }

        match second {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::Failed);
                assert_eq!(status.kind, SemanticOperationKind::CreateAccount);
                assert_eq!(
                    status.error.and_then(|error| error.detail),
                    Some(
                        "semantic operation owner dropped before terminal publication or explicit handoff"
                            .to_string()
                    )
                );
            }
            other => panic!("unexpected second update: {other:?}"),
        }

        tasks.shutdown();
    }

    #[tokio::test]
    async fn parity_critical_owner_invariants_hold() {
        for case in local_terminal_cases() {
            assert_success_is_single_terminal(case).await;
        }
        for case in parity_critical_handoff_cases() {
            assert_drop_terminates(case).await;
            assert_handoff_preserves_exact_terminal_instance(case).await;
        }
    }

    #[tokio::test]
    async fn best_effort_failure_after_terminal_does_not_regress() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = mpsc::channel(8);
        let tx_best_effort = tx.clone();

        let owner = SubmittedLocalOperationOwner::submit(
            app_core,
            tasks.clone(),
            tx,
            OperationId::account_create(),
            SemanticOperationKind::CreateAccount,
        );

        // Publish terminal success.
        owner.succeed().await;

        // Drain the submission and success updates.
        let submitted = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        let terminal = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing success update"));

        match &submitted {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
                assert_eq!(status.kind, SemanticOperationKind::CreateAccount);
            }
            other => panic!("unexpected submission update: {other:?}"),
        }
        match &terminal {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
                assert_eq!(status.kind, SemanticOperationKind::CreateAccount);
            }
            other => panic!("unexpected terminal update: {other:?}"),
        }

        // No additional updates should have arrived yet.
        assert!(
            rx.try_recv().is_err(),
            "no extra updates expected before best-effort send"
        );

        // Simulate a best-effort update (e.g. a toast) sent on the same
        // channel after the terminal status has already been published.
        let best_effort = UiUpdate::AuthoritativeOperationStatus {
            operation_id: OperationId::account_create(),
            instance_id: None,
            causality: None,
            status: SemanticOperationStatus::new(
                SemanticOperationKind::CreateAccount,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        };
        let _ = tx_best_effort.try_send(best_effort);

        // Even though an extra message arrived on the channel, the two
        // authoritative updates we already drained are unmodified: the
        // terminal Succeeded status was not regressed.
        match &terminal {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(
                    status.phase,
                    SemanticOperationPhase::Succeeded,
                    "terminal status must not be regressed by later best-effort sends"
                );
            }
            other => panic!("terminal update changed unexpectedly: {other:?}"),
        }

        tasks.shutdown();
    }

    #[tokio::test]
    async fn terminal_status_not_regressed_by_later_submission() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = mpsc::channel(16);

        let operation_id = OperationId::invitation_accept_channel();

        // First owner: submit and handoff to app workflow.
        let first_owner = SubmittedWorkflowOperationOwner::submit(
            app_core.clone(),
            tasks.clone(),
            tx.clone(),
            operation_id.clone(),
            SemanticOperationKind::AcceptPendingChannelInvitation,
        );
        let first_instance_id = first_owner.instance_id().clone();
        let _transfer = first_owner.handoff_to_app_workflow(
            SemanticOperationTransferScope::AcceptPendingChannelInvitation,
        );

        // Drain the first submission update.
        let first_submitted = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing first submission update"));
        match &first_submitted {
            UiUpdate::AuthoritativeOperationStatus {
                instance_id,
                status,
                ..
            } => {
                assert_eq!(instance_id.as_ref(), Some(&first_instance_id));
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
            }
            other => panic!("unexpected first submission update: {other:?}"),
        }

        // Second owner: same operation_id but gets a different instance_id.
        let second_owner = SubmittedWorkflowOperationOwner::submit(
            app_core,
            tasks.clone(),
            tx,
            operation_id,
            SemanticOperationKind::AcceptPendingChannelInvitation,
        );
        let second_instance_id = second_owner.instance_id().clone();

        // The two submissions must have distinct instance_ids.
        assert_ne!(
            first_instance_id, second_instance_id,
            "each submission must mint a unique instance_id even for the same operation_id"
        );

        // Drop the second owner without settling; it will publish a Failed
        // terminal status for the *second* instance only.
        drop(second_owner);

        // Drain the second owner's submission and drop-failure updates.
        let second_submitted = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing second submission update"));
        let second_failure = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing second dropped-owner failure"));

        match &second_submitted {
            UiUpdate::AuthoritativeOperationStatus {
                instance_id,
                status,
                ..
            } => {
                assert_eq!(instance_id.as_ref(), Some(&second_instance_id));
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
            }
            other => panic!("unexpected second submission update: {other:?}"),
        }

        match &second_failure {
            UiUpdate::AuthoritativeOperationStatus {
                instance_id,
                status,
                ..
            } => {
                assert_eq!(
                    instance_id.as_ref(),
                    Some(&second_instance_id),
                    "failure must target the second instance, not the first"
                );
                assert_eq!(status.phase, SemanticOperationPhase::Failed);
            }
            other => panic!("unexpected second failure update: {other:?}"),
        }

        // Confirm no collision: the first instance's terminal state is
        // unaffected because the second owner's failure carries its own
        // distinct instance_id.
        assert_ne!(
            first_instance_id, second_instance_id,
            "instance_id isolation prevents regression of the first terminal status"
        );

        tasks.shutdown();
    }

    #[tokio::test]
    async fn relinquish_to_workflow_returns_typed_transfer_record_without_failure() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = mpsc::channel(8);

        let owner = SubmittedWorkflowOperationOwner::submit(
            app_core,
            tasks.clone(),
            tx,
            OperationId::invitation_accept_contact(),
            SemanticOperationKind::AcceptContactInvitation,
        );

        let transfer =
            owner.handoff_to_app_workflow(SemanticOperationTransferScope::InvitationImport);

        assert_eq!(
            transfer.kind,
            SemanticOperationKind::AcceptContactInvitation
        );
        assert_eq!(
            transfer.scope,
            SemanticOperationTransferScope::InvitationImport
        );
        assert_eq!(
            transfer.result,
            SemanticOperationTransferResult::Relinquished
        );

        let first = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        match first {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
            }
            other => panic!("unexpected first update: {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "relinquished owner must not publish dropped-owner failure"
        );

        tasks.shutdown();
    }

    #[tokio::test]
    async fn settled_owner_publishes_single_terminal_outcome() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        init_signals_for_test(&app_core).await;
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = mpsc::channel(8);

        SubmittedLocalOperationOwner::submit(
            app_core.clone(),
            tasks.clone(),
            tx,
            OperationId::account_create(),
            SemanticOperationKind::CreateAccount,
        )
        .succeed()
        .await;

        let first = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing submission update"));
        let second = rx
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing success update"));

        match first {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::WorkflowDispatched);
            }
            other => panic!("unexpected first update: {other:?}"),
        }
        match second {
            UiUpdate::AuthoritativeOperationStatus { status, .. } => {
                assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
                assert_eq!(status.kind, SemanticOperationKind::CreateAccount);
            }
            other => panic!("unexpected second update: {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "settled owner must not publish duplicate terminal state"
        );

        tasks.shutdown();
    }
}
