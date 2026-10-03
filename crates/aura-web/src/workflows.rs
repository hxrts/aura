use async_lock::RwLock;
use aura_app::ui::types::BootstrapRuntimeIdentity;
use aura_app::ui::workflows::account as account_workflows;
use aura_app::AppCore;
use std::sync::Arc;

use crate::bootstrap_storage::persist_runtime_account_config;
use crate::error::WebUiError;
use crate::{
    clear_pending_device_enrollment_code, pending_device_enrollment_code_key,
    persist_selected_runtime_identity, selected_runtime_identity_key, WebUiOperation,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AccountCreationStageMode {
    RuntimeInitialized,
    InitialBootstrapStaged,
}

#[derive(Clone, Debug)]
pub(crate) struct AccountCreationStageResult {
    pub(crate) mode: AccountCreationStageMode,
}

pub(crate) async fn stage_account_creation(
    app_core: &Arc<RwLock<AppCore>>,
    nickname: &str,
) -> Result<AccountCreationStageResult, WebUiError> {
    let has_runtime = {
        let core = app_core.read().await;
        core.runtime().is_some()
    };

    if has_runtime {
        crate::stage_runtime_bound_web_account_bootstrap(nickname).await?;
        account_workflows::initialize_runtime_account(app_core, nickname.to_string())
            .await
            .map_err(|error| {
                WebUiError::operation(
                    WebUiOperation::CreateAccount,
                    "WEB_CREATE_ACCOUNT_INIT_FAILED",
                    error.to_string(),
                )
            })?;
        persist_runtime_account_config(
            app_core,
            Some(nickname.to_string()),
            WebUiOperation::CreateAccount,
        )
        .await?;
        return Ok(AccountCreationStageResult {
            mode: AccountCreationStageMode::RuntimeInitialized,
        });
    }

    crate::stage_initial_web_account_bootstrap(nickname).await?;
    Ok(AccountCreationStageResult {
        mode: AccountCreationStageMode::InitialBootstrapStaged,
    })
}

/// Browser persistence consumes an app-issued completion proof. Received IDs
/// cannot enter this owner path before verified remote acceptance/adoption.
pub(crate) async fn persist_completed_enrollment_identity(
    app: &Arc<RwLock<AppCore>>,
    completed: &aura_app::ui::workflows::invitation::DeviceEnrollmentImportCompleted,
    storage_prefix: &str,
) -> Result<(), WebUiError> {
    let identity =
        BootstrapRuntimeIdentity::new(completed.subject_authority(), completed.device_id());
    persist_selected_runtime_identity(&selected_runtime_identity_key(storage_prefix), &identity)?;
    persist_runtime_account_config(app, None, WebUiOperation::ImportDeviceEnrollmentCode).await?;
    clear_pending_device_enrollment_code(&pending_device_enrollment_code_key(storage_prefix))?;
    Ok(())
}
