//! JavaScript harness API bridge for browser-based testing.
//!
//! Exposes the UiController to JavaScript via window.harness, enabling the test
//! harness to send keys, capture screenshots, and query UI state from Playwright.

use aura_app::ui::contract::{ListId, ModalId, RenderHeartbeat, ScreenId, UiSnapshot};
use aura_app::ui::scenarios::{
    IntentAction, SemanticCommandRequest, SemanticCommandResponse, SettingsSection,
};
use aura_app::ui::types::BootstrapRuntimeIdentity;
use aura_app::ui::workflows::account as account_workflows;
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::invitation as invitation_workflows;
use aura_app::ui::workflows::messaging as messaging_workflows;
use aura_app::ui_contract::RuntimeFact;
use aura_core::{types::identifiers::ChannelId, AuthorityId, DeviceId};
use aura_ui::UiController;
use futures::channel::oneshot;
use js_sys::{Array, Function, Object, Reflect, JSON};
use serde_json::{from_str, to_string};
use serde_wasm_bindgen::to_value;
use std::cell::RefCell as StdRefCell;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::future_to_promise;

use crate::task_owner::shared_web_task_owner;

struct PendingUiPublish {
    value: JsValue,
    json: String,
    screen: ScreenId,
    modal: Option<ModalId>,
    operation_count: usize,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Arc<UiController>>> = const { RefCell::new(None) };
    static LAST_PUBLISHED_UI_STATE_JSON: RefCell<Option<String>> = const { RefCell::new(None) };
    static PENDING_UI_PUBLISH: RefCell<Option<PendingUiPublish>> = const { RefCell::new(None) };
    static RENDER_SEQ: RefCell<u64> = const { RefCell::new(0) };
    static UI_PUBLISH_RAF_PENDING: Cell<bool> = const { Cell::new(false) };
    static BOOTSTRAP_HANDOFF_SUBMITTER: RefCell<Option<Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise>>> = const { RefCell::new(None) };
    static RUNTIME_IDENTITY_STAGER: RefCell<Option<Arc<dyn Fn(String) -> js_sys::Promise>>> = const { RefCell::new(None) };
}

fn submit_create_account_in_background(controller: Arc<UiController>, nickname: String) {
    shared_web_task_owner().spawn_local(async move {
        web_sys::console::log_1(
            &format!("[web-harness] create_account start nickname={nickname}").into(),
        );
        controller.set_account_setup_state(false, nickname.clone(), None);
        let has_runtime = {
            let core = controller.app_core().read().await;
            core.runtime().is_some()
        };
        let result: Result<(), String> =
            account_workflows::initialize_runtime_account(controller.app_core(), nickname.clone())
                .await
                .map_err(|error| error.to_string());

        match result {
            Ok(()) => {
                if has_runtime {
                    controller.set_account_setup_state(true, "", None);
                }
                web_sys::console::log_1(
                    &format!("[web-harness] create_account ok nickname={nickname}").into(),
                );
            }
            Err(error) => {
                web_sys::console::error_1(
                    &format!("[web-harness] create_account error {error}").into(),
                );
            }
        }
    });
}

#[derive(Clone, Debug)]
pub enum BootstrapHandoff {
    InitialBootstrap,
    PendingAccountBootstrap {
        account_name: String,
        source: PendingAccountBootstrapSource,
    },
    RuntimeIdentityStaged {
        authority_id: AuthorityId,
        device_id: DeviceId,
        source: RuntimeIdentityStageSource,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum PendingAccountBootstrapSource {
    HarnessSemanticBridge,
    OnboardingUi,
}

#[derive(Clone, Copy, Debug)]
pub enum RuntimeIdentityStageSource {
    HarnessStaging,
    AuthoritySwitch,
    ImportDeviceEnrollment,
}

impl BootstrapHandoff {
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::InitialBootstrap => "initial_bootstrap".to_string(),
            Self::PendingAccountBootstrap {
                account_name,
                source,
            } => format!(
                "pending_account_bootstrap:{}:{}",
                match source {
                    PendingAccountBootstrapSource::HarnessSemanticBridge =>
                        "harness_semantic_bridge",
                    PendingAccountBootstrapSource::OnboardingUi => "onboarding_ui",
                },
                account_name
            ),
            Self::RuntimeIdentityStaged {
                authority_id,
                device_id,
                source,
            } => format!(
                "runtime_identity_staged:{}:{}:{}",
                match source {
                    RuntimeIdentityStageSource::HarnessStaging => "harness_staging",
                    RuntimeIdentityStageSource::AuthoritySwitch => "authority_switch",
                    RuntimeIdentityStageSource::ImportDeviceEnrollment =>
                        "import_device_enrollment",
                },
                authority_id,
                device_id
            ),
        }
    }
}

pub fn set_bootstrap_handoff_submitter(
    submitter: Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise>,
) {
    BOOTSTRAP_HANDOFF_SUBMITTER.with(|slot| {
        *slot.borrow_mut() = Some(submitter);
    });
}

pub fn set_runtime_identity_stager(stager: Arc<dyn Fn(String) -> js_sys::Promise>) {
    RUNTIME_IDENTITY_STAGER.with(|slot| {
        *slot.borrow_mut() = Some(stager);
    });
}

pub async fn submit_bootstrap_handoff(handoff: BootstrapHandoff) -> Result<(), JsValue> {
    let detail = handoff.detail();
    web_sys::console::log_1(
        &format!("[web-harness] submit_bootstrap_handoff start detail={detail}").into(),
    );
    let submitter = BOOTSTRAP_HANDOFF_SUBMITTER.with(|slot| slot.borrow().clone());
    let submitter =
        submitter.ok_or_else(|| JsValue::from_str("bootstrap handoff submitter is unavailable"))?;
    let promise = submitter(handoff);
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await?;
    web_sys::console::log_1(
        &format!("[web-harness] submit_bootstrap_handoff done detail={detail}").into(),
    );
    Ok(())
}

pub async fn stage_runtime_identity(serialized_identity: String) -> Result<(), JsValue> {
    web_sys::console::log_1(&"[web-harness] stage_runtime_identity start".into());
    let stager = RUNTIME_IDENTITY_STAGER.with(|slot| slot.borrow().clone());
    let stager =
        stager.ok_or_else(|| JsValue::from_str("runtime identity stager is unavailable"))?;
    let promise = stager(serialized_identity);
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await?;
    web_sys::console::log_1(&"[web-harness] stage_runtime_identity done".into());
    Ok(())
}

fn browser_screen(screen: ScreenId) -> Option<ScreenId> {
    match screen {
        ScreenId::Onboarding => Some(ScreenId::Onboarding),
        ScreenId::Neighborhood => Some(ScreenId::Neighborhood),
        ScreenId::Chat => Some(ScreenId::Chat),
        ScreenId::Contacts => Some(ScreenId::Contacts),
        ScreenId::Notifications => Some(ScreenId::Notifications),
        ScreenId::Settings => Some(ScreenId::Settings),
    }
}

fn browser_settings_section(section: SettingsSection) -> aura_ui::model::SettingsSection {
    match section {
        SettingsSection::Devices => aura_ui::model::SettingsSection::Devices,
    }
}

fn selected_channel_id(controller: &UiController) -> Result<ChannelId, JsValue> {
    let snapshot = controller.ui_snapshot();
    let selected = snapshot
        .selections
        .iter()
        .find(|selection| selection.list == ListId::Channels)
        .map(|selection| selection.item_id.clone())
        .or_else(|| {
            snapshot
                .lists
                .iter()
                .find(|list| list.id == ListId::Channels)
                .and_then(|list| {
                    if list.items.len() == 1 {
                        list.items.first().map(|item| item.id.clone())
                    } else {
                        None
                    }
                })
        })
        .ok_or_else(|| JsValue::from_str("no channel is selected"))?;
    selected
        .parse::<ChannelId>()
        .map_err(|error| JsValue::from_str(&format!("invalid selected channel id: {error}")))
}

fn selected_device_id(controller: &UiController) -> Result<String, JsValue> {
    let snapshot = controller.ui_snapshot();
    snapshot
        .selections
        .iter()
        .find(|selection| selection.list == ListId::Devices)
        .map(|selection| selection.item_id.clone())
        .or_else(|| {
            snapshot
                .lists
                .iter()
                .find(|list| list.id == ListId::Devices)
                .and_then(|list| {
                    if list.items.len() == 1 {
                        list.items.first().map(|item| item.id.clone())
                    } else {
                        None
                    }
                })
        })
        .ok_or_else(|| JsValue::from_str("no device is selected"))
}

fn selected_authority_id(controller: &UiController) -> Option<String> {
    let snapshot = controller.ui_snapshot();
    snapshot
        .selections
        .iter()
        .find(|selection| selection.list == ListId::Authorities)
        .map(|selection| selection.item_id.clone())
        .or_else(|| {
            snapshot
                .lists
                .iter()
                .find(|list| list.id == ListId::Authorities)
                .and_then(|list| list.items.iter().find(|item| item.selected))
                .map(|item| item.id.clone())
        })
}

async fn schedule_browser_ui_mutation(action: impl FnOnce() + 'static) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let (tx, rx) = oneshot::channel::<()>();
    let action = Rc::new(StdRefCell::new(Some(Box::new(action) as Box<dyn FnOnce()>)));
    let callback_action = action.clone();
    let callback = Closure::once(move || {
        if let Some(action) = callback_action.borrow_mut().take() {
            action();
        }
        let _ = tx.send(());
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 0)
        .map_err(|error| {
            JsValue::from_str(&format!("failed to schedule UI mutation: {error:?}"))
        })?;
    callback.forget();
    rx.await
        .map_err(|_| JsValue::from_str("scheduled UI mutation dropped before execution"))?;
    Ok(())
}

async fn submit_semantic_command(
    controller: Arc<UiController>,
    request: SemanticCommandRequest,
) -> Result<SemanticCommandResponse, JsValue> {
    match request.intent {
        IntentAction::OpenScreen(screen) => {
            let target =
                browser_screen(screen).ok_or_else(|| JsValue::from_str("unsupported screen"))?;
            let controller = controller.clone();
            schedule_browser_ui_mutation(move || controller.set_screen(target)).await?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateAccount { account_name } => {
            update_semantic_debug("create_account_begin", Some(&account_name));
            controller.set_account_setup_state(false, account_name.clone(), None);
            let has_runtime = controller
                .app_core()
                .try_read()
                .map(|core| core.runtime().is_some())
                .unwrap_or(false);
            if has_runtime {
                update_semantic_debug("create_account_runtime_path", Some(&account_name));
                submit_create_account_in_background(controller, account_name.clone());
            } else {
                update_semantic_debug("create_account_stage_start", Some(&account_name));
                web_sys::console::log_1(
                    &format!("[web-harness] create_account stage nickname={account_name}").into(),
                );
                super::stage_initial_web_account_bootstrap(&account_name)
                    .await
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                update_semantic_debug("create_account_staged", Some(&account_name));
                web_sys::console::log_1(
                    &format!("[web-harness] create_account staged nickname={account_name}").into(),
                );
                update_semantic_debug("create_account_handoff_start", Some(&account_name));
                submit_bootstrap_handoff(BootstrapHandoff::PendingAccountBootstrap {
                    account_name: account_name.clone(),
                    source: PendingAccountBootstrapSource::HarnessSemanticBridge,
                })
                .await?;
                update_semantic_debug("create_account_handoff_done", Some(&account_name));
            }
            update_semantic_debug("create_account_return", Some(&account_name));
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateHome { home_name } => {
            context_workflows::create_home(controller.app_core(), Some(home_name), None)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateChannel { channel_name } => {
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
            Ok(SemanticCommandResponse::accepted_channel_binding(
                created.channel_id.to_string(),
                created.context_id.map(|context_id| context_id.to_string()),
            ))
        }
        IntentAction::StartDeviceEnrollment {
            device_name,
            invitee_authority_id,
            ..
        } => {
            let controller_for_screen = controller.clone();
            schedule_browser_ui_mutation(move || {
                controller_for_screen.set_screen(ScreenId::Settings);
                controller_for_screen
                    .set_settings_section(browser_settings_section(SettingsSection::Devices));
            })
            .await?;
            let invitee_authority_id =
                invitee_authority_id
                    .parse::<AuthorityId>()
                    .map_err(|error| {
                        JsValue::from_str(&format!("invalid invitee authority id: {error}"))
                    })?;
            let start = ceremony_workflows::start_device_enrollment_ceremony(
                &controller.app_core(),
                device_name.clone(),
                invitee_authority_id,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.write_clipboard(&start.enrollment_code);
            controller.push_runtime_fact(RuntimeFact::DeviceEnrollmentCodeReady {
                device_name: Some(device_name),
                code_len: Some(start.enrollment_code.len()),
                code: Some(start.enrollment_code),
            });
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::ImportDeviceEnrollmentCode { code } => {
            let app_core = controller.app_core().clone();
            let invitation = invitation_workflows::import_invitation_details(&app_core, &code)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let invitation_info = invitation.info().clone();
            invitation_workflows::accept_device_enrollment_invitation(&app_core, &invitation_info)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::OpenSettingsSection(section) => {
            let controller = controller.clone();
            schedule_browser_ui_mutation(move || {
                controller.set_screen(ScreenId::Settings);
                controller.set_settings_section(browser_settings_section(section));
            })
            .await?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::RemoveSelectedDevice { device_id } => {
            let controller_for_screen = controller.clone();
            schedule_browser_ui_mutation(move || {
                controller_for_screen.set_screen(ScreenId::Settings);
                controller_for_screen
                    .set_settings_section(browser_settings_section(SettingsSection::Devices));
            })
            .await?;
            let device_id = match device_id {
                Some(device_id) => device_id,
                None => selected_device_id(&controller)?,
            };
            ceremony_workflows::start_device_removal_ceremony(&controller.app_core(), device_id)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::SwitchAuthority { authority_id } => {
            let controller_for_screen = controller.clone();
            schedule_browser_ui_mutation(move || {
                controller_for_screen.set_screen(ScreenId::Settings);
                controller_for_screen
                    .set_settings_section(aura_ui::model::SettingsSection::Authority);
            })
            .await?;
            if selected_authority_id(&controller).as_deref() == Some(authority_id.as_str()) {
                return Ok(SemanticCommandResponse::accepted_without_value());
            }
            let authority_id = authority_id
                .parse::<AuthorityId>()
                .map_err(|error| JsValue::from_str(&format!("invalid authority id: {error}")))?;
            if !controller.request_authority_switch(authority_id) {
                return Err(JsValue::from_str(
                    "authority switching is not available for this frontend",
                ));
            }
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateContactInvitation {
            receiver_authority_id,
            ..
        } => {
            let authority_id = receiver_authority_id
                .parse::<AuthorityId>()
                .map_err(|error| JsValue::from_str(&format!("invalid authority id: {error}")))?;
            let app_core = controller.app_core().clone();
            let invitation = invitation_workflows::create_contact_invitation(
                &app_core,
                authority_id.clone(),
                None,
                None,
                None,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let code =
                invitation_workflows::export_invitation(&app_core, invitation.invitation_id())
                    .await
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.write_clipboard(&code);
            controller.push_runtime_fact(RuntimeFact::InvitationCodeReady {
                receiver_authority_id: Some(authority_id.to_string()),
                source_operation: aura_app::ui::contract::OperationId::invitation_create(),
                code: Some(code.clone()),
            });
            Ok(SemanticCommandResponse::accepted_contact_invitation_code(
                code,
            ))
        }
        IntentAction::AcceptContactInvitation { code } => {
            let app_core = controller.app_core().clone();
            let invitation = invitation_workflows::import_invitation_details(&app_core, &code)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            invitation_workflows::accept_imported_invitation(&app_core, invitation)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::AcceptPendingChannelInvitation => {
            invitation_workflows::accept_pending_home_invitation(controller.app_core())
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::JoinChannel { channel_name } => {
            messaging_workflows::join_channel_by_name(controller.app_core(), &channel_name)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::InviteActorToChannel {
            authority_id,
            channel_id,
        } => {
            let authority_id = authority_id
                .parse::<AuthorityId>()
                .map_err(|error| JsValue::from_str(&format!("invalid authority id: {error}")))?;
            let channel_id = match channel_id {
                Some(channel_id) => channel_id
                    .parse::<ChannelId>()
                    .map_err(|error| JsValue::from_str(&format!("invalid channel id: {error}")))?,
                None => selected_channel_id(&controller)?,
            };
            messaging_workflows::invite_authority_to_channel(
                controller.app_core(),
                authority_id,
                channel_id,
                None,
                None,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::SendChatMessage { message } => {
            let timestamp_ms = context_workflows::current_time_ms(controller.app_core())
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let channel_id = selected_channel_id(&controller)?;
            messaging_workflows::send_message(
                controller.app_core(),
                channel_id,
                &message,
                timestamp_ms,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
    }
}

fn publish_ui_snapshot_now(
    window: &web_sys::Window,
    value: JsValue,
    json: String,
    screen: ScreenId,
    modal: Option<ModalId>,
    operation_count: usize,
) {
    let should_publish = LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        let mut last = slot.borrow_mut();
        if last.as_deref() == Some(json.as_str()) {
            false
        } else {
            *last = Some(json.clone());
            true
        }
    });
    if !should_publish {
        return;
    }

    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_CACHE__"),
        &value,
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_JSON__"),
        &JsValue::from_str(&json),
    );

    let binding_mode = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_UI_STATE"),
    )
    .ok()
    .and_then(|candidate| candidate.dyn_into::<Function>().ok())
    .map(|function| {
        if let Err(error) = function.call1(window.as_ref(), &JsValue::from_str(&json)) {
            log_js_callback_error("driver UI state push", &error);
        }
        "driver_push"
    })
    .unwrap_or("console_only");
    if binding_mode == "console_only" {
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-publish]binding={binding_mode};screen={screen:?};modal={modal:?};ops={operation_count}",
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-state]screen={screen:?};modal={modal:?};ops={operation_count};binding={binding_mode}",
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!("[aura-ui-json]{json}")));
    }
}

fn publish_render_heartbeat(window: &web_sys::Window, heartbeat: &RenderHeartbeat) {
    let Ok(value) = to_value(heartbeat) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };

    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT__"),
        &value,
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT_JSON__"),
        &JsValue::from_str(&json),
    );

    if let Ok(function) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_RENDER_HEARTBEAT"),
    )
    .and_then(|candidate| candidate.dyn_into::<Function>())
    {
        if let Err(error) = function.call1(window.as_ref(), &JsValue::from_str(&json)) {
            log_js_callback_error("driver render heartbeat push", &error);
        }
    }
}

fn log_js_callback_error(context: &str, error: &JsValue) {
    let detail = error
        .as_string()
        .or_else(|| {
            JSON::stringify(error)
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"));
    web_sys::console::error_1(&JsValue::from_str(&format!(
        "[web-harness] {context} failed: {detail}"
    )));
}

fn flush_pending_ui_snapshot(window: &web_sys::Window) {
    let Some(pending) = PENDING_UI_PUBLISH.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    let render_seq = RENDER_SEQ.with(|slot| {
        let mut seq = slot.borrow_mut();
        *seq = seq.saturating_add(1);
        *seq
    });
    publish_render_heartbeat(
        window,
        &RenderHeartbeat {
            screen: pending.screen,
            open_modal: pending.modal,
            render_seq,
        },
    );
    publish_ui_snapshot_now(
        window,
        pending.value,
        pending.json,
        pending.screen,
        pending.modal,
        pending.operation_count,
    );
}

fn schedule_pending_ui_snapshot_flush(window: &web_sys::Window) {
    let raf_window = window.clone();
    let raf_callback = Closure::once_into_js(move || {
        flush_pending_ui_snapshot(&raf_window);
        let has_more_pending = PENDING_UI_PUBLISH.with(|slot| slot.borrow().is_some());
        if has_more_pending {
            schedule_pending_ui_snapshot_flush(&raf_window);
        } else {
            UI_PUBLISH_RAF_PENDING.with(|slot| slot.set(false));
        }
    });
    let raf_function: &Function = raf_callback.unchecked_ref();
    if let Err(error) = window.request_animation_frame(raf_function) {
        UI_PUBLISH_RAF_PENDING.with(|slot| slot.set(false));
        log_js_callback_error("requestAnimationFrame for UI snapshot publish", &error);
        flush_pending_ui_snapshot(window);
    }
}

pub fn set_controller(controller: Arc<UiController>) {
    CONTROLLER.with(|slot| {
        *slot.borrow_mut() = Some(controller);
    });
    LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        *slot.borrow_mut() = None;
    });
    PENDING_UI_PUBLISH.with(|slot| {
        *slot.borrow_mut() = None;
    });
    UI_PUBLISH_RAF_PENDING.with(|slot| slot.set(false));
}

fn current_controller() -> Result<Arc<UiController>, JsValue> {
    CONTROLLER
        .with(|slot| slot.borrow().clone())
        .ok_or_else(|| JsValue::from_str("Runtime bridge not available"))
}

pub fn publish_ui_snapshot(snapshot: &UiSnapshot) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(value) = to_value(snapshot) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };
    PENDING_UI_PUBLISH.with(|slot| {
        *slot.borrow_mut() = Some(PendingUiPublish {
            value,
            json,
            screen: snapshot.screen,
            modal: snapshot.open_modal,
            operation_count: snapshot.operations.len(),
        });
    });
    let should_schedule = UI_PUBLISH_RAF_PENDING.with(|slot| {
        if slot.get() {
            false
        } else {
            slot.set(true);
            true
        }
    });
    if should_schedule {
        schedule_pending_ui_snapshot_flush(&window);
    }
}

fn serialize_ui_snapshot(snapshot: &UiSnapshot) -> JsValue {
    match to_value(snapshot)
        .ok()
        .and_then(|value| JSON::stringify(&value).ok())
        .and_then(|value| value.as_string())
    {
        Some(value) => JsValue::from_str(&value),
        None => {
            web_sys::console::error_1(&JsValue::from_str(&format!(
                "failed to serialize UiSnapshot to JSON string"
            )));
            JsValue::NULL
        }
    }
}

fn serialized_published_ui_snapshot(
    window: &web_sys::Window,
    controller: &UiController,
) -> JsValue {
    let live_snapshot = controller.ui_snapshot();
    let live_json = serialize_ui_snapshot(&live_snapshot);
    let published = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_JSON__"),
    )
    .ok()
    .filter(|value| !value.is_null() && !value.is_undefined());

    let Some(published) = published else {
        return live_json;
    };

    let Some(_published_json) = published.as_string() else {
        return live_json;
    };

    published
}

fn update_semantic_debug(event: &str, detail: Option<&str>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let debug = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_SEMANTIC_DEBUG__"),
    )
    .ok()
    .filter(|value| !value.is_null() && !value.is_undefined())
    .unwrap_or_else(|| {
        let object = Object::new();
        let _ = Reflect::set(
            window.as_ref(),
            &JsValue::from_str("__AURA_SEMANTIC_DEBUG__"),
            object.as_ref(),
        );
        object.into()
    });
    let _ = Reflect::set(
        &debug,
        &JsValue::from_str("last_event"),
        &JsValue::from_str(event),
    );
    let _ = Reflect::set(
        &debug,
        &JsValue::from_str("last_detail"),
        &detail.map(JsValue::from_str).unwrap_or(JsValue::NULL),
    );
}

pub fn install_window_harness_api() -> Result<(), JsValue> {
    let harness = Object::new();
    let observe = Object::new();

    let send_keys = Closure::wrap(Box::new(move |keys: JsValue| -> JsValue {
        if let Some(text) = keys.as_string() {
            if let Ok(controller) = current_controller() {
                controller.send_keys(&text);
            }
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("send_keys"),
        send_keys.as_ref().unchecked_ref(),
    )?;
    send_keys.forget();

    let send_key = Closure::wrap(Box::new(move |key: JsValue, repeat: JsValue| -> JsValue {
        let key_name = key.as_string().unwrap_or_default();
        let repeat = repeat
            .as_f64()
            .map(|value| value.max(1.0) as u16)
            .unwrap_or(1);
        if let Ok(controller) = current_controller() {
            controller.send_key_named(&key_name, repeat);
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("send_key"),
        send_key.as_ref().unchecked_ref(),
    )?;
    send_key.forget();

    let stage_runtime_identity_fn = Closure::wrap(Box::new(
        move |serialized_identity: JsValue| -> js_sys::Promise {
            let serialized_identity = serialized_identity
                .as_string()
                .ok_or_else(|| JsValue::from_str("runtime identity payload must be a string"));
            future_to_promise(async move {
                let serialized_identity = serialized_identity?;
                let _ = serde_json::from_str::<BootstrapRuntimeIdentity>(&serialized_identity)
                    .map_err(|error| {
                        JsValue::from_str(&format!(
                            "invalid staged runtime identity payload: {error}"
                        ))
                    })?;
                stage_runtime_identity(serialized_identity).await?;
                Ok(JsValue::UNDEFINED)
            })
        },
    )
        as Box<dyn FnMut(JsValue) -> js_sys::Promise>);
    Reflect::set(
        &harness,
        &JsValue::from_str("stage_runtime_identity"),
        stage_runtime_identity_fn.as_ref().unchecked_ref(),
    )?;
    stage_runtime_identity_fn.forget();

    let navigate_screen = Closure::wrap(Box::new(move |screen: JsValue| -> JsValue {
        let Some(screen_name) = screen.as_string() else {
            return JsValue::FALSE;
        };
        let target = match screen_name.as_str() {
            "onboarding" => ScreenId::Onboarding,
            "neighborhood" => ScreenId::Neighborhood,
            "chat" => ScreenId::Chat,
            "contacts" => ScreenId::Contacts,
            "notifications" => ScreenId::Notifications,
            "settings" => ScreenId::Settings,
            _ => return JsValue::FALSE,
        };
        if let Ok(controller) = current_controller() {
            controller.set_screen(target);
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("navigate_screen"),
        navigate_screen.as_ref().unchecked_ref(),
    )?;
    navigate_screen.forget();

    let snapshot = Closure::wrap(Box::new(move || -> JsValue {
        let Ok(controller) = current_controller() else {
            return JsValue::NULL;
        };
        let rendered = controller.snapshot();
        let payload = Object::new();
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("screen"),
            &JsValue::from_str(&rendered.screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("raw_screen"),
            &JsValue::from_str(&rendered.raw_screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("authoritative_screen"),
            &JsValue::from_str(&rendered.authoritative_screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("normalized_screen"),
            &JsValue::from_str(&rendered.normalized_screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("capture_consistency"),
            &JsValue::from_str("settled"),
        );
        payload.into()
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("snapshot"),
        snapshot.as_ref().unchecked_ref(),
    )?;
    snapshot.forget();

    let ui_state = Closure::wrap(Box::new(move || -> JsValue {
        let Ok(controller) = current_controller() else {
            return JsValue::NULL;
        };
        let Some(window) = web_sys::window() else {
            return serialize_ui_snapshot(&controller.ui_snapshot());
        };
        serialized_published_ui_snapshot(&window, &controller)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("ui_state"),
        ui_state.as_ref().unchecked_ref(),
    )?;
    ui_state.forget();

    let read_clipboard = Closure::wrap(Box::new(move || -> JsValue {
        if let Some(window) = web_sys::window() {
            if let Ok(value) = Reflect::get(
                window.as_ref(),
                &JsValue::from_str("__AURA_HARNESS_CLIPBOARD__"),
            ) {
                if let Some(text) = value.as_string() {
                    if !text.is_empty() {
                        return JsValue::from_str(&text);
                    }
                }
            }
        }
        match current_controller() {
            Ok(controller) => JsValue::from_str(&controller.read_clipboard()),
            Err(_) => JsValue::from_str(""),
        }
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("read_clipboard"),
        read_clipboard.as_ref().unchecked_ref(),
    )?;
    read_clipboard.forget();

    let submit_semantic_command_raw =
        Closure::wrap(Box::new(move |request_json: String| -> js_sys::Promise {
            future_to_promise(async move {
                update_semantic_debug("raw_entry", None);
                web_sys::console::log_1(&"[web-harness] submit_semantic_command entry".into());
                let outcome: Result<JsValue, JsValue> = async {
                    let controller = current_controller()?;
                    let request =
                        from_str::<SemanticCommandRequest>(&request_json).map_err(|error| {
                            JsValue::from_str(&format!("invalid semantic command request: {error}"))
                        })?;
                    update_semantic_debug("raw_parsed", Some(&format!("{:?}", request.intent)));
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness] submit_semantic_command intent={:?}",
                            request.intent
                        )
                        .into(),
                    );
                    let response = submit_semantic_command(controller, request).await?;
                    to_string(&response)
                        .map(|response_json| JsValue::from_str(&response_json))
                        .map_err(|error| {
                            JsValue::from_str(&format!(
                                "failed to serialize semantic command response: {error}"
                            ))
                        })
                }
                .await;

                match outcome {
                    Ok(value) => {
                        update_semantic_debug("raw_resolved", None);
                        Ok(value)
                    }
                    Err(error) => {
                        update_semantic_debug("raw_rejected", error.as_string().as_deref());
                        Err(error)
                    }
                }
            })
        }) as Box<dyn FnMut(String) -> js_sys::Promise>);
    Reflect::set(
        &harness,
        &JsValue::from_str("__submit_semantic_command_raw"),
        submit_semantic_command_raw.as_ref().unchecked_ref(),
    )?;
    let submit_semantic_command_fn = Function::new_with_args(
        "request",
        r#"
window.__AURA_SEMANTIC_DEBUG__ = window.__AURA_SEMANTIC_DEBUG__ || {};
window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_entry";
console.log("[web-harness-js] submit_semantic_command wrapper entry");
const raw = window.__AURA_HARNESS__?.__submit_semantic_command_raw;
if (typeof raw !== "function") {
  window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_missing_raw";
  return Promise.reject(
    new Error("window.__AURA_HARNESS__.__submit_semantic_command_raw is unavailable"),
  );
}
try {
  const result = raw(JSON.stringify(request));
  window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_raw_return";
  console.log("[web-harness-js] submit_semantic_command wrapper raw returned");
  return Promise.resolve(result);
} catch (error) {
  window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_threw";
  window.__AURA_SEMANTIC_DEBUG__.last_detail = error?.message ?? String(error);
  console.error("[web-harness-js] submit_semantic_command wrapper threw", error);
  return Promise.reject(error);
}
"#,
    );
    Reflect::set(
        &harness,
        &JsValue::from_str("submit_semantic_command"),
        submit_semantic_command_fn.as_ref(),
    )?;
    submit_semantic_command_raw.forget();

    let get_authority_id = Closure::wrap(Box::new(move || -> JsValue {
        match current_controller() {
            Ok(controller) => JsValue::from_str(&controller.authority_id()),
            Err(error) => error,
        }
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("get_authority_id"),
        get_authority_id.as_ref().unchecked_ref(),
    )?;
    get_authority_id.forget();

    let tail_log = Closure::wrap(Box::new(move |lines: JsValue| -> JsValue {
        let lines = lines
            .as_f64()
            .map(|value| value.max(1.0) as usize)
            .unwrap_or(20);
        let array = Array::new();
        if let Ok(controller) = current_controller() {
            for line in controller.tail_log(lines) {
                array.push(&JsValue::from_str(&line));
            }
        }
        array.into()
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("tail_log"),
        tail_log.as_ref().unchecked_ref(),
    )?;
    tail_log.forget();

    let root_structure = Closure::wrap(Box::new(move || -> JsValue {
        let Ok(controller) = current_controller() else {
            return JsValue::NULL;
        };
        let snapshot = controller.ui_snapshot();
        let Some(window) = web_sys::window() else {
            return JsValue::NULL;
        };
        let Some(document) = window.document() else {
            return JsValue::NULL;
        };

        let payload = Object::new();
        let app_root_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::AppRoot
                .web_dom_id()
                .expect("ControlId::AppRoot must define a web DOM id")
        );
        let modal_region_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::ModalRegion
                .web_dom_id()
                .expect("ControlId::ModalRegion must define a web DOM id")
        );
        let onboarding_root_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::OnboardingRoot
                .web_dom_id()
                .expect("ControlId::OnboardingRoot must define a web DOM id")
        );
        let toast_region_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::ToastRegion
                .web_dom_id()
                .expect("ControlId::ToastRegion must define a web DOM id")
        );
        let screen_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::Screen(snapshot.screen)
                .web_dom_id()
                .expect("ControlId::Screen(snapshot.screen) must define a web DOM id")
        );

        let app_root_count = document
            .query_selector(&app_root_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let modal_region_count = document
            .query_selector(&modal_region_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let onboarding_root_count = document
            .query_selector(&onboarding_root_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let toast_region_count = document
            .query_selector(&toast_region_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let active_screen_root_count = document
            .query_selector(&screen_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);

        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("screen"),
            &JsValue::from_str(&format!("{:?}", snapshot.screen).to_ascii_lowercase()),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("app_root_count"),
            &JsValue::from_f64(f64::from(app_root_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("modal_region_count"),
            &JsValue::from_f64(f64::from(modal_region_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("onboarding_root_count"),
            &JsValue::from_f64(f64::from(onboarding_root_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("toast_region_count"),
            &JsValue::from_f64(f64::from(toast_region_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("active_screen_root_count"),
            &JsValue::from_f64(f64::from(active_screen_root_count)),
        );
        payload.into()
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("root_structure"),
        root_structure.as_ref().unchecked_ref(),
    )?;
    root_structure.forget();

    let inject_message = Closure::wrap(Box::new(move |message: JsValue| -> JsValue {
        if let Some(text) = message.as_string() {
            if let Ok(controller) = current_controller() {
                controller.inject_message(&text);
            }
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("inject_message"),
        inject_message.as_ref().unchecked_ref(),
    )?;
    inject_message.forget();

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is not available"))?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_HARNESS__"),
        &harness,
    )?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_HARNESS_OBSERVE__"),
        &observe,
    )?;
    let read_only_ui_state = Closure::wrap(Box::new(move || -> JsValue {
        let Ok(controller) = current_controller() else {
            return JsValue::NULL;
        };
        let Some(window) = web_sys::window() else {
            return serialize_ui_snapshot(&controller.ui_snapshot());
        };
        serialized_published_ui_snapshot(&window, &controller)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE__"),
        read_only_ui_state.as_ref().unchecked_ref(),
    )?;
    read_only_ui_state.forget();

    let render_heartbeat = Closure::wrap(Box::new(move || -> JsValue {
        let window = match web_sys::window() {
            Some(window) => window,
            None => return JsValue::NULL,
        };
        Reflect::get(
            window.as_ref(),
            &JsValue::from_str("__AURA_RENDER_HEARTBEAT__"),
        )
        .unwrap_or(JsValue::NULL)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("render_heartbeat"),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT_STATE__"),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    render_heartbeat.forget();

    Ok(())
}
