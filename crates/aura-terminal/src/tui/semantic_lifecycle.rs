use std::future::Future;
use std::sync::Arc;

use crate::tui::context::IoContext;
use crate::tui::tasks::UiTaskRegistry;
use crate::tui::updates::{UiUpdate, UiUpdateSender};
use async_lock::RwLock;
use aura_app::ui::types::AppCore;
use aura_app::ui::workflows::semantic_facts::{
    publish_authoritative_operation_failure, publish_authoritative_operation_phase,
};
use aura_app::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
};

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
    status: SemanticOperationStatus,
) -> UiUpdate {
    UiUpdate::AuthoritativeOperationStatus {
        operation_id,
        status,
    }
}

pub(crate) async fn publish_authoritative_operation_status_to_tui<F>(
    ctx: &IoContext,
    tx: &UiUpdateSender,
    operation_id: OperationId,
    status: SemanticOperationStatus,
    publish: F,
) where
    F: Future<Output = Result<(), aura_core::AuraError>> + Send + 'static,
{
    send_ui_update_required(
        tx,
        authoritative_operation_status_update(operation_id, status),
    )
    .await;
    let tasks = ctx.tasks();
    tasks.spawn(async move {
        if let Err(error) = publish.await {
            tracing::warn!(error = %error, "authoritative operation status publish failed");
        }
    });
}

pub(crate) async fn publish_authoritative_operation_failure_to_tui(
    ctx: &IoContext,
    tx: &UiUpdateSender,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    detail: impl Into<String>,
) {
    let error = SemanticOperationError::new(
        SemanticFailureDomain::Command,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    let status = SemanticOperationStatus::failed(kind, error.clone());
    let app_core = Arc::clone(ctx.app_core_raw());
    publish_authoritative_operation_status_to_tui(
        ctx,
        tx,
        operation_id.clone(),
        status,
        async move {
            publish_authoritative_operation_failure(&app_core, operation_id, kind, error).await
        },
    )
    .await;
}

pub(crate) struct SubmittedOperationOwner {
    app_core: Arc<RwLock<AppCore>>,
    tasks: Arc<UiTaskRegistry>,
    tx: UiUpdateSender,
    operation_id: OperationId,
    kind: SemanticOperationKind,
}

impl SubmittedOperationOwner {
    pub(crate) fn submit_from_parts(
        app_core: Arc<RwLock<AppCore>>,
        tasks: Arc<UiTaskRegistry>,
        tx: UiUpdateSender,
        operation_id: OperationId,
        kind: SemanticOperationKind,
    ) -> Self {
        let publish_app_core = Arc::clone(&app_core);
        let publish_operation_id = operation_id.clone();
        let status =
            SemanticOperationStatus::new(kind, SemanticOperationPhase::WorkflowDispatched);
        send_ui_update_now_or_spawn(
            &tasks,
            &tx,
            authoritative_operation_status_update(operation_id.clone(), status),
        );
        tasks.spawn(async move {
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
            kind,
        }
    }

    pub(crate) async fn succeed(self) {
        let status = SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded);
        send_ui_update_required(
            &self.tx,
            authoritative_operation_status_update(self.operation_id.clone(), status),
        )
        .await;
        let app_core = Arc::clone(&self.app_core);
        let operation_id = self.operation_id;
        let kind = self.kind;
        self.tasks.spawn(async move {
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
        let status = SemanticOperationStatus::failed(self.kind, error.clone());
        send_ui_update_required(
            &self.tx,
            authoritative_operation_status_update(self.operation_id.clone(), status),
        )
        .await;
        let app_core = Arc::clone(&self.app_core);
        let operation_id = self.operation_id;
        let kind = self.kind;
        self.tasks.spawn(async move {
            if let Err(error) =
                publish_authoritative_operation_failure(&app_core, operation_id, kind, error).await
            {
                tracing::warn!(error = %error, "authoritative operation failure publish failed");
            }
        });
    }
}
