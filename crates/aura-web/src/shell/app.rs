use async_lock::Mutex;
use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;
use aura_app::ui::contract::RuntimeFact;
use aura_app::ui::contract::{ControlId, FieldId, ScreenId, UiReadiness};
use aura_app::DUAL_FRONTEND_DEMO_WEB_TABLET_NAME;
use aura_app::{BootstrapCandidateInfo, BootstrapCandidateOrigin};
use aura_ui::{AuraUiRoot, RequiredDomId};
use dioxus::dioxus_core::schedule_update;
use dioxus::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use crate::error::{log_web_error, WebUiError};
use crate::harness_bridge;
use crate::shell_host::{BootstrapState, WebShellHost};
use crate::task_owner::shared_web_task_owner;
use crate::workflows::{self, AccountCreationStageMode};

use super::bootstrap::submit_runtime_bootstrap_handoff;
use super::storage::{
    active_storage_prefix, bootstrap_broker_auth_token, bootstrap_broker_url,
    demo_tablet_enrollment_code_key, dual_demo_web_enabled, persist_demo_tablet_enrollment_code,
};
use crate::browser_promises::browser_sleep_ms;

fn write_signal_with_retry<T>(
    mut signal: Signal<T>,
    value: T,
    operation: WebUiOperation,
    error_code: &'static str,
    context: &'static str,
) where
    T: Clone + 'static,
{
    if let Ok(mut slot) = signal.try_write() {
        *slot = value;
        return;
    }

    let retry_value = value.clone();
    if let Err(schedule_error) = harness_bridge::schedule_browser_task_next_tick(move || {
        if let Ok(mut slot) = signal.try_write() {
            *slot = retry_value;
        } else {
            log_web_error(
                "warn",
                &WebUiError::operation(
                    operation,
                    error_code,
                    format!("browser signal update dropped after retry: {context}"),
                ),
            );
        }
    }) {
        log_web_error(
            "warn",
            &WebUiError::operation(
                operation,
                error_code,
                format!(
                    "browser signal update retry schedule failed for {context}: {schedule_error:?}"
                ),
            ),
        );
    }
}

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

/// Broker records as onboarding candidates, labelled by where the broker is.
fn broker_candidates(
    base_url: &str,
    records: Vec<aura_agent::BootstrapBrokerCandidateRecord>,
) -> Vec<BootstrapCandidateInfo> {
    let origin = if aura_agent::bootstrap_broker_endpoint_is_loopback(base_url) {
        BootstrapCandidateOrigin::LocalBroker
    } else {
        BootstrapCandidateOrigin::LanBroker
    };
    records
        .into_iter()
        .filter_map(|record| {
            Some(BootstrapCandidateInfo {
                authority_id: record.authority_id()?,
                device_id: Some(record.device_id),
                origin,
                address: record.address,
                discovered_at_ms: record.discovered_at_ms,
                nickname_suggestion: record.nickname_suggestion,
            })
        })
        .collect()
}

fn onboarding_finished(controller: &aura_ui::UiController) -> bool {
    let snapshot = controller.semantic_model_snapshot();
    snapshot.readiness == UiReadiness::Ready && snapshot.screen != ScreenId::Onboarding
}

#[component]
fn BootstrappedApp(state: BootstrapState) -> Element {
    let controller = state.controller.clone();
    let bootstrap_delivery_notice = state
        .bootstrap_delivery_error()
        .map(WebUiError::user_message);
    let rerender = schedule_update();
    let mut account_name = use_signal(String::new);
    let mut account_error = use_signal(|| Option::<WebUiError>::None);
    let creating_account = use_signal(|| false);
    let mut import_code = use_signal(|| state.claim_enrollment_prefill().unwrap_or_default());
    let mut import_manifest = use_signal(String::new);
    let mut import_initiator_verifier = use_signal(String::new);
    let mut import_error = use_signal(|| Option::<WebUiError>::None);
    let importing_code = use_signal(|| false);
    let bootstrap_candidates = use_signal(Vec::<BootstrapCandidateInfo>::new);
    let controller_snapshot = controller.semantic_model_snapshot();
    let controller_account_ready = onboarding_finished(&controller);
    let account_ready = state.account_ready || controller_account_ready;
    let dual_demo_enabled = dual_demo_web_enabled();
    let demo_tablet_storage_key = demo_tablet_enrollment_code_key(&active_storage_prefix());
    let latest_demo_tablet_code = if dual_demo_enabled {
        controller_snapshot
            .runtime_events
            .iter()
            .rev()
            .find_map(|event| match &event.fact {
                RuntimeFact::DeviceEnrollmentCodeReady {
                    device_name,
                    code: Some(code),
                    ..
                } if device_name.as_deref() == Some(DUAL_FRONTEND_DEMO_WEB_TABLET_NAME) => {
                    Some(code.clone())
                }
                _ => None,
            })
    } else {
        None
    };

    use_effect({
        let latest_demo_tablet_code = latest_demo_tablet_code.clone();
        let demo_tablet_storage_key = demo_tablet_storage_key.clone();
        move || {
            if let Some(code) = latest_demo_tablet_code.clone() {
                let _ = persist_demo_tablet_enrollment_code(&demo_tablet_storage_key, &code);
            }
        }
    });

    // Account creation replaces the bootstrap controller without necessarily
    // remounting this component. Bind the host ACK policy to the actual current
    // controller before either onboarding or ready import controls are exposed.
    let import_policy_controller = use_hook(|| Rc::new(RefCell::new(Weak::new())));
    let needs_import_policy = import_policy_controller
        .borrow()
        .upgrade()
        .is_none_or(|bound: Arc<aura_ui::UiController>| !Arc::ptr_eq(&bound, &controller));
    if needs_import_policy {
        controller.set_enrollment_import_host_policy(workflows::enrollment_import_host_policy(
            controller.app_core().clone(),
            active_storage_prefix(),
        ));
        *import_policy_controller.borrow_mut() = Arc::downgrade(&controller);
    }

    if account_ready {
        return rsx! {
            super::rendered_controls::ObservedRendererFrame {
                controller: controller.clone(),
                AuraUiRoot { controller: controller.clone() }
            }
        };
    }
    // The onboarding surface owns controller rerenders only until the account
    // is ready. Once `AuraUiRoot` mounts, its shell installs its own callback.
    // Re-installing ours on a later `BootstrappedApp` render would route every
    // controller-driven rerender (harness keys, page-owned navigation) here,
    // where the memoized `AuraUiRoot` does not re-render, so the DOM would stay
    // on the previous screen while the semantic snapshot moved on.
    controller.set_rerender_callback(rerender.clone());

    use_effect({
        let controller = controller.clone();
        let app_core = controller.app_core().clone();
        move || {
            let mut bootstrap_candidates = bootstrap_candidates;
            let controller = controller.clone();
            let app_core = app_core.clone();
            shared_web_task_owner().spawn_local(async move {
                loop {
                    // Candidates are only listed on the onboarding surface;
                    // stop once the account is ready instead of polling (and
                    // re-rendering this component) for the page's lifetime.
                    if onboarding_finished(&controller) {
                        break;
                    }
                    let has_runtime = app_core.read().await.has_runtime();
                    let candidate_result = if has_runtime {
                        let app = app_core.read().await;
                        app.get_bootstrap_candidates()
                            .await
                            .map_err(aura_app::runtime_bridge::RuntimeBridgeError::from)
                    } else {
                        // Before an account exists there is no runtime, so LAN
                        // discovery is unavailable; the configured bootstrap
                        // broker is the agent-free source. Without one, stop
                        // instead of polling a call that can only fail.
                        let (Some(base_url), Some(auth_token)) =
                            (bootstrap_broker_url(), bootstrap_broker_auth_token())
                        else {
                            break;
                        };
                        aura_agent::fetch_bootstrap_broker_candidates(&base_url, &auth_token)
                            .await
                            .map(|records| broker_candidates(&base_url, records))
                            .map_err(|error| {
                                aura_app::runtime_bridge::RuntimeBridgeError::with_source(
                                    aura_app::IntentError::network_error(format!(
                                        "Bootstrap broker candidate discovery failed: {error}"
                                    )),
                                    error,
                                )
                            })
                    };

                    match candidate_result {
                        Ok(candidates) => {
                            bootstrap_candidates.set(candidates);
                        }
                        Err(error) => {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::BootstrapController,
                                    "WEB_BOOTSTRAP_CANDIDATE_RUNTIME_UNAVAILABLE",
                                    error.to_string(),
                                ),
                            );
                            break;
                        }
                    }

                    if let Err(error) = browser_sleep_ms(
                        2_000,
                        WebUiOperation::BootstrapController,
                        "WEB_BOOTSTRAP_CANDIDATE_SLEEP_UNAVAILABLE",
                        "WEB_BOOTSTRAP_CANDIDATE_SLEEP_SCHEDULE_FAILED",
                        "WEB_BOOTSTRAP_CANDIDATE_SLEEP_DROPPED",
                        "window unavailable for bootstrap candidate polling",
                        "bootstrap candidate polling sleep",
                    )
                    .await
                    {
                        log_web_error("warn", &error);
                        break;
                    }
                }
            });
        }
    });

    let run_import: Arc<dyn Fn(String)> = Arc::new({
        let controller = controller.clone();
        let import_error = import_error.clone();
        let importing_code = importing_code.clone();
        let import_manifest = import_manifest.clone();
        let import_initiator_verifier = import_initiator_verifier.clone();
        move |code: String| {
            if importing_code()
                || code.trim().is_empty()
                || import_manifest().trim().is_empty()
                || import_initiator_verifier().trim().is_empty()
            {
                return;
            }
            let mut importing_code = importing_code.clone();
            let mut import_error = import_error.clone();
            importing_code.set(true);
            import_error.set(None);
            let manifest_transfer = Some(aura_app::ui::contract::EnrollmentManifestTransferInput {
                manifest_code: import_manifest(),
                initiator_verifier_code: import_initiator_verifier(),
            });
            let (_, transfer, observer) =
                match aura_ui::semantic_lifecycle::begin_exact_enrollment_import(
                    controller.clone(),
                    code,
                    manifest_transfer,
                    controller.enrollment_import_host_policy(),
                ) {
                    Ok(admitted) => admitted,
                    Err(detail) => {
                        write_signal_with_retry(
                            importing_code,
                            false,
                            WebUiOperation::ImportDeviceEnrollmentCode,
                            "WEB_DEVICE_ENROLLMENT_SIGNAL_WRITE_FAILED",
                            "clear importing_code after device enrollment admission refusal",
                        );
                        write_signal_with_retry(
                            import_error,
                            Some(WebUiError::operation(
                                WebUiOperation::ImportDeviceEnrollmentCode,
                                "WEB_DEVICE_ENROLLMENT_IMPORT_FAILED",
                                detail,
                            )),
                            WebUiOperation::ImportDeviceEnrollmentCode,
                            "WEB_DEVICE_ENROLLMENT_SIGNAL_WRITE_FAILED",
                            "publish device enrollment admission refusal",
                        );
                        return;
                    }
                };
            let controller = controller.clone();
            shared_web_task_owner().spawn_local(async move {
                let result = transfer
                    .run_workflow(controller.clone(), "import_device_enrollment", observer)
                    .await;
                match result {
                    Ok(_completed) => controller.finalize_account_setup(ScreenId::Neighborhood),
                    Err(error) => {
                        let error = WebUiError::operation(
                            WebUiOperation::ImportDeviceEnrollmentCode,
                            "WEB_DEVICE_ENROLLMENT_IMPORT_FAILED",
                            error.to_string(),
                        )
                        .with_source(error);
                        controller.set_account_setup_state(false, "", Some(error.user_message()));
                        write_signal_with_retry(
                            import_error,
                            Some(error),
                            WebUiOperation::ImportDeviceEnrollmentCode,
                            "WEB_DEVICE_ENROLLMENT_SIGNAL_WRITE_FAILED",
                            "publish device enrollment import error",
                        );
                    }
                }
                write_signal_with_retry(
                    importing_code,
                    false,
                    WebUiOperation::ImportDeviceEnrollmentCode,
                    "WEB_DEVICE_ENROLLMENT_SIGNAL_WRITE_FAILED",
                    "clear importing_code after device enrollment import",
                );
            });
        }
    });

    // Extracted as an Arc so both the onclick handler and the onkeydown
    // (Enter-in-field) handler below can trigger the same submit action.
    let run_account_submit: Arc<dyn Fn()> = Arc::new({
        let controller = controller.clone();
        let account_name = account_name.clone();
        let account_error = account_error.clone();
        let creating_account = creating_account.clone();
        move || {
            let mut account_error = account_error.clone();
            let mut creating_account = creating_account.clone();
            if creating_account() {
                return;
            }

            let nickname = account_name();
            // Guard against Enter-on-empty-field; the button is disabled
            // for empty input, so the click path already has this guard.
            if nickname.trim().is_empty() {
                return;
            }
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
            let account_error = account_error.clone();
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
                        write_signal_with_retry(
                            creating_account,
                            false,
                            WebUiOperation::CreateAccount,
                            "WEB_ACCOUNT_CREATION_SIGNAL_WRITE_FAILED",
                            "clear creating_account after account creation",
                        );
                    }
                    Err(error) => {
                        log_web_error("error", &error);
                        let message = error.user_message();
                        controller.set_account_setup_state(
                            false,
                            nickname.clone(),
                            Some(message.clone()),
                        );
                        write_signal_with_retry(
                            account_error,
                            Some(error),
                            WebUiOperation::CreateAccount,
                            "WEB_ACCOUNT_CREATION_SIGNAL_WRITE_FAILED",
                            "publish account creation error",
                        );
                        write_signal_with_retry(
                            creating_account,
                            false,
                            WebUiOperation::CreateAccount,
                            "WEB_ACCOUNT_CREATION_SIGNAL_WRITE_FAILED",
                            "clear creating_account after account creation error",
                        );
                    }
                }
            });
        }
    });

    let submit_account = {
        let run = run_account_submit.clone();
        move |_| run()
    };

    let submit_import = {
        let import_code = import_code.clone();
        let run_import = run_import.clone();
        move |_| {
            let code = import_code();
            run_import(code);
        }
    };

    rsx! {
        super::rendered_controls::ObservedRendererFrame {
            controller: controller.clone(),
        main {
            class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
            super::rendered_controls::RenderedOnboardingRoot {
                div {
                    id: "aura-onboarding-card",
                    class: "w-full max-w-xl overflow-hidden rounded-sm border border-border bg-card p-0 text-card-foreground shadow-2xl",
                    // Header
                    div {
                        class: "bg-card px-4 py-3 border-b border-border text-left",
                        h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                    }
                    // Body
                    div {
                        class: "px-4 py-6 space-y-4",
                        // Create a new account
                        label {
                            class: "block space-y-2",
                            span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Create a new account" }
                            aura_ui::rendered_control::RenderedInput {
                                control_id: ControlId::Field(FieldId::AccountName),
                                class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                placeholder: "Enter your nickname...",
                                value: "{account_name()}",
                                autofocus: true,
                                disabled: creating_account(),
                                oninput: move |event: FormEvent| {
                                    let value = event.value();
                                    account_name.set(value.clone());
                                    account_error.set(None);
                                },
                                onkeydown: {
                                    let run = run_account_submit.clone();
                                    move |event: KeyboardEvent| {
                                        if matches!(event.data().key(), Key::Enter)
                                            && !event.data().modifiers().contains(Modifiers::SHIFT)
                                        {
                                            event.prevent_default();
                                            run();
                                        }
                                    }
                                },
                            }
                        }
                        if let Some(error) = account_error() {
                            p { class: "text-sm text-destructive", "{error.user_message()}" }
                        }
                        div { class: "flex justify-end",
                            button {
                                id: ControlId::OnboardingCreateAccountButton
                                    .required_dom_id("ControlId::OnboardingCreateAccountButton"),
                                class: "inline-flex h-10 items-center justify-center rounded-md bg-primary px-6 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50",
                                disabled: creating_account() || account_name().trim().is_empty(),
                                onclick: submit_account,
                                if creating_account() {
                                    "Creating Account..."
                                } else {
                                    "Create Account"
                                }
                            }
                        }
                        // Divider
                        div { class: "flex items-center gap-3 py-3",
                            div { class: "h-px flex-1 bg-border" }
                            span { class: "text-[11px] font-medium uppercase tracking-[0.08em] text-muted-foreground", "or" }
                            div { class: "h-px flex-1 bg-border" }
                        }
                        if !bootstrap_candidates().is_empty() {
                            div { class: "rounded-md border border-border bg-muted/20 p-3 space-y-2",
                                div {
                                    class: "space-y-1",
                                    p { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Local devices available for enrollment" }
                                    p { class: "text-sm text-muted-foreground",
                                        "Open Aura on one of these runtimes and use its invite action to send a device enrollment code here."
                                    }
                                }
                                ul { class: "space-y-2",
                                    for candidate in bootstrap_candidates().iter() {
                                        li {
                                            class: "rounded-sm border border-border bg-background px-3 py-2 text-sm",
                                            div { class: "font-medium text-foreground",
                                                "{candidate.nickname_suggestion.clone().unwrap_or_else(|| candidate.authority_id.to_string())}"
                                            }
                                            div { class: "text-xs text-muted-foreground",
                                                {
                                                    let origin_label = match candidate.origin {
                                                        BootstrapCandidateOrigin::Lan => "LAN",
                                                        BootstrapCandidateOrigin::LocalBroker => "Local broker",
                                                        BootstrapCandidateOrigin::LanBroker => "LAN broker",
                                                    };
                                                    format!("{origin_label} • {}", candidate.address)
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Join an existing account
                        if let Some(notice) = bootstrap_delivery_notice {
                            p {
                                role: "alert",
                                class: "rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive",
                                "An invitation could not be received. You can still enter an enrollment code and approve its transfer below. {notice}"
                            }
                        }
                        label {
                            class: "block space-y-2",
                            span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Join an existing account" }
                            aura_ui::rendered_control::RenderedInput {
                                control_id: ControlId::Field(FieldId::DeviceImportCode),
                                class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                placeholder: "Enter device enrollment code...",
                                value: "{import_code()}",
                                disabled: importing_code(),
                                oninput: move |event: FormEvent| {
                                    import_code.set(event.value());
                                    import_error.set(None);
                                },
                                onkeydown: {
                                    let import_code = import_code.clone();
                                    let importing_code = importing_code.clone();
                                    let run_import = run_import.clone();
                                    move |event: KeyboardEvent| {
                                        if matches!(event.data().key(), Key::Enter)
                                            && !event.data().modifiers().contains(Modifiers::SHIFT)
                                        {
                                            event.prevent_default();
                                            let code = import_code();
                                            if !code.trim().is_empty() && !importing_code() {
                                                run_import(code);
                                            }
                                        }
                                    }
                                },
                            }
                        }
                        label {
                            class:"block space-y-2",
                            span {"Signed enrollment manifest"}
                            aura_ui::rendered_control::RenderedInput {
                                control_id: ControlId::Field(FieldId::DeviceImportManifest),
                                class:"w-full rounded-md border bg-background px-3 py-2 text-sm",
                                value:"{import_manifest()}",placeholder:"Paste signed manifest code...",
                                disabled:importing_code(),
                                oninput:move |event: FormEvent|{import_manifest.set(event.value());import_error.set(None);},
                            }
                        }
                        label {
                            class:"block space-y-2",
                            span {"Initiator verifier (separate transfer)"}
                            aura_ui::rendered_control::RenderedInput {
                                control_id: ControlId::Field(FieldId::DeviceImportInitiatorVerifier),
                                class:"w-full rounded-md border bg-background px-3 py-2 text-sm",
                                value:"{import_initiator_verifier()}",placeholder:"Paste separately transferred verifier...",
                                disabled:importing_code(),
                                oninput:move |event: FormEvent|{import_initiator_verifier.set(event.value());import_error.set(None);},
                            }
                        }
                        if let Some(error) = import_error() {
                            p { class: "text-sm text-destructive", "{error.user_message()}" }
                        }
                        div { class: "flex justify-end",
                            button {
                                id: ControlId::OnboardingImportDeviceButton
                                    .required_dom_id("ControlId::OnboardingImportDeviceButton"),
                                class: "inline-flex h-10 items-center justify-center rounded-md bg-primary px-6 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50",
                                disabled: importing_code() || import_code().trim().is_empty() || import_manifest().trim().is_empty() || import_initiator_verifier().trim().is_empty(),
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
    }
}

#[cfg(all(test, target_arch = "wasm32", feature = "web"))]
#[path = "enrollment_browser_tests.rs"]
mod enrollment_browser_tests;
