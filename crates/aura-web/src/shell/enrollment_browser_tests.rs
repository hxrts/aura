use super::*;
use aura_app::ui::contract::{OperationState, RenderedControlFrame, UiSnapshot};
use futures::{channel::mpsc, StreamExt};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn authority_account_configs_remain_separate_and_foreign_key_refuses() {
    let operation = aura_app::frontend_primitives::FrontendUiOperation::ImportDeviceEnrollmentCode;
    let bob = aura_core::AuthorityId::new_from_entropy([191; 32]);
    let alice = aura_core::AuthorityId::new_from_entropy([192; 32]);
    let scope = crate::shell::storage::WebStorageScope::active();
    let storage = web_sys::window()
        .expect("browser")
        .local_storage()
        .expect("storage access")
        .expect("storage");
    let keys = [
        scope.account_config_key(),
        scope.authority_account_config_key(bob),
        scope.authority_account_config_key(alice),
    ];
    let previous = keys
        .iter()
        .map(|key| storage.get_item(key).expect("read fixture storage"))
        .collect::<Vec<_>>();
    let bob_config = aura_app::ui::types::AccountConfig::new(
        bob,
        aura_agent::core::default_context_id_for_authority(bob),
        Some("Bob".into()),
        1,
    );
    let alice_config = aura_app::ui::types::AccountConfig::new(
        alice,
        aura_agent::core::default_context_id_for_authority(alice),
        Some("Alice".into()),
        2,
    );
    crate::bootstrap_storage::persist_account_config(operation, &bob_config)
        .expect("persist Bob profile");
    crate::bootstrap_storage::persist_account_config(operation, &alice_config)
        .expect("persist Alice profile");
    assert_eq!(
        crate::bootstrap_storage::load_persisted_account_config_for_authority(operation, bob)
            .expect("load Bob profile"),
        Some(bob_config.clone()),
    );
    assert_eq!(
        crate::bootstrap_storage::load_persisted_account_config_for_authority(operation, alice)
            .expect("load Alice profile"),
        Some(alice_config.clone()),
    );
    storage
        .set_item(
            &scope.authority_account_config_key(alice),
            &serde_json::to_string(&bob_config).expect("serialize foreign config"),
        )
        .expect("stage foreign config");
    assert!(
        crate::bootstrap_storage::load_persisted_account_config_for_authority(operation, alice)
            .is_err(),
        "foreign stored config must refuse before legacy fallback"
    );
    for (key, value) in keys.iter().zip(previous) {
        match value {
            Some(value) => storage
                .set_item(key, &value)
                .expect("restore fixture storage"),
            None => storage.remove_item(key).expect("clear fixture storage"),
        }
    }
}

struct MountedForm {
    state: BootstrapState,
    root: web_sys::Element,
    events: mpsc::Receiver<UiSnapshot>,
    owner: crate::task_owner::WebTaskOwner,
    capture: u64,
}

impl MountedForm {
    async fn mount(state: BootstrapState) -> Self {
        console_log!("task311 mount actual form");
        let (sender, events) = mpsc::channel(256);
        let sender = async_lock::Mutex::new(sender);
        state
            .controller
            .set_ui_snapshot_sink(Arc::new(move |snapshot| {
                sender
                    .try_lock()
                    .expect("single renderer producer")
                    .try_send(snapshot)
                    .expect("bounded fixture events");
            }));
        let document = web_sys::window()
            .expect("browser")
            .document()
            .expect("document");
        let root = document.create_element("div").expect("mount root");
        document
            .body()
            .expect("body")
            .append_child(&root)
            .expect("attach root");
        let mut dom = VirtualDom::new_with_props(
            BootstrappedApp,
            BootstrappedAppProps {
                state: state.clone(),
            },
        );
        let observer = crate::shell::rendered_controls::BrowserRendererObserver::default();
        observer.install_context(&mut dom);
        let config = observer.config(dioxus::web::Config::new().rootnode(root.clone().into()));
        let owner = crate::task_owner::new_web_task_owner();
        owner.spawn_local_cancellable(async move { dioxus::web::run(dom, config).await });
        let mut mounted = Self {
            state,
            root,
            events,
            owner,
            capture: 0,
        };
        console_log!("task311 awaiting initial renderer frame");
        mounted.next_frame().await;
        console_log!("task311 initial renderer frame received");
        mounted
    }

    async fn next_frame(&mut self) -> UiSnapshot {
        loop {
            let snapshot = self.events.next().await.expect("actual renderer event");
            console_log!(
                "task311 frame {:?} controls {:?} operations {:?}",
                snapshot.rendered_control_frame,
                snapshot.rendered_controls,
                snapshot.operations
            );
            if let RenderedControlFrame::Committed {
                capture_revision, ..
            } = snapshot.rendered_control_frame
            {
                if capture_revision > self.capture
                    && snapshot
                        .rendered_controls
                        .iter()
                        .any(|control| control.control == ControlId::OnboardingRoot)
                {
                    self.capture = capture_revision;
                    return snapshot;
                }
            }
        }
    }

    fn input(&self, field: FieldId) -> web_sys::HtmlInputElement {
        let id = ControlId::Field(field).required_dom_id("enrollment fixture field");
        self.root
            .query_selector(&format!("#{id}"))
            .expect("valid selector")
            .expect("actual mounted field")
            .dyn_into()
            .expect("input element")
    }

    async fn edit(&mut self, field: FieldId, value: &str) {
        let input = self.input(field);
        input.set_value(value);
        let init = web_sys::EventInit::new();
        init.set_bubbles(true);
        let event = web_sys::Event::new_with_event_init_dict("input", &init).expect("input event");
        console_log!("task311 dispatch field {field:?}");
        input.dispatch_event(&event).expect("dispatch actual input");
        self.next_frame().await;
    }

    async fn approve_transferred_enrollment(
        &mut self,
        code: &str,
        transfer: &aura_app::ui::contract::EnrollmentManifestTransferInput,
    ) {
        self.edit(FieldId::DeviceImportCode, code).await;
        self.edit(FieldId::DeviceImportManifest, &transfer.manifest_code)
            .await;
        self.edit(
            FieldId::DeviceImportInitiatorVerifier,
            &transfer.initiator_verifier_code,
        )
        .await;
        let button_id = ControlId::OnboardingImportDeviceButton.required_dom_id("import button");
        let button: web_sys::HtmlElement = self
            .root
            .query_selector(&format!("#{button_id}"))
            .expect("selector")
            .expect("actual approval button")
            .dyn_into()
            .expect("button");
        assert!(!button.has_attribute("disabled"));
        button.click();
    }

    async fn close(self) {
        self.state
            .controller
            .clear_ui_snapshot_sink()
            .expect("detach original fixture observer before renderer disposal");
        self.owner.shutdown();
        self.owner.wait_drained().await;
        self.root.remove();
    }
}

async fn form_state_with_app(
    app: Arc<async_lock::RwLock<aura_app::AppCore>>,
    pending: String,
) -> BootstrapState {
    aura_app::AppCore::init_signals_with_hooks(&app)
        .await
        .expect("canonical app signal initialization");
    let controller = Arc::new(aura_ui::UiController::new(
        app,
        Arc::new(aura_ui::MemoryClipboard::default()),
    ));
    BootstrapState::enrollment_form_fixture(controller, pending)
}

async fn form_state() -> BootstrapState {
    let app = aura_app::AppCore::new(aura_app::AppConfig::default()).expect("fixture app");
    form_state_with_app(
        Arc::new(async_lock::RwLock::new(app)),
        "broker-candidate".into(),
    )
    .await
}

#[wasm_bindgen_test]
async fn replacement_controller_refuses_import_without_its_own_host_ack_owner() {
    let first = form_state().await;
    first.controller.set_enrollment_import_host_policy(
        crate::workflows::enrollment_import_host_policy(
            first.controller.app_core().clone(),
            active_storage_prefix(),
        ),
    );
    let replacement = Arc::new(aura_ui::UiController::new(
        first.controller.app_core().clone(),
        Arc::new(aura_ui::MemoryClipboard::default()),
    ));
    assert!(matches!(
        replacement.enrollment_import_host_policy(),
        aura_app::ui::workflows::invitation::EnrollmentImportHostPolicy::RuntimeOnly
    ));
    let refusal = aura_ui::semantic_lifecycle::begin_exact_enrollment_import(
        replacement.clone(),
        "untrusted-code".to_string(),
        None,
        replacement.enrollment_import_host_policy(),
    )
    .err()
    .expect("foreign controller must not inherit first controller's host owner");
    assert!(refusal.contains("host persistence acknowledgment"));
}

#[wasm_bindgen_test]
async fn bootstrap_delivery_refusal_is_visible_without_approving_enrollment() {
    let mut state = form_state().await;
    state.set_bootstrap_delivery_error_for_test(
        WebUiError::operation(
            WebUiOperation::BootstrapController,
            "WEB_BOOTSTRAP_BATCH_REFUSED",
            "bootstrap receiver returned a typed refusal",
        )
        .with_source(aura_core::AuraError::invalid(
            "original bootstrap delivery was refused",
        )),
    );
    let form = MountedForm::mount(state).await;
    let notice = form
        .root
        .query_selector("[role=alert]")
        .expect("valid alert selector")
        .expect("actual mounted refusal notice");
    let message = notice.text_content().expect("visible refusal text");
    assert!(message.contains("WEB_BOOTSTRAP_BATCH_REFUSED"));
    assert!(message.contains("approve its transfer"));
    assert_eq!(
        form.input(FieldId::DeviceImportCode).value(),
        "broker-candidate"
    );
    assert!(!form.state.account_ready);
    assert!(form
        .state
        .controller
        .semantic_model_snapshot()
        .operations
        .is_empty());
    form.close().await;
}

#[wasm_bindgen_test]
async fn broker_prefill_is_one_shot_and_preserves_dirty_form() {
    let state = form_state().await;
    let mut form = MountedForm::mount(state.clone()).await;
    assert_eq!(
        form.input(FieldId::DeviceImportCode).value(),
        "broker-candidate"
    );
    form.edit(FieldId::DeviceImportCode, "user-transfer").await;
    form.edit(FieldId::DeviceImportManifest, "user-manifest")
        .await;
    assert_eq!(
        form.input(FieldId::DeviceImportCode).value(),
        "user-transfer"
    );
    assert_eq!(
        form.input(FieldId::DeviceImportManifest).value(),
        "user-manifest"
    );
    assert!(state.claim_enrollment_prefill().is_none());
    form.close().await;
    let remounted = MountedForm::mount(state).await;
    assert!(remounted
        .input(FieldId::DeviceImportCode)
        .value()
        .is_empty());
    remounted.close().await;
}

#[wasm_bindgen_test]
async fn broker_prefill_does_not_submit_without_user_approval() {
    let mut form = MountedForm::mount(form_state().await).await;
    assert!(form
        .state
        .controller
        .semantic_model_snapshot()
        .operations
        .is_empty());
    form.edit(FieldId::DeviceImportManifest, "independent-manifest")
        .await;
    form.edit(
        FieldId::DeviceImportInitiatorVerifier,
        "independent-verifier",
    )
    .await;
    assert!(form
        .state
        .controller
        .semantic_model_snapshot()
        .operations
        .is_empty());
    assert!(!form
        .state
        .controller
        .semantic_model_snapshot()
        .operations
        .iter()
        .any(|operation| operation.state == OperationState::Submitting));
    form.close().await;
}

#[wasm_bindgen_test]
async fn import_failure_preserves_original_identity_and_pending_transfer() {
    let mut form = MountedForm::mount(form_state().await).await;
    let before = form.state.controller.semantic_model_snapshot();
    let identity_key =
        crate::shell::storage::selected_runtime_identity_key(&active_storage_prefix());
    let original_identity = aura_app::ui::types::BootstrapRuntimeIdentity::new(
        aura_core::AuthorityId::new_from_entropy([181; 32]),
        aura_core::DeviceId::new_from_entropy([182; 32]),
    );
    let previous_identity = crate::shell::storage::load_selected_runtime_identity(&identity_key)
        .expect("read original selection");
    crate::shell::storage::persist_selected_runtime_identity(&identity_key, &original_identity)
        .expect("stage existing selection data");
    form.approve_transferred_enrollment(
        "broker-candidate",
        &aura_app::ui::contract::EnrollmentManifestTransferInput {
            manifest_code: "invalid-manifest".into(),
            initiator_verifier_code: "invalid-verifier".into(),
        },
    )
    .await;
    loop {
        let snapshot = form.next_frame().await;
        if snapshot
            .operations
            .iter()
            .any(|operation| operation.state == OperationState::Failed)
        {
            assert!(snapshot
                .operations
                .iter()
                .any(|operation| operation.failure.is_some()));
            break;
        }
    }
    assert_eq!(
        form.input(FieldId::DeviceImportCode).value(),
        "broker-candidate"
    );
    assert_eq!(
        form.input(FieldId::DeviceImportManifest).value(),
        "invalid-manifest"
    );
    assert_eq!(
        form.input(FieldId::DeviceImportInitiatorVerifier).value(),
        "invalid-verifier"
    );
    assert_eq!(
        form.state.pending_device_enrollment_code.as_deref(),
        Some("broker-candidate")
    );
    assert_eq!(
        form.state.controller.semantic_model_snapshot().screen,
        before.screen
    );
    assert!(!form.state.account_ready);
    assert_eq!(
        crate::shell::storage::load_selected_runtime_identity(&identity_key)
            .expect("read retained selection"),
        Some(original_identity)
    );
    if let Some(previous) = previous_identity {
        crate::shell::storage::persist_selected_runtime_identity(&identity_key, &previous)
            .expect("restore fixture storage");
    } else {
        web_sys::window()
            .expect("browser")
            .local_storage()
            .expect("storage access")
            .expect("storage")
            .remove_item(&identity_key)
            .expect("remove fixture selection");
    }
    form.close().await;
}
