//! VM hardening profile tests for Aura's telltale runtime integration.

#![cfg(feature = "choreo-backend-telltale-machine")]
#![allow(clippy::expect_used)]

use std::sync::Arc;

use aura_agent::{
    apply_protocol_execution_policy, apply_scheduler_execution_policy,
    aura_output_predicate_allow_list, build_vm_config, configured_guard_capacity,
    policy_for_protocol, scheduler_control_input_for_protocol_machine_image,
    scheduler_policy_for_input, AuraChoreoEngine, AuraChoreoEngineError, AuraVmEffectHandler,
    AuraVmHardeningProfile, AuraVmParityProfile, AuraVmRuntimeSelector, AuraVmSchedulerSignals,
    AURA_VM_SCHED_PRIORITY_AGING, AURA_VM_SCHED_PROGRESS_AWARE,
};
use aura_mpst::upstream::types::{GlobalType, Label};
use telltale_machine::runtime::loader::CodeImage;
use telltale_machine::{
    model::effects::{EffectFailure, EffectHandler, EffectResult},
    runtime::loader::CodeImage as ProtocolMachineCodeImage,
    AuthoritativeReadKind, AuthoritativeReadLifecycle, FinalizationStage, ObsEvent,
    OutputConditionHint, RunStatus, RuntimeContracts, SessionId,
    TopologyPerturbation as ProtocolMachineTopologyPerturbation, Value,
};

fn simple_send_image() -> CodeImage {
    let global = GlobalType::send("Sender", "Receiver", Label::new("msg"), GlobalType::End);
    let locals = aura_mpst::upstream::theory::projection::project_all(&global)
        .expect("projection must succeed")
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    CodeImage::from_local_types(&locals, &global)
}

fn protocol_machine_image(image: &CodeImage) -> ProtocolMachineCodeImage {
    let image = ProtocolMachineCodeImage::from_local_types(&image.local_types, &image.global_type);
    image.validate_runtime_shape().expect("runtime image");
    image
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
    let runtime_image = protocol_machine_image(&image);

    engine
        .open_protocol_machine_session(&runtime_image)
        .expect("open session");
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
    ) -> EffectResult<Value> {
        EffectResult::success(Value::Unit)
    }

    fn handle_recv(
        &self,
        _role: &str,
        _partner: &str,
        _label: &str,
        _state: &mut Vec<Value>,
        _payload: &Value,
    ) -> EffectResult<()> {
        EffectResult::success(())
    }

    fn handle_choose(
        &self,
        _role: &str,
        _partner: &str,
        labels: &[String],
        _state: &[Value],
    ) -> EffectResult<String> {
        labels
            .first()
            .cloned()
            .map(EffectResult::success)
            .unwrap_or_else(|| {
                EffectResult::failure(EffectFailure::contract_violation("no labels available"))
            })
    }

    fn step(&self, _role: &str, _state: &mut Vec<Value>) -> EffectResult<()> {
        EffectResult::success(())
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
    let runtime_image = protocol_machine_image(&image);

    engine
        .open_protocol_machine_session(&runtime_image)
        .expect("open session");
    let err = engine
        .run(32)
        .expect_err("run must fail on unknown predicate");
    match err {
        AuraChoreoEngineError::OutputConditionRejected {
            predicate_ref,
            tick,
            witness_ref,
            finalization_path,
            ..
        } => {
            assert_eq!(predicate_ref, "aura.unknown");
            assert!(
                tick.is_some(),
                "output condition diagnostics should include tick"
            );
            assert_eq!(witness_ref.as_deref(), Some("unknown-witness"));
            let finalization_path =
                finalization_path.expect("rejection should expose finalization path");
            assert_eq!(finalization_path.stage, FinalizationStage::Rejected);
            assert!(
                !finalization_path.proof_ids.is_empty(),
                "rejection should expose public materialization proof refs"
            );
            assert!(
                !finalization_path.publication_ids.is_empty(),
                "rejection should expose public publication refs"
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }

    let semantic_objects = engine.vm_semantic_objects();
    assert!(
        semantic_objects.authoritative_reads.iter().any(|read| {
            read.kind == AuthoritativeReadKind::OutputCondition
                && read.lifecycle == AuthoritativeReadLifecycle::Rejected
                && read.predicate_ref.as_deref() == Some("aura.unknown")
                && read.reason.as_deref() == Some("output condition failed")
        }),
        "rejected output conditions should surface as rejected authoritative reads"
    );
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

#[tokio::test]
async fn envelope_bounded_admission_fails_closed_when_runtime_capability_missing() {
    let policy = policy_for_protocol("aura.sync.epoch_rotation", None).expect("policy");
    let selector = AuraVmRuntimeSelector::for_policy(policy);
    let mut config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::RuntimeDefault,
    );
    apply_protocol_execution_policy(&mut config, policy);
    let image = simple_send_image();
    let runtime_image = protocol_machine_image(&image);
    let scheduler_input = scheduler_control_input_for_protocol_machine_image(
        &runtime_image,
        policy.protocol_class,
        configured_guard_capacity(&config),
        AuraVmSchedulerSignals::default(),
    );
    let scheduler_policy = scheduler_policy_for_input(scheduler_input);
    apply_scheduler_execution_policy(&mut config, &scheduler_policy);

    let mut contracts = RuntimeContracts::full();
    contracts.determinism_artifacts.full = false;
    let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts_and_selector(
        config,
        Arc::new(AuraVmEffectHandler::default()),
        Some(contracts),
        selector,
    )
    .expect("engine");

    let err = engine
        .open_protocol_machine_session_for_policy_admitted(&runtime_image, policy, &[])
        .await
        .expect_err("admission must fail without derived envelope capability");
    assert!(matches!(
        err,
        AuraChoreoEngineError::MissingRuntimeCapability { .. }
    ));
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
    let runtime_image = protocol_machine_image(&image);

    engine
        .open_protocol_machine_session(&runtime_image)
        .expect("open session");
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
            ProtocolMachineTopologyPerturbation::Crash {
                site: "prod-topology-node".to_string(),
            },
        );
    }
    let mut engine = AuraChoreoEngine::new(config, Arc::clone(&handler));
    let image = simple_send_image();
    let runtime_image = protocol_machine_image(&image);

    engine
        .open_protocol_machine_session(&runtime_image)
        .expect("open session");
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
            Some(ProtocolMachineTopologyPerturbation::Crash { ref site }) if site == "prod-topology-node"
        )),
        "expected scheduled topology crash to appear in trace"
    );
}

#[tokio::test]
async fn admitted_sync_sessions_select_progress_aware_scheduler() {
    let image = simple_send_image();
    let handler = Arc::new(AuraVmEffectHandler::default());
    let policy = policy_for_protocol("aura.sync.epoch_rotation", None).expect("policy");
    let mut config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    apply_protocol_execution_policy(&mut config, policy);
    let runtime_image = protocol_machine_image(&image);
    let scheduler_input = scheduler_control_input_for_protocol_machine_image(
        &runtime_image,
        policy.protocol_class,
        configured_guard_capacity(&config),
        AuraVmSchedulerSignals::default(),
    );
    let scheduler_policy = scheduler_policy_for_input(scheduler_input);
    assert_eq!(scheduler_policy.policy_ref, AURA_VM_SCHED_PROGRESS_AWARE);
    apply_scheduler_execution_policy(&mut config, &scheduler_policy);

    let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts(
        config,
        handler,
        Some(RuntimeContracts::full()),
    )
    .expect("engine with admitted scheduler policy");
    engine
        .open_protocol_machine_session_for_policy_admitted(&runtime_image, policy, &[])
        .await
        .expect("admitted session should open");

    assert_eq!(
        engine.vm_config().sched_policy,
        scheduler_policy.sched_policy
    );
}

#[tokio::test]
async fn admission_rejects_scheduler_drift_under_budget_pressure() {
    let image = simple_send_image();
    let handler = Arc::new(AuraVmEffectHandler::default());
    handler.set_scheduler_signals(AuraVmSchedulerSignals {
        guard_contention_events: 0,
        flow_budget_pressure_bps: 8_200,
        leakage_budget_pressure_bps: 0,
    });

    let policy = policy_for_protocol("aura.recovery.grant", None).expect("policy");
    let mut config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    apply_protocol_execution_policy(&mut config, policy);
    let runtime_image = protocol_machine_image(&image);

    let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts(
        config,
        handler,
        Some(RuntimeContracts::full()),
    )
    .expect("engine should admit base config");
    let err = engine
        .open_protocol_machine_session_for_policy_admitted(&runtime_image, policy, &[])
        .await
        .expect_err("scheduler mismatch must fail admission");

    match err {
        AuraChoreoEngineError::Interpreter { message } => {
            assert!(message.contains("unsupported VM scheduler profile"));
            assert!(message.contains(AURA_VM_SCHED_PRIORITY_AGING));
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}
