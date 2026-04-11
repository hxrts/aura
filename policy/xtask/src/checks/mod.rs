mod arch;
mod policy;
mod runtime_typed_lifecycle_bridge;
mod support;

use anyhow::{bail, Result};

pub fn run(name: &str, args: &[String]) -> Result<()> {
    match name {
        "arch" => arch::run(args),
        "browser-restart-boundary" => policy::run_browser_restart_boundary(),
        "harness-authoritative-fact-boundary" => policy::run_harness_authoritative_fact_boundary(),
        "harness-actor-vs-move-ownership" => policy::run_harness_actor_vs_move_ownership(),
        "harness-ownership-policy" => policy::run_harness_ownership_policy(),
        "harness-typed-json-boundary" => policy::run_harness_typed_json_boundary(),
        "harness-typed-semantic-errors" => policy::run_harness_typed_semantic_errors(),
        "observed-layer-boundaries" => policy::run_observed_layer_boundaries(),
        "ownership-annotation-ratchet" => policy::run_ownership_annotation_ratchet(args),
        "ownership-category-declarations" => policy::run_ownership_category_declarations(),
        "ownership-policy" => policy::run_ownership_policy(),
        "ownership-workflow-tag-ratchet" => policy::run_ownership_workflow_tag_ratchet(),
        "privacy-legacy-sweep" => policy::run_privacy_legacy_sweep(),
        "privacy-runtime-locality" => policy::run_privacy_runtime_locality(),
        "protocol-choreo-wiring" => policy::run_protocol_choreo_wiring(),
        "protocol-device-enrollment-contract" => policy::run_protocol_device_enrollment_contract(),
        "runtime-typed-lifecycle-bridge" => runtime_typed_lifecycle_bridge::run(),
        "runtime-boundary-allowlist" => policy::run_runtime_boundary_allowlist(args),
        "runtime-error-boundary" => policy::run_runtime_error_boundary(),
        "runtime-shutdown-order" => policy::run_runtime_shutdown_order(),
        "service-registry-ownership" => policy::run_service_registry_ownership(),
        "service-surface-declarations" => policy::run_service_surface_declarations(),
        "testing-exception-boundary" => policy::run_testing_exception_boundary(),
        _ => bail!("unknown policy check: {name}"),
    }
}
