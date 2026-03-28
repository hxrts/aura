use aura_app::scenario_contract::{
    SharedActionContract, SubmissionContract, SubmissionState, SubmissionValueContract,
};
use aura_app::ui::contract::{OperationId, ScreenId};
use aura_app::ui::scenarios::{
    IntentAction, SemanticCommandRequest, SemanticCommandResponse, SemanticCommandValue,
    SemanticSubmissionHandle, SettingsSection, UiOperationHandle,
};
use aura_app::ui::signals::SETTINGS_SIGNAL;
use aura_app::ui::types::InvitationBridgeType;
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::invitation as invitation_workflows;
use aura_app::ui::workflows::messaging as messaging_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings as settings_workflows;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::ui::workflows::time as time_workflows;
use aura_app::ui_contract::{ChannelBindingWitness, RuntimeFact, SemanticOperationKind};
use aura_core::{
    effects::reactive::ReactiveEffects,
    types::identifiers::{ChannelId, ContextId},
    AuthorityId,
};
use aura_ui::{
    semantic_lifecycle::{
        begin_exact_handoff_operation, UiOperationTransfer, UiOperationTransferScope,
    },
    UiController,
};
use serde_json::{from_str, to_string};
use std::cell::RefCell as StdRefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsValue;

use crate::browser_promises::await_browser_promise_with_timeout;
use crate::harness::channel_selection::{
    authoritative_channel_binding, selected_authority_id, selected_channel_binding,
    selected_channel_id, selected_device_id, SelectionError, WeakChannelSelection,
};
use crate::harness_bridge::{
    BootstrapHandoff, PendingAccountBootstrapSource, RuntimeIdentityStageSource,
};
use crate::{
    active_storage_prefix, load_selected_runtime_identity, selected_runtime_identity_key,
    submit_runtime_bootstrap_handoff,
    task_owner::shared_web_task_owner,
    workflows::{
        self, AccountCreationStageMode, CurrentRuntimeIdentity, DeviceEnrollmentImportRequest,
        RebootstrapPolicy,
    },
};

pub(crate) fn browser_settings_section(
    section: SettingsSection,
) -> aura_ui::model::SettingsSection {
    match section {
        SettingsSection::Devices => aura_ui::model::SettingsSection::Devices,
    }
}

#[derive(Debug)]
pub(crate) struct BrowserSemanticBridgeRequest(SemanticCommandRequest);

impl BrowserSemanticBridgeRequest {
    pub(crate) fn from_json(request_json: &str) -> Result<Self, JsValue> {
        from_str::<SemanticCommandRequest>(request_json)
            .map(Self)
            .map_err(|error| JsValue::from_str(&format!("invalid semantic command request: {error}")))
    }

    pub(crate) async fn submit(
        self,
        controller: Arc<UiController>,
    ) -> Result<BrowserSemanticBridgeResponse, JsValue> {
        submit_semantic_command(controller, self.0)
            .await
            .map(BrowserSemanticBridgeResponse)
    }
}

#[derive(Debug)]
pub(crate) struct BrowserSemanticBridgeResponse(SemanticCommandResponse);

impl BrowserSemanticBridgeResponse {
    pub(crate) fn into_json(self) -> Result<String, JsValue> {
        to_string(&self.0).map_err(|error| {
            JsValue::from_str(&format!(
                "failed to serialize semantic command response: {error}"
            ))
        })
    }

    pub(crate) fn into_js_value(self) -> Result<JsValue, JsValue> {
        self.into_json().map(|response_json| JsValue::from_str(&response_json))
    }
}

#[derive(Clone, Debug)]
enum RoutedSemanticIntent {
    OpenScreen(ScreenId),
    CreateAccount {
        account_name: String,
    },
    CreateHome {
        home_name: String,
    },
    CreateChannel {
        channel_name: String,
    },
    StartDeviceEnrollment {
        device_name: String,
        invitee_authority_id: AuthorityId,
    },
    ImportDeviceEnrollmentCode {
        code: String,
    },
    OpenSettingsSection(SettingsSection),
    RemoveSelectedDevice {
        device_id: String,
    },
    SwitchAuthority {
        current_authority_id: Option<String>,
        authority_id: AuthorityId,
    },
    CreateContactInvitation {
        receiver_authority_id: AuthorityId,
    },
    AcceptContactInvitation {
        code: String,
    },
    AcceptPendingChannelInvitation,
    JoinChannel {
        channel_name: String,
    },
    InviteActorToChannel {
        authority_id: AuthorityId,
        channel_id: ChannelId,
        context_id: Option<ContextId>,
        channel_name: Option<String>,
    },
    SendChatMessage {
        message: String,
        channel: WeakChannelSelection,
    },
}

#[derive(Clone, Debug)]
enum RouteSemanticIntentError {
    Selection(SelectionError),
    InvalidAuthorityId(String),
    InvalidChannelId(String),
    InvalidContextId(String),
    MissingAuthoritativeChannelBinding,
}

impl RouteSemanticIntentError {
    fn invalid_authority_id(error: impl ToString) -> Self {
        Self::InvalidAuthorityId(error.to_string())
    }

    fn invalid_channel_id(error: impl ToString) -> Self {
        Self::InvalidChannelId(error.to_string())
    }

    #[must_use]
    fn into_js_value(self) -> JsValue {
        let detail = match self {
            Self::Selection(error) => error.detail(),
            Self::InvalidAuthorityId(error) => format!("invalid authority id: {error}"),
            Self::InvalidChannelId(error) => format!("invalid channel id: {error}"),
            Self::InvalidContextId(error) => format!("invalid context id: {error}"),
            Self::MissingAuthoritativeChannelBinding => {
                "invite_actor_to_channel requires an authoritative channel binding".to_string()
            }
        };
        JsValue::from_str(&detail)
    }
}

impl From<SelectionError> for RouteSemanticIntentError {
    fn from(value: SelectionError) -> Self {
        Self::Selection(value)
    }
}

fn semantic_response_with_handle(
    handle: UiOperationHandle,
    value: SemanticCommandValue,
) -> SemanticCommandResponse {
    SemanticCommandResponse {
        submission: SubmissionState::Accepted,
        handle: SemanticSubmissionHandle {
            ui_operation: Some(handle),
        },
        value,
    }
}

fn submission_value_matches_contract(
    contract: SubmissionValueContract,
    value: &SemanticCommandValue,
) -> bool {
    match (contract, value) {
        (SubmissionValueContract::None, SemanticCommandValue::None) => true,
        (
            SubmissionValueContract::ContactInvitationCode,
            SemanticCommandValue::ContactInvitationCode { .. },
        ) => true,
        (
            SubmissionValueContract::AuthoritativeChannelBinding,
            SemanticCommandValue::AuthoritativeChannelBinding { .. },
        ) => true,
        _ => false,
    }
}

fn declared_immediate_response(
    contract: &SharedActionContract,
    value: SemanticCommandValue,
) -> Result<SemanticCommandResponse, JsValue> {
    match &contract.submission {
        SubmissionContract::Immediate {
            value: expected_value,
        } if submission_value_matches_contract(*expected_value, &value) => {
            Ok(SemanticCommandResponse::accepted(value))
        }
        SubmissionContract::Immediate {
            value: expected_value,
        } => Err(JsValue::from_str(&format!(
            "intent {:?} declared immediate submission value {:?}, observed {:?}",
            contract.intent, expected_value, value
        ))),
        SubmissionContract::OperationHandle {
            operation_id,
            value: expected_value,
        } => Err(JsValue::from_str(&format!(
            "intent {:?} requires operation handle submission for {} with value {:?}",
            contract.intent, operation_id.0, expected_value
        ))),
    }
}

fn declared_immediate_unit_response(
    contract: &SharedActionContract,
) -> Result<SemanticCommandResponse, JsValue> {
    declared_immediate_response(contract, SemanticCommandValue::None)
}

fn declared_handle_response(
    contract: &SharedActionContract,
    handle: UiOperationHandle,
    value: SemanticCommandValue,
) -> Result<SemanticCommandResponse, JsValue> {
    match &contract.submission {
        SubmissionContract::OperationHandle {
            operation_id,
            value: expected_value,
        } if operation_id == handle.id()
            && submission_value_matches_contract(*expected_value, &value) =>
        {
            Ok(semantic_response_with_handle(handle, value))
        }
        SubmissionContract::OperationHandle {
            operation_id,
            value: expected_value,
        } => Err(JsValue::from_str(&format!(
            "intent {:?} declared handle submission for {} with value {:?}, observed handle {} and value {:?}",
            contract.intent, operation_id.0, expected_value, handle.id().0, value
        ))),
        SubmissionContract::Immediate { value: expected_value } => Err(JsValue::from_str(
            &format!(
                "intent {:?} declared immediate submission with value {:?}, observed handle {}",
                contract.intent, expected_value, handle.id().0
            ),
        )),
    }
}

fn declared_handle_unit_response(
    contract: &SharedActionContract,
    handle: UiOperationHandle,
) -> Result<SemanticCommandResponse, JsValue> {
    declared_handle_response(contract, handle, SemanticCommandValue::None)
}

fn begin_declared_handoff_operation(
    controller: Arc<UiController>,
    contract: &SharedActionContract,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    scope: UiOperationTransferScope,
) -> Result<(UiOperationHandle, UiOperationTransfer), JsValue> {
    match &contract.submission {
        SubmissionContract::OperationHandle {
            operation_id: expected_operation_id,
            ..
        } if expected_operation_id == &operation_id => Ok(begin_exact_handoff_operation(
            controller,
            operation_id,
            kind,
            scope,
        )),
        SubmissionContract::OperationHandle {
            operation_id: expected_operation_id,
            ..
        } => Err(JsValue::from_str(&format!(
            "intent {:?} declared handoff operation {}, observed {}",
            contract.intent, expected_operation_id.0, operation_id.0
        ))),
        SubmissionContract::Immediate { .. } => Err(JsValue::from_str(&format!(
            "intent {:?} declared immediate submission but attempted handoff for {}",
            contract.intent, operation_id.0
        ))),
    }
}

fn begin_declared_exact_ui_operation(
    controller: Arc<UiController>,
    contract: &SharedActionContract,
    operation_id: OperationId,
) -> Result<UiOperationHandle, JsValue> {
    match &contract.submission {
        SubmissionContract::OperationHandle {
            operation_id: expected_operation_id,
            ..
        } if expected_operation_id == &operation_id => {
            Ok(begin_exact_ui_operation(controller, operation_id))
        }
        SubmissionContract::OperationHandle {
            operation_id: expected_operation_id,
            ..
        } => Err(JsValue::from_str(&format!(
            "intent {:?} declared exact ui operation {}, observed {}",
            contract.intent, expected_operation_id.0, operation_id.0
        ))),
        SubmissionContract::Immediate { .. } => Err(JsValue::from_str(&format!(
            "intent {:?} declared immediate submission but attempted exact-handle issue for {}",
            contract.intent, operation_id.0
        ))),
    }
}

fn begin_exact_ui_operation(
    controller: Arc<UiController>,
    operation_id: OperationId,
) -> UiOperationHandle {
    let handle = Rc::new(StdRefCell::new(None::<UiOperationHandle>));
    let captured_handle = handle.clone();
    let captured_operation_id = operation_id;
    crate::harness_bridge::apply_browser_ui_mutation(controller, move |controller| {
        let next_handle =
            controller.begin_exact_operation_handle_submission(captured_operation_id.clone());
        captured_handle.borrow_mut().replace(next_handle);
    });
    let exact_handle = handle
        .borrow_mut()
        .take()
        .unwrap_or_else(|| panic!("exact browser operation submission must return a handle"));
    exact_handle
}

fn spawn_background_semantic_task(
    label: &'static str,
    task: impl std::future::Future<Output = Result<(), JsValue>> + 'static,
) {
    shared_web_task_owner().spawn_local(async move {
        yield_to_browser_event_loop().await;
        if let Err(error) = task.await {
            let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
            update_semantic_debug(label, Some(&detail));
            web_sys::console::error_1(
                &format!("[web-harness] background semantic task {label} failed: {detail}").into(),
            );
        }
    });
}

async fn yield_to_browser_event_loop() {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = await_browser_promise_with_timeout(
            js_sys::Promise::resolve(&JsValue::UNDEFINED),
            250,
            aura_ui::FrontendUiOperation::BackgroundSync,
            "WEB_BROWSER_EVENT_LOOP_YIELD_REJECTED",
            "WEB_BROWSER_EVENT_LOOP_YIELD_TIMEOUT",
            "WEB_BROWSER_EVENT_LOOP_YIELD_TIMEOUT_SCHEDULE_FAILED",
            "WEB_BROWSER_EVENT_LOOP_YIELD_TIMEOUT_DROPPED",
            "browser event-loop yield",
            None,
        )
        .await;
    }
}

fn spawn_handoff_workflow_task<T, Fut, Success, SuccessFut>(
    label: &'static str,
    controller: Arc<UiController>,
    transfer: UiOperationTransfer,
    panic_context: &'static str,
    workflow: Fut,
    on_success: Success,
) where
    T: 'static,
    Fut: std::future::Future<Output = aura_app::ui_contract::WorkflowTerminalOutcome<T>> + 'static,
    Success: FnOnce(Arc<UiController>, T) -> SuccessFut + 'static,
    SuccessFut: std::future::Future<Output = Result<(), JsValue>> + 'static,
{
    spawn_background_semantic_task(label, async move {
        let value = transfer
            .run_workflow(controller.clone(), panic_context, workflow)
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        on_success(controller, value).await
    });
}

async fn start_and_monitor_runtime_device_removal(
    controller: Arc<UiController>,
    device_id: String,
) -> Result<(), JsValue> {
    let app_core = controller.app_core().clone();
    let ceremony_handle = ceremony_workflows::start_device_removal_ceremony(&app_core, device_id)
        .await
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let status_handle = ceremony_handle.status_handle();
    match ceremony_workflows::get_key_rotation_ceremony_status(&app_core, &status_handle).await {
        Ok(status) if status.is_complete => {
            settings_workflows::refresh_settings_from_runtime(&app_core)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.request_rerender();
            return Ok(());
        }
        Ok(_) => {}
        Err(error) => {
            return Err(JsValue::from_str(&error.to_string()));
        }
    }
    shared_web_task_owner().spawn_local({
        let controller = controller.clone();
        let app_core = app_core.clone();
        async move {
            let lifecycle = ceremony_workflows::monitor_key_rotation_ceremony_with_policy(
                &app_core,
                &status_handle,
                ceremony_workflows::CeremonyPollPolicy {
                    interval: std::time::Duration::from_millis(250),
                    max_attempts: 160,
                    rollback_on_failure: true,
                    refresh_settings_on_complete: true,
                },
                |_| {
                    controller.request_rerender();
                },
                |duration| {
                    let app_core = app_core.clone();
                    async move {
                        let sleep_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                        let _ = time_workflows::sleep_ms(&app_core, sleep_ms).await;
                    }
                },
            )
            .await;

            match lifecycle {
                Ok(lifecycle) => {
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness] device_removal_monitor state={:?};complete={};failed={};attempts={}",
                            lifecycle.state,
                            lifecycle.status.is_complete,
                            lifecycle.status.has_failed,
                            lifecycle.attempts
                        )
                        .into(),
                    );
                    controller.request_rerender();
                }
                Err(error) => {
                    web_sys::console::warn_1(
                        &format!("[web-harness] device_removal_monitor failed: {error}").into(),
                    );
                }
            }
        }
    });
    Ok(())
}

pub(crate) fn update_semantic_debug(event: &str, detail: Option<&str>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let debug = js_sys::Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_SEMANTIC_DEBUG__"),
    )
    .ok()
    .filter(|value| !value.is_null() && !value.is_undefined())
    .unwrap_or_else(|| {
        let object = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            window.as_ref(),
            &JsValue::from_str("__AURA_SEMANTIC_DEBUG__"),
            object.as_ref(),
        );
        object.into()
    });
    let _ = js_sys::Reflect::set(
        &debug,
        &JsValue::from_str("last_event"),
        &JsValue::from_str(event),
    );
    let _ = js_sys::Reflect::set(
        &debug,
        &JsValue::from_str("last_detail"),
        &detail.map(JsValue::from_str).unwrap_or(JsValue::NULL),
    );
}

async fn route_semantic_intent(
    controller: &UiController,
    intent: IntentAction,
) -> Result<RoutedSemanticIntent, RouteSemanticIntentError> {
    match intent {
        IntentAction::OpenScreen(screen) => Ok(RoutedSemanticIntent::OpenScreen(screen)),
        IntentAction::CreateAccount { account_name } => {
            Ok(RoutedSemanticIntent::CreateAccount { account_name })
        }
        IntentAction::CreateHome { home_name } => {
            Ok(RoutedSemanticIntent::CreateHome { home_name })
        }
        IntentAction::CreateChannel { channel_name } => {
            Ok(RoutedSemanticIntent::CreateChannel { channel_name })
        }
        IntentAction::StartDeviceEnrollment {
            device_name,
            invitee_authority_id,
            ..
        } => Ok(RoutedSemanticIntent::StartDeviceEnrollment {
            device_name,
            invitee_authority_id: invitee_authority_id
                .parse::<AuthorityId>()
                .map_err(RouteSemanticIntentError::invalid_authority_id)?,
        }),
        IntentAction::ImportDeviceEnrollmentCode { code } => {
            Ok(RoutedSemanticIntent::ImportDeviceEnrollmentCode { code })
        }
        IntentAction::OpenSettingsSection(section) => {
            Ok(RoutedSemanticIntent::OpenSettingsSection(section))
        }
        IntentAction::RemoveSelectedDevice { device_id } => {
            let device_id = match device_id {
                Some(device_id) => device_id,
                None => selected_device_id(controller)?,
            };
            Ok(RoutedSemanticIntent::RemoveSelectedDevice { device_id })
        }
        IntentAction::SwitchAuthority { authority_id } => {
            Ok(RoutedSemanticIntent::SwitchAuthority {
                current_authority_id: selected_authority_id(controller),
                authority_id: authority_id
                    .parse::<AuthorityId>()
                    .map_err(RouteSemanticIntentError::invalid_authority_id)?,
            })
        }
        IntentAction::CreateContactInvitation {
            receiver_authority_id,
            ..
        } => Ok(RoutedSemanticIntent::CreateContactInvitation {
            receiver_authority_id: receiver_authority_id
                .parse::<AuthorityId>()
                .map_err(RouteSemanticIntentError::invalid_authority_id)?,
        }),
        IntentAction::AcceptContactInvitation { code } => {
            Ok(RoutedSemanticIntent::AcceptContactInvitation { code })
        }
        IntentAction::AcceptPendingChannelInvitation => {
            Ok(RoutedSemanticIntent::AcceptPendingChannelInvitation)
        }
        IntentAction::JoinChannel { channel_name } => {
            Ok(RoutedSemanticIntent::JoinChannel { channel_name })
        }
        IntentAction::InviteActorToChannel {
            authority_id,
            channel_id,
            context_id,
            channel_name,
        } => Ok(RoutedSemanticIntent::InviteActorToChannel {
            authority_id: authority_id
                .parse::<AuthorityId>()
                .map_err(RouteSemanticIntentError::invalid_authority_id)?,
            channel_id: channel_id
                .ok_or(RouteSemanticIntentError::MissingAuthoritativeChannelBinding)?
                .parse::<ChannelId>()
                .map_err(RouteSemanticIntentError::invalid_channel_id)?,
            context_id: context_id
                .map(|context_id| {
                    context_id.parse::<ContextId>().map_err(|error| {
                        RouteSemanticIntentError::InvalidContextId(error.to_string())
                    })
                })
                .transpose()?,
            channel_name,
        }),
        IntentAction::SendChatMessage { message } => Ok(RoutedSemanticIntent::SendChatMessage {
            message,
            channel: selected_channel_id(controller)?,
        }),
    }
}

async fn execute_semantic_intent(
    controller: Arc<UiController>,
    intent: RoutedSemanticIntent,
    contract: SharedActionContract,
) -> Result<SemanticCommandResponse, JsValue> {
    match intent {
        RoutedSemanticIntent::OpenScreen(screen) => {
            crate::harness_bridge::apply_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.set_screen(screen);
                },
            );
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::CreateAccount { account_name } => {
            update_semantic_debug("create_account_begin", Some(&account_name));
            let staged_account_name = account_name.clone();
            crate::harness_bridge::schedule_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.set_account_setup_state(false, staged_account_name, None);
                },
            )
            .await?;
            let stage_result =
                workflows::stage_account_creation(controller.app_core(), &account_name)
                    .await
                    .map_err(|error| JsValue::from_str(&error.user_message()))?;
            if stage_result.mode == AccountCreationStageMode::RuntimeInitialized {
                update_semantic_debug("create_account_runtime_path", Some(&account_name));
                controller.finalize_account_setup(ScreenId::Neighborhood);
                crate::harness_bridge::publish_semantic_controller_snapshot(controller);
            } else {
                update_semantic_debug("create_account_stage_start", Some(&account_name));
                submit_runtime_bootstrap_handoff(BootstrapHandoff::PendingAccountBootstrap {
                    account_name: account_name.clone(),
                    source: PendingAccountBootstrapSource::HarnessSemanticBridge,
                })
                .await
                .map_err(|error| JsValue::from_str(&error.user_message()))?;
                update_semantic_debug("create_account_handoff_done", Some(&account_name));
            }
            update_semantic_debug("create_account_return", Some(&account_name));
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::CreateHome { home_name } => {
            context_workflows::create_home(controller.app_core(), Some(home_name), None)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::CreateChannel { channel_name } => {
            let timestamp_ms = context_workflows::current_time_ms(controller.app_core())
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let created = messaging_workflows::create_channel_with_authoritative_binding(
                controller.app_core(),
                &channel_name,
                None,
                &[],
                1,
                timestamp_ms,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            declared_immediate_response(
                &contract,
                ChannelBindingWitness::new(
                    created.channel_id.to_string(),
                    created.context_id.map(|context_id| context_id.to_string()),
                )
                .semantic_value(),
            )
        }
        RoutedSemanticIntent::StartDeviceEnrollment {
            device_name,
            invitee_authority_id,
        } => {
            crate::harness_bridge::apply_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.set_screen(ScreenId::Settings);
                    controller
                        .set_settings_section(browser_settings_section(SettingsSection::Devices));
                },
            );
            let (handle, transfer) = begin_declared_handoff_operation(
                controller.clone(),
                &contract,
                OperationId::device_enrollment(),
                SemanticOperationKind::StartDeviceEnrollment,
                UiOperationTransferScope::StartDeviceEnrollment,
            )?;
            let app_core = controller.app_core().clone();
            let controller = controller.clone();
            let workflow_device_name = device_name.clone();
            let success_device_name = device_name;
            let workflow_instance_id = handle.instance_id().clone();
            spawn_handoff_workflow_task(
                "start_device_enrollment_failed",
                controller,
                transfer,
                "start_device_enrollment callback",
                async move {
                    ceremony_workflows::start_device_enrollment_ceremony_with_terminal_status(
                        &app_core,
                        workflow_device_name,
                        invitee_authority_id,
                        Some(workflow_instance_id),
                    )
                    .await
                },
                move |controller, start| async move {
                    controller.write_clipboard(&start.enrollment_code);
                    controller.push_runtime_fact(RuntimeFact::DeviceEnrollmentCodeReady {
                        device_name: Some(success_device_name),
                        code_len: Some(start.enrollment_code.len()),
                        code: Some(start.enrollment_code),
                    });
                    Ok(())
                },
            );
            declared_handle_unit_response(&contract, handle)
        }
        RoutedSemanticIntent::ImportDeviceEnrollmentCode { code } => {
            let app_core = controller.app_core().clone();
            let runtime = runtime_workflows::require_runtime(&app_core)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let storage_prefix = active_storage_prefix();
            let result = workflows::accept_device_enrollment_import(
                &app_core,
                DeviceEnrollmentImportRequest {
                    code: &code,
                    current_runtime_identity: CurrentRuntimeIdentity {
                        authority_id: runtime.authority_id(),
                        selected_runtime_identity: load_selected_runtime_identity(
                            &selected_runtime_identity_key(&storage_prefix),
                        )
                        .map_err(|error| JsValue::from_str(&error.to_string()))?,
                    },
                    storage_prefix: &storage_prefix,
                    rebootstrap_policy: RebootstrapPolicy::StageIfRequired,
                    operation: crate::WebUiOperation::ImportDeviceEnrollmentCode,
                },
            )
            .await
            .map_err(|error| JsValue::from_str(&error.user_message()))?;
            if result.rebootstrap_required {
                let staged_runtime_identity = result.staged_runtime_identity;
                submit_runtime_bootstrap_handoff(BootstrapHandoff::RuntimeIdentityStaged {
                    authority_id: staged_runtime_identity.authority_id,
                    device_id: staged_runtime_identity.device_id,
                    source: RuntimeIdentityStageSource::ImportDeviceEnrollment,
                })
                .await
                .map_err(|error| JsValue::from_str(&error.user_message()))?;
                return declared_immediate_unit_response(&contract);
            }
            crate::harness_bridge::apply_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.finalize_account_setup(ScreenId::Neighborhood);
                },
            );
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::OpenSettingsSection(section) => {
            crate::harness_bridge::apply_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.set_screen(ScreenId::Settings);
                    controller.set_settings_section(browser_settings_section(section));
                },
            );
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::RemoveSelectedDevice { device_id } => {
            crate::harness_bridge::apply_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.set_screen(ScreenId::Settings);
                    controller
                        .set_settings_section(browser_settings_section(SettingsSection::Devices));
                },
            );
            start_and_monitor_runtime_device_removal(controller.clone(), device_id).await?;
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::SwitchAuthority {
            current_authority_id,
            authority_id,
        } => {
            crate::harness_bridge::apply_browser_ui_mutation(
                controller.clone(),
                move |controller| {
                    controller.set_screen(ScreenId::Settings);
                    controller.set_settings_section(aura_ui::model::SettingsSection::Authority);
                },
            );
            if current_authority_id.as_deref() == Some(authority_id.to_string().as_str()) {
                return declared_immediate_unit_response(&contract);
            }
            if !controller.request_authority_switch(authority_id) {
                return Err(JsValue::from_str(
                    "authority switching is not available for this frontend",
                ));
            }
            declared_immediate_unit_response(&contract)
        }
        RoutedSemanticIntent::CreateContactInvitation {
            receiver_authority_id,
        } => {
            let (handle, transfer) = begin_declared_handoff_operation(
                controller.clone(),
                &contract,
                OperationId::invitation_create(),
                SemanticOperationKind::CreateContactInvitation,
                UiOperationTransferScope::CreateInvitation,
            )?;
            let app_core = controller.app_core().clone();
            let nickname = {
                let core = app_core.read().await;
                core.read(&*SETTINGS_SIGNAL)
                    .await
                    .ok()
                    .map(|settings| settings.nickname_suggestion.trim().to_string())
                    .filter(|name| !name.is_empty())
            };
            let code = transfer
                .run_workflow(
                    controller.clone(),
                    "create_contact_invitation callback",
                    invitation_workflows::create_contact_invitation_code_with_terminal_status(
                        &app_core,
                        receiver_authority_id.clone(),
                        nickname,
                        None,
                        None,
                        Some(handle.instance_id().clone()),
                    ),
                )
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.write_clipboard(&code);
            controller.remember_invitation_code(&code);
            controller.push_runtime_fact(RuntimeFact::InvitationCodeReady {
                receiver_authority_id: Some(receiver_authority_id.to_string()),
                source_operation: aura_app::ui::contract::OperationId::invitation_create(),
                code: Some(code.clone()),
            });
            declared_handle_response(
                &contract,
                handle,
                SemanticCommandValue::ContactInvitationCode { code },
            )
        }
        RoutedSemanticIntent::AcceptContactInvitation { code } => {
            let app_core = controller.app_core().clone();
            let (handle, transfer) = begin_declared_handoff_operation(
                controller.clone(),
                &contract,
                OperationId::invitation_accept(),
                SemanticOperationKind::AcceptContactInvitation,
                UiOperationTransferScope::AcceptInvitation,
            )?;
            update_semantic_debug("accept_contact_invitation_import_start", None);
            web_sys::console::log_1(&"[web-harness] accept_contact_invitation import_start".into());
            let invitation = invitation_workflows::import_invitation_details(&app_core, &code)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let invitation_info = invitation.info().clone();
            let instance_id = handle.instance_id().clone();
            let transfer_instance_id = transfer.instance_id().clone();
            let transfer_operation_id = transfer.operation_id().clone();
            let controller = controller.clone();
            controller.inject_message("debug:accept_contact:spawn_enqueued");
            controller.push_log("debug:accept_contact:spawn_enqueued");
            spawn_background_semantic_task("accept_contact_invitation_failed", async move {
                controller.inject_message("debug:accept_contact:workflow_start");
                controller.push_log("debug:accept_contact:workflow_start");
                update_semantic_debug(
                    "accept_contact_invitation_workflow_start",
                    Some(transfer_instance_id.0.as_str()),
                );
                web_sys::console::log_1(
                    &format!(
                        "[web-harness] accept_contact_invitation workflow_start instance={}",
                        transfer_instance_id.0
                    )
                    .into(),
                );
                let result = transfer
                    .run_workflow(
                        controller.clone(),
                        "accept_contact_invitation callback",
                        invitation_workflows::accept_imported_invitation_with_terminal_status(
                            &app_core,
                            invitation,
                            Some(instance_id),
                        ),
                    )
                    .await;
                update_semantic_debug(
                    "accept_contact_invitation_workflow_done",
                    Some(if result.is_ok() { "ok" } else { "err" }),
                );
                controller.inject_message(if result.is_ok() {
                    "debug:accept_contact:workflow_done:ok"
                } else {
                    "debug:accept_contact:workflow_done:err"
                });
                controller.push_log(if result.is_ok() {
                    "debug:accept_contact:workflow_done:ok"
                } else {
                    "debug:accept_contact:workflow_done:err"
                });
                web_sys::console::log_1(
                    &format!(
                        "[web-harness] accept_contact_invitation workflow_done status={}",
                        if result.is_ok() { "ok" } else { "err" }
                    )
                    .into(),
                );
                let accepted_contact = match &result {
                    Ok(()) => match &invitation_info.invitation_type {
                        InvitationBridgeType::Contact { nickname } => {
                            let display_name = nickname
                                .clone()
                                .filter(|value| !value.trim().is_empty())
                                .unwrap_or_else(|| invitation_info.sender_id.to_string());
                            Some((invitation_info.sender_id, display_name))
                        }
                        _ => None,
                    },
                    Err(_) => None,
                };
                update_semantic_debug(
                    "accept_contact_invitation_apply_terminal",
                    Some(transfer_instance_id.0.as_str()),
                );
                controller.inject_message("debug:accept_contact:apply_terminal");
                controller.push_log("debug:accept_contact:apply_terminal");
                web_sys::console::log_1(
                    &format!(
                        "[web-harness] accept_contact_invitation apply_terminal instance={}",
                        transfer_instance_id.0
                    )
                    .into(),
                );
                let applied_snapshot =
                    crate::harness_bridge::publish_semantic_controller_snapshot(controller.clone());
                let applied_state = applied_snapshot
                    .operation_state_for_instance(&transfer_operation_id, &transfer_instance_id)
                    .map(|state| format!("{state:?}"))
                    .unwrap_or_else(|| "Missing".to_string());
                controller.push_log(&format!(
                    "debug:accept_contact:published_terminal:{}",
                    applied_state
                ));
                if let Some((authority_id, display_name)) = accepted_contact {
                    update_semantic_debug(
                        "accept_contact_invitation_complete_runtime",
                        Some(display_name.as_str()),
                    );
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness] accept_contact_invitation complete_runtime display_name={display_name}"
                        )
                        .into(),
                    );
                    crate::harness_bridge::apply_browser_ui_mutation(
                        controller.clone(),
                        move |controller| {
                            controller.complete_runtime_contact_invitation_acceptance(
                                authority_id,
                                display_name,
                            );
                        },
                    );
                    spawn_background_semantic_task(
                        "accept_contact_invitation_post_followups",
                        async move {
                            update_semantic_debug("accept_contact_invitation_post_followups", None);
                            invitation_workflows::run_post_contact_accept_followups(
                                &app_core,
                                authority_id,
                            )
                            .await;
                            let _ = system_workflows::refresh_account(&app_core).await;
                            Ok(())
                        },
                    );
                }
                update_semantic_debug("accept_contact_invitation_done", None);
                controller.push_log("debug:accept_contact:done");
                web_sys::console::log_1(&"[web-harness] accept_contact_invitation done".into());
                result.map_err(|error| JsValue::from_str(&error.to_string()))
            });
            declared_handle_unit_response(&contract, handle)
        }
        RoutedSemanticIntent::AcceptPendingChannelInvitation => {
            let (handle, transfer) = begin_declared_handoff_operation(
                controller.clone(),
                &contract,
                OperationId::invitation_accept(),
                SemanticOperationKind::AcceptPendingChannelInvitation,
                UiOperationTransferScope::AcceptPendingChannelInvitation,
            )?;
            let app_core = controller.app_core().clone();
            let workflow_app_core = app_core.clone();
            let instance_id = handle.instance_id().clone();
            let controller = controller.clone();
            spawn_handoff_workflow_task(
                "accept_pending_channel_invitation_failed",
                controller,
                transfer,
                "accept_pending_channel_invitation callback",
                async move {
                    invitation_workflows::accept_pending_channel_invitation_with_binding_terminal_status(
                        &workflow_app_core,
                        Some(instance_id),
                    )
                    .await
                },
                |_controller, _accepted| async move { Ok(()) },
            );
            declared_handle_unit_response(&contract, handle)
        }
        RoutedSemanticIntent::JoinChannel { channel_name } => {
            let handle = begin_declared_exact_ui_operation(
                controller.clone(),
                &contract,
                OperationId::join_channel(),
            )?;
            messaging_workflows::join_channel_by_name_with_instance(
                controller.app_core(),
                &channel_name,
                Some(handle.instance_id().clone()),
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let binding = selected_channel_binding(&controller)
                .await
                .map_err(|error| JsValue::from_str(&error.detail()))?;
            declared_handle_response(&contract, handle, binding.semantic_value())
        }
        RoutedSemanticIntent::InviteActorToChannel {
            authority_id,
            channel_id,
            context_id,
            channel_name,
        } => {
            let context_id = match context_id {
                Some(context_id) => context_id,
                None => {
                    let binding = authoritative_channel_binding(&controller, &channel_id)
                        .await
                        .map_err(|error| JsValue::from_str(&error.detail()))?;
                    binding
                        .context_id
                        .as_deref()
                        .ok_or_else(|| {
                            JsValue::from_str(
                                "invite_actor_to_channel requires an authoritative channel context",
                            )
                        })?
                        .parse::<ContextId>()
                        .map_err(|error| {
                            JsValue::from_str(&format!(
                                "invalid authoritative channel context for {channel_id}: {error}"
                            ))
                        })?
                }
            };
            let (handle, transfer) = begin_declared_handoff_operation(
                controller.clone(),
                &contract,
                OperationId::invitation_create(),
                SemanticOperationKind::InviteActorToChannel,
                UiOperationTransferScope::InviteActorToChannel,
            )?;
            let app_core = controller.app_core().clone();
            let authority_id = authority_id.to_string();
            let channel_hint = channel_name;
            let channel_id = channel_id.to_string();
            let context_id = Some(context_id);
            let instance_id = handle.instance_id().clone();
            let workflow_instance_id = instance_id.clone();
            let controller = controller.clone();
            let authority_id = authority_id.parse::<AuthorityId>().map_err(|error| {
                JsValue::from_str(&format!(
                    "invalid canonical invite authority id {authority_id}: {error}"
                ))
            })?;
            let channel_id = channel_id.parse::<ChannelId>().map_err(|error| {
                JsValue::from_str(&format!(
                    "invalid canonical invite channel id {channel_id}: {error}"
                ))
            })?;
            let authoritative_channel = messaging_workflows::authoritative_channel_ref(
                channel_id,
                context_id.expect("invite_actor_to_channel requires authoritative context"),
            );
            let workflow_app_core = app_core.clone();
            let workflow_authority_id = authority_id;
            let workflow_channel_id = channel_id;
            let workflow_context_id = context_id;
            let followup_app_core = app_core.clone();
            let followup_authority_id = workflow_authority_id;
            let followup_channel = authoritative_channel;
            let workflow = async move {
                let outcome =
                    messaging_workflows::invite_authority_to_channel_with_context_terminal_status(
                        &workflow_app_core,
                        workflow_authority_id,
                        workflow_channel_id,
                        workflow_context_id,
                        channel_hint.clone(),
                        Some(workflow_instance_id),
                        None,
                        None,
                    )
                    .await;
                outcome
            };
            spawn_handoff_workflow_task(
                "invite_actor_to_channel_failed",
                controller,
                transfer,
                "invite_actor_to_channel callback",
                workflow,
                move |_controller, _invitation_id| async move {
                    spawn_background_semantic_task(
                        "invite_actor_to_channel_post_followups",
                        async move {
                            messaging_workflows::run_post_channel_invite_followups(
                                &followup_app_core,
                                followup_authority_id,
                                followup_channel,
                            )
                            .await;
                            Ok(())
                        },
                    );
                    Ok(())
                },
            );
            declared_handle_unit_response(&contract, handle)
        }
        RoutedSemanticIntent::SendChatMessage { message, channel } => {
            let (handle, transfer) = begin_declared_handoff_operation(
                controller.clone(),
                &contract,
                OperationId::send_message(),
                SemanticOperationKind::SendChatMessage,
                UiOperationTransferScope::SendChatMessage,
            )?;
            let app_core = controller.app_core().clone();
            let workflow_app_core = app_core.clone();
            let controller = controller.clone();
            let instance_id = handle.instance_id().clone();
            spawn_background_semantic_task("send_chat_message_failed", async move {
                transfer
                    .run_workflow(
                        controller,
                        "send_chat_message_failed",
                        messaging_workflows::handoff::send_chat_message(
                            &workflow_app_core,
                            messaging_workflows::handoff::SendChatMessageRequest {
                                target: messaging_workflows::handoff::SendChatTarget::ChannelId(
                                    channel.channel_id().clone(),
                                ),
                                content: message,
                                operation_instance_id: Some(instance_id),
                            },
                        ),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|error| JsValue::from_str(&error.to_string()))
            });
            declared_handle_unit_response(&contract, handle)
        }
    }
}

pub(crate) async fn submit_semantic_command(
    controller: Arc<UiController>,
    request: SemanticCommandRequest,
) -> Result<SemanticCommandResponse, JsValue> {
    let contract = request.intent.contract();
    let routed = route_semantic_intent(controller.as_ref(), request.intent)
        .await
        .map_err(RouteSemanticIntentError::into_js_value)?;
    execute_semantic_intent(controller, routed, contract).await
}

#[cfg(test)]
mod tests {
    use super::{
        BrowserSemanticBridgeRequest, BrowserSemanticBridgeResponse, SemanticCommandResponse,
    };
    use aura_app::ui::contract::OperationId;
    use aura_app::scenario_contract::{IntentAction, SubmissionContract, SubmissionValueContract};
    use serde_json::json;

    #[test]
    fn start_device_enrollment_contract_requires_handle_submission() {
        let contract = IntentAction::StartDeviceEnrollment {
            device_name: "Browser Device".to_string(),
            code_name: "Browser Code".to_string(),
            invitee_authority_id:
                "aura:a:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
        }
        .contract();

        assert_eq!(
            contract.submission,
            SubmissionContract::OperationHandle {
                operation_id: OperationId::device_enrollment(),
                value: SubmissionValueContract::None,
            }
        );
    }

    #[test]
    fn join_channel_contract_requires_handle_with_binding() {
        let contract = IntentAction::JoinChannel {
            channel_name: "General".to_string(),
        }
        .contract();

        assert_eq!(
            contract.submission,
            SubmissionContract::OperationHandle {
                operation_id: OperationId::join_channel(),
                value: SubmissionValueContract::AuthoritativeChannelBinding,
            }
        );
    }

    #[test]
    fn browser_semantic_bridge_request_rejects_invalid_json() {
        let error = BrowserSemanticBridgeRequest::from_json("{")
            .expect_err("invalid semantic command json should fail");
        assert!(
            error
                .as_string()
                .unwrap_or_default()
                .contains("invalid semantic command request"),
            "bridge request parse errors should stay typed and contextual"
        );
    }

    #[test]
    fn browser_semantic_bridge_response_serializes_contract_shape() {
        let response =
            BrowserSemanticBridgeResponse(SemanticCommandResponse::accepted_without_value());
        let serialized = response
            .into_json()
            .expect("semantic bridge response should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized response should be valid json");
        assert_eq!(parsed["status"], json!("accepted"));
    }

    #[test]
    fn execute_semantic_intent_uses_declared_submission_helpers() {
        let source = include_str!("commands.rs");
        let start = source
            .find("async fn execute_semantic_intent(")
            .expect("execute_semantic_intent function");
        let end = source[start..]
            .find("pub(crate) async fn submit_semantic_command(")
            .map(|offset| start + offset)
            .expect("submit_semantic_command marker");
        let body = &source[start..end];

        for forbidden in [
            "SemanticCommandResponse::accepted_without_value()",
            "begin_exact_handoff_operation(",
            "begin_exact_ui_operation(",
            "semantic_response_with_handle(",
            "semantic_unit_result_with_handle(",
            "semantic_channel_result(",
            "semantic_channel_result_with_handle(",
        ] {
            assert!(
                !body.contains(forbidden),
                "execute_semantic_intent should use declared submission helpers instead of `{forbidden}`"
            );
        }
    }

    #[test]
    fn invite_actor_to_channel_does_not_reintroduce_weak_name_fallback() {
        let source = include_str!("commands.rs");
        let start = source
            .find("RoutedSemanticIntent::InviteActorToChannel {")
            .expect("invite_actor_to_channel branch");
        let end = source[start..]
            .find("RoutedSemanticIntent::SendChatMessage")
            .map(|offset| start + offset)
            .expect("send_chat_message branch marker");
        let body = &source[start..end];
        assert!(
            body.contains("let channel_hint = channel_name;"),
            "browser invite path should forward authoritative channel metadata directly"
        );
        assert!(
            !body.contains("canonical_channel_name_hint"),
            "browser invite path must not recover canonical channel names from weak observed state"
        );
    }
}
