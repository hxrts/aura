use aura_app::ui::types::{
    BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity, BootstrapSurface,
    PendingAccountBootstrap,
};
use aura_app::ui::workflows::account as account_workflows;
use aura_effects::{new_authority_id, new_device_id, RealRandomHandler};
use aura_ui::FrontendUiOperation as WebUiOperation;

use crate::error::{log_web_error, WebUiError};
use crate::harness_bridge;

use super::storage::{
    active_storage_prefix, harness_instance_id, pending_account_bootstrap_key,
    persist_pending_account_bootstrap, persist_selected_runtime_identity,
    selected_runtime_identity_key,
};

pub(crate) fn apply_harness_mode_document_flags() {
    if harness_instance_id().is_none() {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(root) = document.document_element() else {
        return;
    };
    if let Err(error) = root.set_attribute("data-aura-harness-mode", "1") {
        log_web_error(
            "warn",
            &WebUiError::config(
                WebUiOperation::ApplyHarnessModeDocumentFlags,
                "WEB_HARNESS_DOCUMENT_FLAG_SET_FAILED",
                format!("failed to apply harness mode document flag: {error:?}"),
            ),
        );
    }
}

async fn persist_pending_web_account_bootstrap(
    nickname: &str,
) -> Result<PendingAccountBootstrap, WebUiError> {
    let pending_bootstrap = account_workflows::prepare_pending_account_bootstrap(nickname)
        .map_err(|error| {
            WebUiError::input(
                WebUiOperation::PersistPendingAccountBootstrap,
                "WEB_PENDING_BOOTSTRAP_PREPARE_FAILED",
                error.to_string(),
            )
        })?;
    let storage_prefix = active_storage_prefix();
    let pending_account_key = pending_account_bootstrap_key(&storage_prefix);
    persist_pending_account_bootstrap(&pending_account_key, &pending_bootstrap)?;

    let staged_event = BootstrapEvent::new(
        BootstrapSurface::Web,
        BootstrapEventKind::PendingBootstrapStaged,
    );
    web_sys::console::log_1(&staged_event.to_string().into());
    Ok(pending_bootstrap)
}

pub(crate) async fn stage_runtime_bound_web_account_bootstrap(
    nickname: &str,
) -> Result<(), WebUiError> {
    let pending_bootstrap = persist_pending_web_account_bootstrap(nickname).await?;
    web_sys::console::log_1(
        &format!(
            "[web-bootstrap] staged_runtime_bound_account nickname={}",
            pending_bootstrap.nickname_suggestion
        )
        .into(),
    );
    Ok(())
}

pub(crate) async fn stage_initial_web_account_bootstrap(nickname: &str) -> Result<(), WebUiError> {
    let pending_bootstrap = persist_pending_web_account_bootstrap(nickname).await?;
    let storage_prefix = active_storage_prefix();
    let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
    let random = RealRandomHandler::new();
    let authority_id = new_authority_id(&random).await;
    let device_id = new_device_id(&random).await;
    let runtime_identity = BootstrapRuntimeIdentity::new(authority_id, device_id);

    persist_selected_runtime_identity(&runtime_identity_key, &runtime_identity)?;
    web_sys::console::log_1(
        &format!(
            "[web-bootstrap] staged_initial_account authority={authority_id};device={device_id};nickname={}",
            pending_bootstrap.nickname_suggestion
        )
        .into(),
    );
    Ok(())
}

pub(crate) fn device_enrollment_bootstrap_name(nickname_suggestion: Option<&str>) -> String {
    let nickname_suggestion = nickname_suggestion.unwrap_or("").trim();
    if nickname_suggestion.is_empty() {
        "Aura User".to_string()
    } else {
        nickname_suggestion.to_string()
    }
}

pub(crate) async fn submit_runtime_bootstrap_handoff(
    handoff: harness_bridge::BootstrapHandoff,
) -> Result<(), WebUiError> {
    harness_bridge::submit_bootstrap_handoff(handoff)
        .await
        .map_err(|error| {
            WebUiError::operation(
                WebUiOperation::SubmitBootstrapHandoff,
                "WEB_BOOTSTRAP_HANDOFF_FAILED",
                format!("failed to submit web bootstrap handoff: {:?}", error),
            )
        })
}
