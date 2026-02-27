//! Differential conformance checks for reconfiguration baseline vs delegated runs.

#![allow(clippy::expect_used)]

use aura_core::{
    AuraConformanceArtifactV1, AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1,
    AuraEnvelopeLawClass, ConformanceSurfaceName,
};
use aura_testkit::{compare_artifacts, EnvelopeLawRegistry};

fn artifact_with_effects(effect_entries: Vec<serde_json::Value>) -> AuraConformanceArtifactV1 {
    let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
        target: "native".to_string(),
        profile: "ci".to_string(),
        scenario: "reconfiguration".to_string(),
        seed: Some(7),
        commit: None,
        async_host_transcript_entries: None,
        async_host_transcript_digest_hex: None,
    });
    artifact.insert_surface(
        ConformanceSurfaceName::Observable,
        AuraConformanceSurfaceV1::new(vec![serde_json::json!({"event": "ok"})], None),
    );
    artifact.insert_surface(
        ConformanceSurfaceName::SchedulerStep,
        AuraConformanceSurfaceV1::new(
            vec![
                serde_json::json!({"step": "delegate"}),
                serde_json::json!({"step": "verify"}),
            ],
            None,
        ),
    );
    artifact.insert_surface(
        ConformanceSurfaceName::Effect,
        AuraConformanceSurfaceV1::new(effect_entries, None),
    );
    artifact
}

#[test]
fn baseline_and_reconfigured_runs_match_under_declared_envelope() {
    let baseline = artifact_with_effects(vec![
        serde_json::json!({"effect_kind": "handle_recv", "msg": "commit"}),
        serde_json::json!({"effect_kind": "send_decision", "sid": "a"}),
        serde_json::json!({"effect_kind": "send_decision", "sid": "b"}),
        serde_json::json!({"effect_kind": "topology_event", "node": "g1"}),
    ]);
    let reconfigured = artifact_with_effects(vec![
        serde_json::json!({"effect_kind": "handle_recv", "msg": "commit"}),
        serde_json::json!({"effect_kind": "send_decision", "sid": "b"}),
        serde_json::json!({"effect_kind": "topology_event", "node": "g1"}),
        serde_json::json!({"effect_kind": "send_decision", "sid": "a"}),
    ]);

    let report = compare_artifacts(
        &baseline,
        &reconfigured,
        &EnvelopeLawRegistry::from_aura_registry(),
    );
    assert!(
        report.equivalent,
        "expected equivalence under commutative/algebraic envelope laws"
    );
}

#[test]
fn strict_effect_divergence_is_detected_under_fault_injection() {
    let baseline = artifact_with_effects(vec![
        serde_json::json!({"effect_kind": "handle_recv", "msg": "commit"}),
        serde_json::json!({"effect_kind": "send_decision", "sid": "a"}),
    ]);
    let faulty = artifact_with_effects(vec![
        serde_json::json!({"effect_kind": "handle_recv", "msg": "fault"}),
        serde_json::json!({"effect_kind": "send_decision", "sid": "a"}),
    ]);

    let report = compare_artifacts(
        &baseline,
        &faulty,
        &EnvelopeLawRegistry::from_aura_registry(),
    );
    assert!(
        !report.equivalent,
        "fault-injected strict mismatch must fail"
    );
    let mismatch = report.first_mismatch.expect("mismatch details");
    assert_eq!(mismatch.surface, ConformanceSurfaceName::Effect);
    assert_eq!(mismatch.law, Some(AuraEnvelopeLawClass::Strict));
}
