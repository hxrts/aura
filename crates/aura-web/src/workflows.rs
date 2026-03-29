use async_lock::RwLock;
use aura_app::ui::types::{BootstrapRuntimeIdentity, InvitationBridgeType};
use aura_app::ui::workflows::account as account_workflows;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::invitation as invitation_workflows;
use aura_app::AppCore;
use aura_core::types::identifiers::AuthorityId;
use std::sync::Arc;

use crate::bootstrap_storage::persist_runtime_account_config;
use crate::error::WebUiError;
use crate::{
    clear_pending_device_enrollment_code, pending_device_enrollment_code_key,
    persist_pending_device_enrollment_code, persist_selected_runtime_identity,
    selected_runtime_identity_key, WebUiOperation,
};

#[derive(Clone, Debug)]
pub(crate) struct CurrentRuntimeIdentity {
    pub(crate) authority_id: AuthorityId,
    pub(crate) selected_runtime_identity: Option<BootstrapRuntimeIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RebootstrapPolicy {
    StageIfRequired,
    RejectIfRequired,
}

#[derive(Clone, Debug)]
pub(crate) struct DeviceEnrollmentImportRequest<'a> {
    pub(crate) code: &'a str,
    pub(crate) current_runtime_identity: CurrentRuntimeIdentity,
    pub(crate) storage_prefix: &'a str,
    pub(crate) rebootstrap_policy: RebootstrapPolicy,
    pub(crate) operation: WebUiOperation,
}

#[derive(Clone, Debug)]
pub(crate) struct DeviceEnrollmentImportResult {
    pub(crate) accepted: bool,
    pub(crate) rebootstrap_required: bool,
    pub(crate) bootstrap_name: String,
    pub(crate) staged_runtime_identity: BootstrapRuntimeIdentity,
}

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

pub(crate) async fn accept_device_enrollment_import(
    app_core: &Arc<RwLock<AppCore>>,
    request: DeviceEnrollmentImportRequest<'_>,
) -> Result<DeviceEnrollmentImportResult, WebUiError> {
    let invitation = invitation_workflows::import_invitation_details(app_core, request.code)
        .await
        .map_err(|error| {
            WebUiError::operation(
                request.operation,
                "WEB_DEVICE_ENROLLMENT_IMPORT_DETAILS_FAILED",
                error.to_string(),
            )
        })?;
    let invitation_info = invitation.info().clone();
    let InvitationBridgeType::DeviceEnrollment {
        subject_authority,
        device_id,
        nickname_suggestion,
        ..
    } = invitation_info.invitation_type.clone()
    else {
        return Err(WebUiError::input(
            request.operation,
            "WEB_DEVICE_ENROLLMENT_CODE_INVALID_KIND",
            "Code is not a device enrollment invitation",
        ));
    };

    let bootstrap_name = crate::device_enrollment_bootstrap_name(nickname_suggestion.as_deref());
    let staged_runtime_identity = BootstrapRuntimeIdentity::new(subject_authority, device_id);
    let runtime_matches_invitation = request.current_runtime_identity.authority_id
        == subject_authority
        && request
            .current_runtime_identity
            .selected_runtime_identity
            .as_ref()
            .map(|identity| identity.device_id)
            == Some(device_id);

    if !runtime_matches_invitation {
        if request.rebootstrap_policy == RebootstrapPolicy::StageIfRequired {
            let pending_code_storage_key =
                pending_device_enrollment_code_key(request.storage_prefix);
            let runtime_identity_key = selected_runtime_identity_key(request.storage_prefix);
            persist_pending_device_enrollment_code(&pending_code_storage_key, request.code)
                .map_err(|error| error.with_operation(request.operation))?;
            persist_selected_runtime_identity(&runtime_identity_key, &staged_runtime_identity)
                .map_err(|error| error.with_operation(request.operation))?;
        }

        return Ok(DeviceEnrollmentImportResult {
            accepted: false,
            rebootstrap_required: true,
            bootstrap_name,
            staged_runtime_identity,
        });
    }

    if context_workflows::current_home_context(app_core)
        .await
        .is_err()
    {
        account_workflows::initialize_runtime_account(app_core, bootstrap_name.clone())
            .await
            .map_err(|error| {
                WebUiError::operation(
                    request.operation,
                    "WEB_DEVICE_ENROLLMENT_ACCOUNT_INIT_FAILED",
                    error.to_string(),
                )
            })?;
    }

    invitation_workflows::accept_device_enrollment_invitation(app_core, &invitation_info)
        .await
        .map_err(|error| {
            WebUiError::operation(
                request.operation,
                "WEB_DEVICE_ENROLLMENT_ACCEPT_FAILED",
                error.to_string(),
            )
        })?;
    persist_runtime_account_config(app_core, Some(bootstrap_name.clone()), request.operation)
        .await?;
    clear_pending_device_enrollment_code(&pending_device_enrollment_code_key(
        request.storage_prefix,
    ))?;

    Ok(DeviceEnrollmentImportResult {
        accepted: true,
        rebootstrap_required: false,
        bootstrap_name,
        staged_runtime_identity,
    })
}
