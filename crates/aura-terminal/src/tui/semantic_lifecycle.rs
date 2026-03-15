use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::tui::tasks::UiTaskRegistry;
use crate::tui::updates::{UiUpdate, UiUpdateSender};
use async_lock::RwLock;
use aura_app::ui::types::AppCore;
use aura_app::ui::workflows::semantic_facts::{
    publish_authoritative_operation_failure, publish_authoritative_operation_phase,
};
use aura_app::ui_contract::{
    HarnessUiOperationHandle, OperationId, OperationInstanceId, SemanticFailureCode,
    SemanticFailureDomain, SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
    SemanticOperationStatus,
};

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
    kind: SemanticOperationKind,
    scope: SemanticOperationTransferScope,
    result: SemanticOperationTransferResult,
}

async fn send_ui_update_required(tx: &UiUpdateSender, update: UiUpdate) {
    if tx.try_send(update.clone()).is_err() {
        let _ = tx.send(update).await;
    }
}

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
    app_core: Arc<RwLock<AppCore>>,
    tasks: Arc<UiTaskRegistry>,
    tx: UiUpdateSender,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    owner: SemanticOperationOwner,
    publish_gate: Arc<tokio::sync::Mutex<()>>,
    settled: bool,
}

impl SubmittedOperationOwner {
    pub(crate) fn submit_from_parts(
        app_core: Arc<RwLock<AppCore>>,
        tasks: Arc<UiTaskRegistry>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let instance_id = next_owned_operation_instance_id(&operation_id);
        let publish_gate = Arc::new(tokio::sync::Mutex::new(()));
        let publish_gate_for_submit = Arc::clone(&publish_gate);
        let publish_app_core = Arc::clone(&app_core);
        let publish_operation_id = operation_id.clone();
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
        tasks.spawn(async move {
            let _guard = publish_gate_for_submit.lock().await;
            if let Err(error) = publish_authoritative_operation_phase(
                &publish_app_core,
                publish_operation_id,
                kind,
                SemanticOperationPhase::WorkflowDispatched,
            )
            .await
            {
                tracing::warn!(error = %error, "authoritative operation submission publish failed");
            }
        });

        Self {
            app_core,
            tasks,
            tx,
            operation_id,
            instance_id,
            kind,
            owner: SemanticOperationOwner::FrontendCallback,
            publish_gate,
            settled: false,
        }
    }

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
            app_core,
            tasks,
            tx,
            operation_id,
            instance_id,
            kind,
            owner: SemanticOperationOwner::FrontendCallback,
            publish_gate: Arc::new(tokio::sync::Mutex::new(())),
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
        let app_core = Arc::clone(&self.app_core);
        let operation_id = self.operation_id.clone();
        let kind = self.kind;
        let publish_gate = Arc::clone(&self.publish_gate);
        self.tasks.spawn(async move {
            let _guard = publish_gate.lock().await;
            if let Err(error) = publish_authoritative_operation_phase(
                &app_core,
                operation_id,
                kind,
                SemanticOperationPhase::Succeeded,
            )
            .await
            {
                tracing::warn!(error = %error, "authoritative operation success publish failed");
            }
        });
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
        let app_core = Arc::clone(&self.app_core);
        let operation_id = self.operation_id.clone();
        let kind = self.kind;
        let publish_gate = Arc::clone(&self.publish_gate);
        self.tasks.spawn(async move {
            let _guard = publish_gate.lock().await;
            if let Err(error) =
                publish_authoritative_operation_failure(&app_core, operation_id, kind, error).await
            {
                tracing::warn!(error = %error, "authoritative operation failure publish failed");
            }
        });
    }

    pub(crate) fn relinquish_to_workflow(
        mut self,
        scope: SemanticOperationTransferScope,
    ) -> SemanticOperationTransfer {
        self.settled = true;
        SemanticOperationTransfer {
            prior_owner: self.owner,
            new_owner: SemanticOperationOwner::AppWorkflow,
            operation_id: self.operation_id.clone(),
            kind: self.kind,
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
        .with_detail(detail.clone());
        let status = SemanticOperationStatus::failed(self.kind, error.clone());

        send_ui_update_now_or_spawn(
            &self.tasks,
            &self.tx,
            authoritative_operation_status_update(
                self.operation_id.clone(),
                Some(self.instance_id.clone()),
                status,
            ),
        );

        let app_core = Arc::clone(&self.app_core);
        let operation_id = self.operation_id.clone();
        let kind = self.kind;
        let publish_gate = Arc::clone(&self.publish_gate);
        self.tasks.spawn(async move {
            let _guard = publish_gate.lock().await;
            if let Err(error) =
                publish_authoritative_operation_failure(&app_core, operation_id, kind, error).await
            {
                tracing::warn!(error = %error, "authoritative dropped-owner failure publish failed");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use aura_app::{AppConfig, AppCore};
    use aura_core::effects::reactive::ReactiveEffects;
    use tokio::sync::mpsc;

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

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let facts = {
            let core = app_core.read().await;
            core.read(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                .await
                .unwrap_or_default()
        };
        assert!(facts.iter().any(|fact| {
            fact.operation_status_bridge()
                .is_some_and(|(operation_id, _instance_id, status)| {
                    operation_id == OperationId::account_create()
                        && status.phase == SemanticOperationPhase::Failed
                })
        }));

        tasks.shutdown();
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
            owner.relinquish_to_workflow(SemanticOperationTransferScope::InvitationImport);

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

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let facts = {
            let core = app_core.read().await;
            core.read(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                .await
                .unwrap_or_default()
        };
        assert!(facts.iter().any(|fact| {
            fact.operation_status_bridge()
                .is_some_and(|(operation_id, _instance_id, status)| {
                    operation_id == OperationId::account_create()
                        && status.phase == SemanticOperationPhase::Succeeded
                })
        }));

        tasks.shutdown();
    }
}
