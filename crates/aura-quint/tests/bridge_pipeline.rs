//! Bridge pipeline integration tests.
//!
//! Verifies the full Quint bridge pipeline: parse → compile → evaluate →
//! extract. Tests cross-validation, bundle export, and invariant module
//! generation.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use aura_quint::{
    export_quint_to_telltale_bundle, generate_quint_invariant_module, parse_telltale_properties,
    run_cross_validation, BridgeBundleV1, QuintModelCheckExecutor, StaticQuintExecutor,
};
use serde_json::Value as JsonValue;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("bridge")
        .join(name)
}

fn load_bundle_fixture(name: &str) -> BridgeBundleV1 {
    let payload = std::fs::read(fixture_path(name)).expect("read fixture");
    serde_json::from_slice(&payload).expect("decode bridge bundle fixture")
}

fn load_ir_fixture(name: &str) -> JsonValue {
    let payload = std::fs::read(fixture_path(name)).expect("read fixture");
    serde_json::from_slice(&payload).expect("decode quint ir fixture")
}

#[test]
fn bridge_pipeline_positive_fixture_cross_validates() {
    let bundle = load_bundle_fixture("positive_bundle.json");
    let imported = parse_telltale_properties(&bundle);
    assert_eq!(imported.len(), 1);
    let module = generate_quint_invariant_module("BridgePipeline", &imported)
        .expect("generate quint invariant module");
    assert!(module.contains("BridgeSafe"));

    let mut executor =
        StaticQuintExecutor::new(BTreeMap::from([("bridge_safe".to_string(), true)]));
    let report = run_cross_validation(&bundle, &mut executor).expect("run cross validation");
    assert!(
        report.is_consistent(),
        "positive fixture should have no discrepancies"
    );
    assert_eq!(report.properties_checked, 1);
    assert_eq!(report.certificates_compared, 1);
}

#[test]
fn bridge_pipeline_negative_fixture_detects_discrepancy() {
    let bundle = load_bundle_fixture("negative_bundle.json");
    let imported = parse_telltale_properties(&bundle);
    assert_eq!(imported.len(), 1);

    let mut executor =
        StaticQuintExecutor::new(BTreeMap::from([("bridge_violation".to_string(), false)]));
    let report = run_cross_validation(&bundle, &mut executor).expect("run cross validation");
    assert_eq!(report.discrepancies.len(), 1);
    assert!(!report.is_consistent());
    assert_eq!(report.discrepancies[0].property_id, "bridge_violation");
}

#[test]
fn bridge_pipeline_quint_ir_fixture_exports_bundle() {
    let ir = load_ir_fixture("quint_ir_fixture.json");
    let exported =
        export_quint_to_telltale_bundle(&ir, "tests/fixtures/bridge/quint_ir_fixture.json")
            .expect("export quint bundle from fixture");
    assert_eq!(exported.session_types.len(), 2);
    assert_eq!(
        exported.metadata.get("source").expect("source metadata"),
        "tests/fixtures/bridge/quint_ir_fixture.json"
    );
}

#[test]
fn bridge_pipeline_executor_failure_is_reported() {
    struct FailingExecutor;
    impl QuintModelCheckExecutor for FailingExecutor {
        fn check(&mut self, _property_id: &str, _expression: &str) -> Result<bool, String> {
            Err("executor unavailable".to_string())
        }
    }

    let bundle = load_bundle_fixture("positive_bundle.json");
    let mut executor = FailingExecutor;
    let err = run_cross_validation(&bundle, &mut executor).expect_err("expect executor failure");
    assert!(err.contains("executor unavailable"));
}

#[test]
fn bridge_pipeline_ci_discrepancy_artifact() {
    let positive_bundle = load_bundle_fixture("positive_bundle.json");
    let negative_bundle = load_bundle_fixture("negative_bundle.json");

    let mut positive_executor =
        StaticQuintExecutor::new(BTreeMap::from([("bridge_safe".to_string(), true)]));
    let positive = run_cross_validation(&positive_bundle, &mut positive_executor)
        .expect("run positive cross validation");

    let mut negative_executor =
        StaticQuintExecutor::new(BTreeMap::from([("bridge_violation".to_string(), false)]));
    let negative = run_cross_validation(&negative_bundle, &mut negative_executor)
        .expect("run negative cross validation");

    let artifact_dir = std::env::var("AURA_LEAN_QUINT_BRIDGE_ARTIFACT_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("artifacts/lean-quint-bridge"));
    std::fs::create_dir_all(&artifact_dir).expect("create bridge artifact directory");
    let artifact_path = artifact_dir.join("bridge_discrepancy_report.json");

    let payload = serde_json::json!({
        "schema_version": "aura.lean-quint-bridge.discrepancy.v1",
        "suite": "aura-quint bridge pipeline",
        "positive_fixture_consistent": positive.is_consistent(),
        "negative_fixture_discrepancies": negative.discrepancies.len(),
        "properties_checked_total": positive.properties_checked + negative.properties_checked,
        "certificates_compared_total": positive.certificates_compared + negative.certificates_compared,
        "fixtures": ["positive_bundle.json", "negative_bundle.json"]
    });
    std::fs::write(
        &artifact_path,
        serde_json::to_vec_pretty(&payload).expect("serialize discrepancy artifact"),
    )
    .expect("write discrepancy artifact");
    assert!(artifact_path.exists());
}
