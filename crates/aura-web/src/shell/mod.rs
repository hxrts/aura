pub(crate) mod app;
pub(crate) mod bootstrap;
pub(crate) mod maintenance;
mod rendered_controls;
pub(crate) mod storage;

pub(crate) use app::App;
pub(crate) use bootstrap::{
    apply_harness_mode_document_flags, stage_initial_web_account_bootstrap,
    stage_runtime_bound_web_account_bootstrap, submit_runtime_bootstrap_handoff,
    submit_runtime_bootstrap_handoff_accepted,
};
pub(crate) use maintenance::{
    cancel_generation_maintenance_loops, spawn_generation_maintenance_loops,
};
pub(crate) use storage::{
    active_storage_prefix, bootstrap_broker_auth_token, bootstrap_broker_invitation_token,
    bootstrap_broker_url, clear_pending_device_enrollment_code, clear_storage_key,
    dual_demo_web_enabled, harness_instance_id, harness_mode_enabled,
    load_pending_account_bootstrap, load_pending_device_enrollment_code,
    load_selected_runtime_identity, logged_optional, pending_account_bootstrap_key,
    pending_device_enrollment_code_key, persist_pending_device_enrollment_code,
    persist_selected_runtime_identity, selected_runtime_identity_key,
};

#[cfg(all(test, target_arch = "wasm32", feature = "web"))]
mod rendered_controls_browser_tests;

pub(crate) use rendered_controls::{capture_current_layout, launch_observed};
