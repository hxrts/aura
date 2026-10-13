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

/// Prepare from the original app completion without selecting identity or clearing
/// transfer data. Actual synchronous writes begin only after the app budget ACK.
pub(crate) fn enrollment_import_host_policy(
    app: Arc<RwLock<AppCore>>,
    storage_prefix: String,
) -> aura_app::ui::workflows::invitation::EnrollmentImportHostPolicy {
    aura_app::ui::workflows::invitation::EnrollmentImportHostPolicy::Prepare(Arc::new(
        move |completed| {
            let app = app.clone();
            let storage_prefix = storage_prefix.clone();
            Box::pin(async move {
                let identity = BootstrapRuntimeIdentity::new(
                    completed.subject_authority(),
                    completed.device_id(),
                );
                let config = crate::bootstrap_storage::prepare_imported_enrollment_account_config(
                    &app, completed,
                )
                .await
                .map_err(host_persistence_error)?;
                Ok(
                    aura_app::ui::workflows::invitation::PreparedEnrollmentHostCommit::new(
                        move || {
                            // These writes are individually fallible, not an atomic transaction.
                            // A failure preserves actual partial progress and its native source.
                            crate::bootstrap_storage::persist_account_config(
                                WebUiOperation::ImportDeviceEnrollmentCode,
                                &config,
                            )
                            .map_err(host_persistence_error)?;
                            persist_selected_runtime_identity(
                                &selected_runtime_identity_key(&storage_prefix),
                                &identity,
                            )
                            .map_err(host_persistence_error)?;
                            clear_pending_device_enrollment_code(
                                &pending_device_enrollment_code_key(&storage_prefix),
                            )
                            .map_err(host_persistence_error)?;
                            Ok(())
                        },
                    ),
                )
            })
        },
    ))
}

fn host_persistence_error(error: WebUiError) -> aura_core::AuraError {
    aura_core::AuraError::Storage {
        message: "Browser enrollment identity persistence failed".into(),
        source: Some(Arc::new(error)),
    }
}
