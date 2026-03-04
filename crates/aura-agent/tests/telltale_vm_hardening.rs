//! VM hardening profile tests for Aura's telltale runtime integration.

#![cfg(feature = "choreo-backend-telltale-vm")]
#![allow(clippy::expect_used)]

use std::sync::Arc;

use aura_agent::{
    aura_output_predicate_allow_list, build_vm_config, AuraChoreoEngine, AuraChoreoEngineError,
    AuraVmEffectHandler, AuraVmHardeningProfile, AuraVmParityProfile,
};
use telltale_types::{GlobalType, Label};
use telltale_vm::effect::EffectHandler;
use telltale_vm::effect::TopologyPerturbation;
use telltale_vm::loader::CodeImage;
use telltale_vm::output_condition::OutputConditionHint;
use telltale_vm::vm::{ObsEvent, RunStatus};
use telltale_vm::{SessionId, Value};

fn simple_send_image() -> CodeImage {
    let global = GlobalType::send("Sender", "Receiver", Label::new("msg"), GlobalType::End);
    let locals = telltale_theory::projection::project_all(&global)
        .expect("projection must succeed")
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    CodeImage::from_local_types(&locals, &global)
}

#[test]
fn ci_profile_allows_known_output_predicates_from_aura_handler() {
    let config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::NativeCooperative,
    );
    let handler = Arc::new(AuraVmEffectHandler::default());
    let mut engine = AuraChoreoEngine::new(config, handler);
    let image = simple_send_image();

    engine.open_session(&image).expect("open session");
    let status = engine.run(32).expect("run should succeed");
    assert_eq!(status, RunStatus::AllDone);

    let allowed = aura_output_predicate_allow_list();
    let checked_predicates = engine
        .vm()
        .trace()
        .iter()
        .filter_map(|event| match event {
            ObsEvent::OutputConditionChecked { predicate_ref, .. } => Some(predicate_ref.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(!checked_predicates.is_empty());
    assert!(
        checked_predicates
            .iter()
            .all(|predicate| allowed.iter().any(|allowed| allowed == predicate)),
        "all emitted output predicates must be in Aura allowlist"
    );
}

#[derive(Default)]
struct UnknownPredicateHandler;

impl EffectHandler for UnknownPredicateHandler {
    fn handle_send(
        &self,
        _role: &str,
        _partner: &str,
        _label: &str,
        _state: &[Value],
    ) -> Result<Value, String> {
        Ok(Value::Unit)
    }

    fn handle_recv(
        &self,
        _role: &str,
        _partner: &str,
        _label: &str,
        _state: &mut Vec<Value>,
        _payload: &Value,
    ) -> Result<(), String> {
        Ok(())
    }

    fn handle_choose(
        &self,
        _role: &str,
        _partner: &str,
        labels: &[String],
        _state: &[Value],
    ) -> Result<String, String> {
        labels
            .first()
            .cloned()
            .ok_or_else(|| "no labels available".to_string())
    }

    fn step(&self, _role: &str, _state: &mut Vec<Value>) -> Result<(), String> {
        Ok(())
    }

    fn output_condition_hint(
        &self,
        _sid: SessionId,
        _role: &str,
        _state: &[Value],
    ) -> Option<OutputConditionHint> {
        Some(OutputConditionHint {
            predicate_ref: "aura.unknown".to_string(),
            witness_ref: Some("unknown-witness".to_string()),
        })
    }
}

#[test]
fn ci_profile_rejects_unknown_output_predicates_with_diagnostics() {
    let config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::NativeCooperative,
    );
    let handler = Arc::new(UnknownPredicateHandler);
    let mut engine = AuraChoreoEngine::new(config, handler);
    let image = simple_send_image();

    engine.open_session(&image).expect("open session");
    let err = engine
        .run(32)
        .expect_err("run must fail on unknown predicate");
    match err {
        AuraChoreoEngineError::OutputConditionRejected {
            predicate_ref,
            tick,
            witness_ref,
            ..
        } => {
            assert_eq!(predicate_ref, "aura.unknown");
            assert!(
                tick.is_some(),
                "output condition diagnostics should include tick"
            );
            assert_eq!(witness_ref.as_deref(), Some("unknown-witness"));
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[tokio::test]
async fn admission_fails_deterministically_when_byzantine_capability_missing() {
    let config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::NativeCooperative,
    );
    let handler = Arc::new(AuraVmEffectHandler::default());
    let engine = AuraChoreoEngine::new(config, handler);

    let first = engine
        .admit_bundle(&["byzantine_envelope"])
        .await
        .expect_err("admission should fail with empty capability inventory");
    let second = engine
        .admit_bundle(&["byzantine_envelope"])
        .await
        .expect_err("admission should fail consistently");

    let first_ref = match first {
        AuraChoreoEngineError::MissingRuntimeCapability { capability } => capability,
        other => panic!("unexpected error variant: {other:?}"),
    };
    let second_ref = match second {
        AuraChoreoEngineError::MissingRuntimeCapability { capability } => capability,
        other => panic!("unexpected error variant: {other:?}"),
    };

    assert_eq!(
        first_ref, second_ref,
        "missing capability ref should be stable"
    );
    assert_ne!(
        first_ref, "byzantine_envelope",
        "error should expose redacted capability reference"
    );
}

#[test]
fn run_emits_bound_exceeded_when_step_budget_is_exhausted() {
    let config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::NativeCooperative,
    );
    let handler = Arc::new(AuraVmEffectHandler::default());
    let mut engine = AuraChoreoEngine::new(config, handler);
    let image = simple_send_image();

    engine.open_session(&image).expect("open session");
    let err = engine
        .run(1)
        .expect_err("run should fail when deterministic step budget is exhausted");
    assert!(
        matches!(err, AuraChoreoEngineError::BoundExceeded { .. }),
        "expected BoundExceeded, got: {err:?}"
    );
}

#[test]
fn prod_profile_topology_only_capture_records_topology_events() {
    let config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    let handler = Arc::new(AuraVmEffectHandler::default());
    for tick in 0..=8 {
        handler.schedule_topology_event(
            tick,
            TopologyPerturbation::Crash {
                site: "prod-topology-node".to_string(),
            },
        );
    }
    let mut engine = AuraChoreoEngine::new(config, Arc::clone(&handler));
    let image = simple_send_image();

    engine.open_session(&image).expect("open session");
    let status = engine.run(32).expect("run should succeed");
    assert_eq!(status, RunStatus::AllDone);

    let effect_trace = engine.vm().effect_trace();
    assert!(
        !effect_trace.is_empty(),
        "topology-only mode should still capture topology events"
    );
    assert!(
        effect_trace
            .iter()
            .all(|entry| entry.effect_kind == "topology_event"),
        "prod profile should capture topology events only"
    );
    assert!(
        effect_trace.iter().any(|entry| matches!(
            entry.topology,
            Some(TopologyPerturbation::Crash { ref site }) if site == "prod-topology-node"
        )),
        "expected scheduled topology crash to appear in trace"
    );
}
