use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::tui::tasks::UiTaskRegistry;
use crate::tui::updates::{UiUpdate, UiUpdateSender};
use async_lock::RwLock;
use aura_app::ui::types::AppCore;
use aura_app::ui_contract::{
    HarnessUiOperationHandle, OperationId, OperationInstanceId, SemanticFailureCode,
    SemanticFailureDomain, SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
    SemanticOperationStatus,
};
use aura_core::SemanticOwnerProtocol;

static NEXT_OWNER_OPERATION_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticOperationOwner {
    FrontendCallback,
    AppWorkflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticOperationTransferScope {
    InvitationImport,
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
    prior_owner: SemanticOperationOwner,
    new_owner: SemanticOperationOwner,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    protocol: SemanticOwnerProtocol,
    scope: SemanticOperationTransferScope,
    result: SemanticOperationTransferResult,
}

use super::updates::send_ui_update_required;

fn send_ui_update_now_or_spawn(tasks: &Arc<UiTaskRegistry>, tx: &UiUpdateSender, update: UiUpdate) {
    if tx.try_send(update.clone()).is_ok() {
        return;
    }

    let tx = tx.clone();
    tasks.spawn(async move {
        let _ = tx.send(update).await;
    });
}

#[must_use]
pub(crate) fn authoritative_operation_status_update(
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    status: SemanticOperationStatus,
) -> UiUpdate {
    UiUpdate::AuthoritativeOperationStatus {
        operation_id,
        instance_id,
        status,
    }
}

fn next_owned_operation_instance_id(operation_id: &OperationId) -> OperationInstanceId {
    let nonce = NEXT_OWNER_OPERATION_NONCE.fetch_add(1, Ordering::Relaxed) + 1;
    OperationInstanceId(format!("tui-op-{}-{}", operation_id.0, nonce))
}

pub(crate) struct SubmittedOperationOwner {
    _app_core: Arc<RwLock<AppCore>>,
    tasks: Arc<UiTaskRegistry>,
    tx: UiUpdateSender,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    owner: SemanticOperationOwner,
    settled: bool,
}

impl SubmittedOperationOwner {
    pub(crate) fn submit_local_only(
        app_core: Arc<RwLock<AppCore>>,
        tasks: Arc<UiTaskRegistry>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let instance_id = next_owned_operation_instance_id(&operation_id);
        let status = SemanticOperationStatus::new(kind, SemanticOperationPhase::WorkflowDispatched);
        send_ui_update_now_or_spawn(
            &tasks,
            &tx,
            authoritative_operation_status_update(
                operation_id.clone(),
                Some(instance_id.clone()),
                status,
            ),
        );

        Self {
            _app_core: app_core,
            tasks,
            tx,
            operation_id,
            instance_id,
            kind,
            owner: SemanticOperationOwner::FrontendCallback,
            settled: false,
        }
    }

    pub(crate) async fn succeed(mut self) {
        self.settled = true;
        let status = SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded);
        send_ui_update_required(
            &self.tx,
            authoritative_operation_status_update(
                self.operation_id.clone(),
                Some(self.instance_id.clone()),
                status,
            ),
        )
        .await;
    }

    pub(crate) async fn fail(self, detail: impl Into<String>) {
        let error = SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(detail.into());
        self.fail_with(error).await;
    }

    pub(crate) async fn fail_with(mut self, error: SemanticOperationError) {
        self.settled = true;
        let status = SemanticOperationStatus::failed(self.kind, error.clone());
        send_ui_update_required(
            &self.tx,
            authoritative_operation_status_update(
                self.operation_id.clone(),
                Some(self.instance_id.clone()),
                status,
            ),
        )
        .await;
    }

    pub(crate) fn handoff_to_app_workflow(
        mut self,
        scope: SemanticOperationTransferScope,
    ) -> SemanticOperationTransfer {
        self.settled = true;
        SemanticOperationTransfer {
            prior_owner: self.owner,
            new_owner: SemanticOperationOwner::AppWorkflow,
            operation_id: self.operation_id.clone(),
            instance_id: self.instance_id.clone(),
            kind: self.kind,
            protocol: SemanticOwnerProtocol::CANONICAL,
            scope,
            result: SemanticOperationTransferResult::Relinquished,
        }
    }

    pub(crate) fn harness_handle(&self) -> HarnessUiOperationHandle {
        HarnessUiOperationHandle {
            operation_id: self.operation_id.clone(),
            instance_id: self.instance_id.clone(),
        }
    }
}

impl SemanticOperationTransfer {
    pub(crate) fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub(crate) fn instance_id(&self) -> &OperationInstanceId {
        &self.instance_id
    }

    pub(crate) fn kind(&self) -> SemanticOperationKind {
        self.kind
    }
}

impl Drop for SubmittedOperationOwner {
    fn drop(&mut self) {
        if self.settled {
            return;
        }

        let detail =
            "semantic operation owner dropped before terminal publication or explicit handoff"
                .to_string();
        let error = SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(detail);
        let status = SemanticOperationStatus::failed(self.kind, error);

        send_ui_update_now_or_spawn(
            &self.tasks,
            &self.tx,
            authoritative_operation_status_update(
                self.operation_id.clone(),
                Some(self.instance_id.clone()),
                status,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::{AppConfig, AppCore};
    use tokio::sync::mpsc;

    #[derive(Clone, Copy)]
    struct OperationInvariantCase {
        operation_id: fn() -> OperationId,
        kind: SemanticOperationKind,
    }

    fn parity_critical_cases() -> [OperationInvariantCase; 4] {
        [
            OperationInvariantCase {
                operation_id: OperationId::invitation_create,
                kind: SemanticOperationKind::InviteActorToChannel,
            },
            OperationInvariantCase {
                operation_id: OperationId::invitation_accept,
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

    async fn new_owner(
        case: OperationInvariantCase,
    ) -> (
        Arc<RwLock<AppCore>>,
        Arc<UiTaskRegistry>,
        mpsc::Receiver<UiUpdate>,
        SubmittedOperationOwner,
    ) {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let tasks = Arc::new(UiTaskRegistry::new());
        let (tx, rx) = mpsc::channel(8);
        let owner = SubmittedOperationOwner::submit_local_only(
            app_core.clone(),
            tasks.clone(),
            tx,
            (case.operation_id)(),
            case.kind,
        );
        (app_core, tasks, rx, owner)
    }

    async fn assert_drop_terminates(case: OperationInvariantCase) {
        let (_app_core, tasks, mut rx, owner) = new_owner(case).await;
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
        let (_app_core, tasks, mut rx, owner) = new_owner(case).await;
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
        let (_app_core, tasks, mut rx, owner) = new_owner(case).await;
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
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let tasks = Arc::new(UiTaskRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);

        let owner = SubmittedOperationOwner::submit_local_only(
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
        for case in parity_critical_cases() {
            assert_drop_terminates(case).await;
            assert_success_is_single_terminal(case).await;
            assert_handoff_preserves_exact_terminal_instance(case).await;
        }
    }

    #[tokio::test]
    async fn relinquish_to_workflow_returns_typed_transfer_record_without_failure() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let tasks = Arc::new(UiTaskRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);

        let owner = SubmittedOperationOwner::submit_local_only(
            app_core,
            tasks.clone(),
            tx,
            OperationId::invitation_accept(),
            SemanticOperationKind::AcceptContactInvitation,
        );

        let transfer =
            owner.handoff_to_app_workflow(SemanticOperationTransferScope::InvitationImport);

        assert_eq!(
            transfer.prior_owner,
            SemanticOperationOwner::FrontendCallback
        );
        assert_eq!(transfer.new_owner, SemanticOperationOwner::AppWorkflow);
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
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let tasks = Arc::new(UiTaskRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);

        SubmittedOperationOwner::submit_local_only(
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
