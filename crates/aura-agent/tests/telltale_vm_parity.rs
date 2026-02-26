//! Cross-target parity tests for Telltale VM backends used by Aura.
#![cfg(feature = "choreo-backend-telltale-vm")]
#![allow(clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;

use telltale_types::{GlobalType, Label, LocalTypeR};
use telltale_vm::coroutine::Value;
use telltale_vm::effect::EffectHandler;

fn ping_pong_global() -> GlobalType {
    GlobalType::send(
        "A",
        "B",
        Label::new("ping"),
        GlobalType::send("B", "A", Label::new("pong"), GlobalType::End),
    )
}

fn binary_choice_global() -> GlobalType {
    GlobalType::comm(
        "A",
        "B",
        vec![
            (Label::new("accept"), GlobalType::End),
            (Label::new("reject"), GlobalType::End),
        ],
    )
}

fn project_locals(global: &GlobalType) -> BTreeMap<String, LocalTypeR> {
    telltale_theory::projection::project_all(global)
        .expect("project global choreography to local session types")
        .into_iter()
        .collect()
}

#[derive(Debug, Default)]
struct NoOpHandler;

impl EffectHandler for NoOpHandler {
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
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{binary_choice_global, ping_pong_global, project_locals, NoOpHandler};
    use aura_agent::AuraEnvelopeParityPolicy;
    use telltale_vm::loader::CodeImage;
    use telltale_vm::serialization::canonical_replay_fragment_v1;
    use telltale_vm::threaded::ThreadedVM;
    use telltale_vm::vm::{ObsEvent, RunStatus, VMConfig, VM};
    use telltale_vm::{
        CommunicationReplayMode, EffectDeterminismTier, EffectTraceEntry, EnvelopeDiff,
    };

    struct ParityRun {
        obs_trace: Vec<ObsEvent>,
        effect_trace: Vec<EffectTraceEntry>,
    }

    fn normalize_handler_identity(trace: &[EffectTraceEntry]) -> Vec<EffectTraceEntry> {
        trace
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.handler_identity.clear();
                entry
            })
            .collect()
    }

    fn run_cooperative(image: &CodeImage) -> ParityRun {
        let mut vm = VM::new(VMConfig::default());
        let handler = NoOpHandler;
        vm.load_choreography(image).expect("load choreography");
        let status = vm.run(&handler, 64).expect("cooperative run");
        assert_eq!(status, RunStatus::AllDone);
        ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: vm.effect_trace().to_vec(),
        }
    }

    fn run_threaded(image: &CodeImage) -> ParityRun {
        let mut vm = ThreadedVM::with_workers(VMConfig::default(), 2);
        let handler = NoOpHandler;
        vm.load_choreography(image).expect("load choreography");
        let status = vm
            .run_concurrent(&handler, 64, 1)
            .expect("threaded run with canonical concurrency");
        assert_eq!(status, RunStatus::AllDone);
        ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: vm.effect_trace().to_vec(),
        }
    }

    fn run_cooperative_replay(image: &CodeImage, replay_trace: &[EffectTraceEntry]) -> ParityRun {
        let mut vm = VM::new(VMConfig::default());
        let handler = NoOpHandler;
        vm.load_choreography(image).expect("load choreography");
        let status = vm
            .run_replay(&handler, replay_trace, 64)
            .expect("cooperative replay run");
        assert_eq!(status, RunStatus::AllDone);
        ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: vm.effect_trace().to_vec(),
        }
    }

    #[test]
    fn native_cooperative_and_threaded_traces_match() {
        let globals = vec![ping_pong_global(), binary_choice_global()];

        for global in globals {
            let locals = project_locals(&global);
            let image = CodeImage::from_local_types(&locals, &global);
            let cooperative = run_cooperative(&image);
            let threaded = run_threaded(&image);

            let cooperative_fragment = canonical_replay_fragment_v1(
                &cooperative.obs_trace,
                &cooperative.effect_trace,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                EffectDeterminismTier::StrictDeterministic,
                CommunicationReplayMode::Off,
                None,
                Vec::new(),
            );
            let threaded_fragment = canonical_replay_fragment_v1(
                &threaded.obs_trace,
                &threaded.effect_trace,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                EffectDeterminismTier::StrictDeterministic,
                CommunicationReplayMode::Off,
                None,
                Vec::new(),
            );
            let diff = EnvelopeDiff::from_replay_fragments(
                "native_cooperative",
                "native_threaded",
                &cooperative_fragment,
                &threaded_fragment,
                1,
                1,
                1,
                EffectDeterminismTier::StrictDeterministic,
            );

            AuraEnvelopeParityPolicy::commutative_algebraic_only()
                .validate(&diff)
                .expect("envelope diff must satisfy Aura commutative policy");
            assert_eq!(cooperative_fragment, threaded_fragment);
        }
    }

    #[test]
    fn native_replay_conformance_matches_recorded_trace() {
        let globals = vec![ping_pong_global(), binary_choice_global()];

        for global in globals {
            let locals = project_locals(&global);
            let image = CodeImage::from_local_types(&locals, &global);

            let recorded = run_cooperative(&image);
            let replayed = run_cooperative_replay(&image, &recorded.effect_trace);
            let normalized_recorded_effects = normalize_handler_identity(&recorded.effect_trace);
            let normalized_replayed_effects = normalize_handler_identity(&replayed.effect_trace);

            let recorded_fragment = canonical_replay_fragment_v1(
                &recorded.obs_trace,
                &normalized_recorded_effects,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                EffectDeterminismTier::ReplayDeterministic,
                CommunicationReplayMode::Off,
                None,
                Vec::new(),
            );
            let replayed_fragment = canonical_replay_fragment_v1(
                &replayed.obs_trace,
                &normalized_replayed_effects,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                EffectDeterminismTier::ReplayDeterministic,
                CommunicationReplayMode::Off,
                None,
                Vec::new(),
            );

            assert_eq!(recorded_fragment, replayed_fragment);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{ping_pong_global, project_locals, NoOpHandler};
    use telltale_vm::loader::CodeImage;
    use telltale_vm::trace::normalize_trace;
    use telltale_vm::vm::{ObsEvent, VMConfig, VM};
    use telltale_vm::wasm::WasmVM;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn wasm_cooperative_trace_matches_native_cooperative_trace() {
        let global = ping_pong_global();
        let locals = project_locals(&global);
        let image = CodeImage::from_local_types(&locals, &global);

        let mut native_vm = VM::new(VMConfig::default());
        let native_handler = NoOpHandler;
        native_vm
            .load_choreography(&image)
            .expect("load choreography in native VM");
        native_vm
            .run(&native_handler, 64)
            .expect("run native cooperative VM");
        let native_trace = normalize_trace(native_vm.trace());

        let spec_json = serde_json::to_string(&serde_json::json!({
            "local_types": locals,
            "global_type": global,
        }))
        .expect("serialize choreography spec for wasm VM");

        let mut wasm_vm = WasmVM::new();
        wasm_vm
            .load_choreography_json(&spec_json)
            .expect("load choreography JSON in wasm VM");
        wasm_vm.run(64, 1).expect("run wasm cooperative VM");
        let wasm_trace_json = wasm_vm
            .trace_normalized_json()
            .expect("serialize wasm normalized trace");
        let wasm_trace: Vec<ObsEvent> =
            serde_json::from_str(&wasm_trace_json).expect("decode wasm normalized trace");

        assert_eq!(native_trace, wasm_trace);
    }
}
