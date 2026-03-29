use async_lock::Mutex;
use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;
use aura_app::ui::contract::{ControlId, FieldId, ScreenId, UiReadiness};
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_ui::{AuraUiRoot, RequiredDomId};
use dioxus::dioxus_core::schedule_update;
use dioxus::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::error::{log_web_error, WebUiError};
use crate::harness_bridge;
use crate::shell_host::{BootstrapState, WebShellHost};
use crate::task_owner::shared_web_task_owner;
use crate::workflows::{
    self, AccountCreationStageMode, CurrentRuntimeIdentity, DeviceEnrollmentImportRequest,
    RebootstrapPolicy,
};

use super::bootstrap::submit_runtime_bootstrap_handoff;
use super::storage::{
    active_storage_prefix, load_selected_runtime_identity, logged_optional,
    selected_runtime_identity_key,
};

#[component]
pub(crate) fn App() -> Element {
    let bootstrap_started = use_hook(|| Rc::new(Cell::new(false)));
    let bootstrap_epoch = use_signal(|| 0_u64);
    let committed_bootstrap = use_signal(|| Option::<BootstrapState>::None);
    let bootstrap_error = use_signal(|| Option::<WebUiError>::None);
    let rebootstrap_lock = use_hook(|| Arc::new(Mutex::new(())));
    let shell_host = use_hook(move || {
        WebShellHost::new(
            bootstrap_epoch,
            committed_bootstrap,
            bootstrap_error,
            rebootstrap_lock.clone(),
        )
    });

    use_effect(|| {
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            document.set_title("Aura");
        }
    });

    use_effect(move || {
        let submitter = shell_host.bootstrap_submitter();
        harness_bridge::set_bootstrap_handoff_submitter(submitter.clone());
        harness_bridge::set_runtime_identity_stager(
            shell_host.runtime_identity_stager(submitter.clone()),
        );

        if !bootstrap_started.get() {
            bootstrap_started.set(true);
            let _ = submitter(harness_bridge::BootstrapHandoff::InitialBootstrap);
        }
    });

    if let Some(state) = committed_bootstrap() {
        return rsx! {
            BootstrappedApp {
                key: "{state.generation_id}",
                state,
            }
        };
    }

    if let Some(error) = bootstrap_error() {
        return rsx! {
            main {
                class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                div {
                    class: "max-w-xl space-y-3 text-center",
                    h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                    p { class: "text-sm text-muted-foreground", "Web runtime bootstrap failed." }
                    p { class: "text-xs text-muted-foreground break-words", "{error.user_message()}" }
                }
            }
        };
    }

    rsx! {
        main {
            class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
            div {
                class: "max-w-xl space-y-3 text-center",
                h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                p { class: "text-sm text-muted-foreground", "Initializing web runtime..." }
            }
        }
    }
}

#[component]
fn BootstrappedApp(state: BootstrapState) -> Element {
    let controller = state.controller.clone();
    let rerender = schedule_update();
    controller.set_rerender_callback(rerender.clone());
    let mut account_name = use_signal(String::new);
    let mut account_error = use_signal(|| Option::<WebUiError>::None);
    let creating_account = use_signal(|| false);
    let mut import_code = use_signal(String::new);
    let mut import_error = use_signal(|| Option::<WebUiError>::None);
    let importing_code = use_signal(|| false);
    let mut auto_import_started = use_signal(|| false);
    let controller_snapshot = controller.semantic_model_snapshot();
    let controller_account_ready = controller_snapshot.readiness == UiReadiness::Ready
        && controller_snapshot.screen != ScreenId::Onboarding;
    let account_ready = state.account_ready || controller_account_ready;

    if account_ready {
        return rsx! {
            AuraUiRoot {
                controller: controller.clone(),
            }
        };
    }

    let run_import: Arc<dyn Fn(String)> = Arc::new({
        let controller = controller.clone();
        let import_error = import_error.clone();
        let importing_code = importing_code.clone();
        move |code: String| {
            let mut import_error = import_error.clone();
            let mut importing_code = importing_code.clone();
            if importing_code() {
                return;
            }

            let storage_prefix = active_storage_prefix();
            importing_code.set(true);
            import_error.set(None);

            let controller = controller.clone();
            let scheduled_controller = controller.clone();
            if let Err(error) = harness_bridge::schedule_browser_task_next_tick(move || {
                shared_web_task_owner().spawn_local(async move {
                    let app_core = scheduled_controller.app_core().clone();
                    let result: Result<_, WebUiError> = async {
                        let current_authority = runtime_workflows::require_runtime(&app_core)
                            .await
                            .map_err(|error| {
                                WebUiError::operation(
                                    WebUiOperation::ImportDeviceEnrollmentCode,
                                    "WEB_RUNTIME_REQUIRED_FAILED",
                                    error.to_string(),
                                )
                            })?
                            .authority_id();
                        let current_runtime_identity = CurrentRuntimeIdentity {
                            authority_id: current_authority,
                            selected_runtime_identity: logged_optional(
                                load_selected_runtime_identity(&selected_runtime_identity_key(
                                    &storage_prefix,
                                )),
                            ),
                        };
                        let result = workflows::accept_device_enrollment_import(
                            &app_core,
                            DeviceEnrollmentImportRequest {
                                code: &code,
                                current_runtime_identity: current_runtime_identity.clone(),
                                storage_prefix: &storage_prefix,
                                rebootstrap_policy: RebootstrapPolicy::StageIfRequired,
                                operation: WebUiOperation::ImportDeviceEnrollmentCode,
                            },
                        )
                        .await?;
                        if result.rebootstrap_required {
                            let staged_runtime_identity = result.staged_runtime_identity.clone();
                            let selected_runtime_identity =
                                current_runtime_identity.selected_runtime_identity;
                            let device_id = staged_runtime_identity.device_id;
                            let subject_authority = staged_runtime_identity.authority_id;
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] staging_rebootstrap current_authority={};subject_authority={};selected_runtime_identity={:?};invited_device={}",
                                    current_authority,
                                    subject_authority,
                                    selected_runtime_identity,
                                    device_id
                                )
                                .into(),
                            );
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] staged_rebootstrap subject_authority={};device_id={}",
                                    subject_authority, device_id
                                )
                                .into(),
                            );
                            submit_runtime_bootstrap_handoff(
                                harness_bridge::BootstrapHandoff::RuntimeIdentityStaged {
                                    authority_id: subject_authority,
                                    device_id,
                                    source: harness_bridge::RuntimeIdentityStageSource::ImportDeviceEnrollment,
                                },
                            )
                            .await
                            .map_err(|error| {
                                error.with_operation(WebUiOperation::ImportDeviceEnrollmentCode)
                            })?;
                            return Ok(result);
                        }
                        web_sys::console::log_1(
                            &format!(
                                "[web-import-device] accepting_on_bound_runtime authority={};selected_runtime_identity={:?};invited_device={}",
                                current_authority,
                                current_runtime_identity.selected_runtime_identity,
                                result.staged_runtime_identity.device_id
                            )
                            .into(),
                        );
                        web_sys::console::log_1(
                            &format!(
                                "[web-import-device] initializing_runtime_account nickname={}",
                                result.bootstrap_name
                            )
                            .into(),
                        );
                        Ok(result)
                    }
                    .await;

                    // Guard signal writes — component may have unmounted during async work.
                    match result {
                        Ok(result) => {
                            if result.accepted {
                                web_sys::console::log_1(&"[web-import-device] finalizing_ui".into());
                                scheduled_controller.info_toast("Device enrollment complete");
                                scheduled_controller.finalize_account_setup(ScreenId::Neighborhood);
                                harness_bridge::publish_semantic_controller_snapshot(
                                    scheduled_controller.clone(),
                                );
                                web_sys::console::log_1(&"[web-import-device] finalized_ui".into());
                            } else {
                                scheduled_controller
                                    .info_toast("Switching runtime to finish import");
                            }
                            let _ = importing_code.try_write().map(|mut v| *v = false);
                        }
                        Err(error) => {
                            let message = error.user_message();
                            scheduled_controller.set_account_setup_state(
                                false,
                                "",
                                Some(message.clone()),
                            );
                            let _ = import_error.try_write().map(|mut v| *v = Some(error));
                            let _ = importing_code.try_write().map(|mut v| *v = false);
                        }
                    }
                });
            }) {
                let error = WebUiError::operation(
                    WebUiOperation::ImportDeviceEnrollmentCode,
                    "WEB_DEVICE_ENROLLMENT_SCHEDULE_FAILED",
                    format!("{error:?}"),
                );
                controller.set_account_setup_state(false, "", Some(error.user_message()));
                let _ = import_error.try_write().map(|mut v| *v = Some(error));
                let _ = importing_code.try_write().map(|mut v| *v = false);
            }
        }
    });

    let submit_account = {
        let controller = controller.clone();
        let account_name = account_name.clone();
        let mut account_error = account_error.clone();
        let mut creating_account = creating_account.clone();
        move |_| {
            if creating_account() {
                return;
            }

            let nickname = account_name();
            web_sys::console::log_1(
                &format!(
                    "[web-onboarding] submit_account start nickname={}",
                    nickname
                )
                .into(),
            );
            creating_account.set(true);
            account_error.set(None);
            controller.set_account_setup_state(false, nickname.clone(), None);

            let controller = controller.clone();
            shared_web_task_owner().spawn_local(async move {
                let result: Result<_, WebUiError> = async {
                    let result =
                        workflows::stage_account_creation(controller.app_core(), &nickname).await?;
                    if result.mode == AccountCreationStageMode::InitialBootstrapStaged {
                        submit_runtime_bootstrap_handoff(
                            harness_bridge::BootstrapHandoff::PendingAccountBootstrap {
                                account_name: nickname.clone(),
                                source: harness_bridge::PendingAccountBootstrapSource::OnboardingUi,
                            },
                        )
                        .await?;
                    }
                    Ok(result)
                }
                .await;

                // Guard signal writes — the component may have unmounted during
                // the async bootstrap handoff (e.g., on timeout + re-bootstrap).
                match result {
                    Ok(result) => {
                        web_sys::console::log_1(&"[web-onboarding] submit_account ok".into());
                        if result.mode == AccountCreationStageMode::RuntimeInitialized {
                            controller.finalize_account_setup(ScreenId::Neighborhood);
                        } else {
                            controller.info_toast("Finishing account bootstrap");
                        }
                        let _ = creating_account.try_write().map(|mut v| *v = false);
                    }
                    Err(error) => {
                        log_web_error("error", &error);
                        let message = error.user_message();
                        controller.set_account_setup_state(
                            false,
                            nickname.clone(),
                            Some(message.clone()),
                        );
                        let _ = account_error.try_write().map(|mut v| *v = Some(error));
                        let _ = creating_account.try_write().map(|mut v| *v = false);
                    }
                }
            });
        }
    };

    let submit_import = {
        let import_code = import_code.clone();
        let run_import = run_import.clone();
        move |_| {
            let code = import_code();
            run_import(code);
        }
    };

    if !auto_import_started() {
        if let Some(pending_code) = state.pending_device_enrollment_code.clone() {
            if !pending_code.is_empty() {
                auto_import_started.set(true);
                import_code.set(pending_code.clone());
                let run_import = run_import.clone();
                if let Err(error) = harness_bridge::schedule_browser_task_next_tick(move || {
                    run_import(pending_code);
                }) {
                    log_web_error(
                        "error",
                        &WebUiError::operation(
                            WebUiOperation::ImportDeviceEnrollmentCode,
                            "WEB_DEVICE_ENROLLMENT_AUTOSTART_SCHEDULE_FAILED",
                            format!("{error:?}"),
                        ),
                    );
                }
            }
        }
    }

    rsx! {
        main {
            class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
            div {
                id: ControlId::OnboardingRoot
                    .required_dom_id("ControlId::OnboardingRoot"),
                class: "w-full max-w-xl",
                div {
                    id: "aura-onboarding-card",
                    class: "w-full max-w-xl overflow-hidden rounded-sm border border-border bg-card p-0 text-card-foreground shadow-2xl",
                    // Header
                    div {
                        class: "bg-card px-4 py-3 border-b border-border",
                        h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                    }
                    // Body
                    div {
                        class: "px-4 py-4 space-y-4",
                        // Create a new account
                        h2 { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Create a new account" }
                        label {
                            class: "block space-y-2",
                            span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Nickname" }
                            input {
                                id: FieldId::AccountName
                                    .required_dom_id("FieldId::AccountName"),
                                class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                value: "{account_name()}",
                                disabled: creating_account(),
                                oninput: move |event| {
                                    let value = event.value();
                                    account_name.set(value.clone());
                                    account_error.set(None);
                                },
                            }
                        }
                        if let Some(error) = account_error() {
                            p { class: "text-sm text-destructive", "{error.user_message()}" }
                        }
                        button {
                            id: ControlId::OnboardingCreateAccountButton
                                .required_dom_id("ControlId::OnboardingCreateAccountButton"),
                            class: "inline-flex h-10 items-center justify-center rounded-md bg-foreground px-6 text-sm font-medium text-background transition-colors disabled:pointer-events-none disabled:opacity-50",
                            disabled: creating_account() || account_name().trim().is_empty(),
                            onclick: submit_account,
                            if creating_account() {
                                "Creating Account..."
                            } else {
                                "Create Account"
                            }
                        }
                        // Divider
                        div { class: "flex items-center gap-3 py-1",
                            div { class: "h-px flex-1 bg-border" }
                            span { class: "text-[11px] font-medium uppercase tracking-[0.08em] text-muted-foreground", "or" }
                            div { class: "h-px flex-1 bg-border" }
                        }
                        // Join an existing account
                        h2 { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Join an existing account" }
                        label {
                            class: "block space-y-2",
                            span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Device Enrollment Code" }
                            input {
                                id: FieldId::DeviceImportCode
                                    .required_dom_id("FieldId::DeviceImportCode"),
                                class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                value: "{import_code()}",
                                disabled: importing_code(),
                                oninput: move |event| {
                                    import_code.set(event.value());
                                    import_error.set(None);
                                },
                            }
                        }
                        if let Some(error) = import_error() {
                            p { class: "text-sm text-destructive", "{error.user_message()}" }
                        }
                        button {
                            id: ControlId::OnboardingImportDeviceButton
                                .required_dom_id("ControlId::OnboardingImportDeviceButton"),
                            class: "inline-flex h-10 items-center justify-center rounded-md bg-foreground px-6 text-sm font-medium text-background transition-colors disabled:pointer-events-none disabled:opacity-50",
                            disabled: importing_code() || import_code().trim().is_empty(),
                            onclick: submit_import,
                            if importing_code() {
                                "Joining Account..."
                            } else {
                                "Join Account"
                            }
                        }
                    }
                }
            }
        }
    }
}
