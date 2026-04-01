//! Cross-target parity tests for Telltale protocol-machine backends used by Aura.
#![cfg(feature = "choreo-backend-telltale-machine")]
#![allow(clippy::expect_used, clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

use aura_mpst::upstream::types::{GlobalType, Label, LocalTypeR};
use cfg_if::cfg_if;
use telltale_machine::coroutine::Value;
use telltale_machine::model::effects::{EffectFailure, EffectHandler, EffectResult};

#[derive(Clone, Copy)]
struct ScenarioSpec {
    name: &'static str,
    build: fn(seed: u64) -> GlobalType,
}

const SCENARIOS: &[ScenarioSpec] = &[
    // Synthetic-fast corpus for deterministic parity regression coverage.
    ScenarioSpec {
        name: "consensus_fast_fallback",
        build: consensus_fast_fallback_global,
    },
    ScenarioSpec {
        name: "invitation_lifecycle",
        build: invitation_lifecycle_global,
    },
    ScenarioSpec {
        name: "recovery_interruption_resume",
        build: recovery_interruption_resume_global,
    },
    ScenarioSpec {
        name: "sync_anti_entropy_receipts",
        build: sync_anti_entropy_receipts_global,
    },
    ScenarioSpec {
        name: "mixed_protocol_composition",
        build: mixed_protocol_composition_global,
    },
];

const FIXED_SEEDS: &[u64] = &[1, 7, 42, 1337];

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn rotating_seed_window() -> Vec<u64> {
    let window = std::env::var("AURA_CONFORMANCE_ROTATING_WINDOW")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(0);

    if window == 0 {
        return Vec::new();
    }

    let base = std::env::var("AURA_CONFORMANCE_ROTATION_OFFSET")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .or_else(|| {
            std::env::var("GITHUB_RUN_NUMBER")
                .ok()
                .and_then(|raw| raw.parse::<u64>().ok())
        })
        .unwrap_or(0);

    (0..window)
        .map(|idx| splitmix64(base.wrapping_add(idx as u64)))
        .collect()
}

cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
        fn quint_mbt_seed_window() -> Vec<u64> {
            use aura_testkit::consensus::load_itf_trace;
            use std::path::Path;

            let path = std::env::var("AURA_CONFORMANCE_ITF_TRACE")
                .ok()
                .unwrap_or_else(|| "artifacts/traces/consensus.itf.json".to_string());
            let trace_path = Path::new(&path);
            if !trace_path.exists() {
                return Vec::new();
            }

            let cap = std::env::var("AURA_CONFORMANCE_ITF_SEED_WINDOW")
                .ok()
                .and_then(|raw| raw.parse::<usize>().ok())
                .unwrap_or(8);

            let Ok(trace) = load_itf_trace(trace_path) else {
                return Vec::new();
            };

            trace
                .states
                .iter()
                .take(cap)
                .map(|state| {
                    let material = ((state.index as u64) << 32)
                        ^ (state.instances.len() as u64)
                        ^ (state.epoch << 1)
                        ^ (trace.meta.source.len() as u64);
                    splitmix64(material)
                })
                .collect()
        }
    } else {
        fn quint_mbt_seed_window() -> Vec<u64> {
            Vec::new()
        }
    }
}

fn scenario_seed_corpus() -> Vec<u64> {
    if let Ok(raw_seed) = std::env::var("AURA_CONFORMANCE_SEED") {
        let seed = raw_seed
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("AURA_CONFORMANCE_SEED must be a u64, got: {raw_seed}"));
        return vec![seed];
    }

    let mut seeds = BTreeSet::new();
    seeds.extend(FIXED_SEEDS.iter().copied());
    seeds.extend(rotating_seed_window());
    seeds.extend(quint_mbt_seed_window());
    seeds.into_iter().collect()
}

fn selected_scenarios() -> Vec<ScenarioSpec> {
    let selected = std::env::var("AURA_CONFORMANCE_SCENARIO").ok();
    let mut scenarios: Vec<_> = SCENARIOS
        .iter()
        .copied()
        .filter(|scenario| {
            selected
                .as_deref()
                .map(|needle| needle == scenario.name)
                .unwrap_or(true)
        })
        .collect();

    scenarios.sort_by(|a, b| a.name.cmp(b.name));
    assert!(
        !scenarios.is_empty(),
        "no scenarios selected. set AURA_CONFORMANCE_SCENARIO to one of: {}",
        SCENARIOS
            .iter()
            .map(|scenario| scenario.name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    scenarios
}

fn selected_seed_corpus() -> Vec<u64> {
    let seeds = scenario_seed_corpus();
    assert!(!seeds.is_empty(), "seed corpus must not be empty");
    seeds
}

fn sync_rounds(rounds: usize) -> GlobalType {
    if rounds == 0 {
        GlobalType::End
    } else {
        GlobalType::send(
            "Primary",
            "Replica",
            Label::new(format!("delta_{rounds}")),
            GlobalType::send(
                "Replica",
                "Primary",
                Label::new(format!("receipt_{rounds}")),
                GlobalType::send(
                    "Primary",
                    "Relay",
                    Label::new(format!("propagate_{rounds}")),
                    GlobalType::send(
                        "Relay",
                        "Replica",
                        Label::new(format!("relay_receipt_{rounds}")),
                        sync_rounds(rounds - 1),
                    ),
                ),
            ),
        )
    }
}

fn consensus_fast_fallback_global(seed: u64) -> GlobalType {
    let path = if seed & 1 == 0 {
        "fast_path"
    } else {
        "fallback_path"
    };
    let witness_vote = if seed & 1 == 0 {
        "witness_fast_vote"
    } else {
        "witness_fallback_vote"
    };
    let observer_cert = if seed & 1 == 0 {
        "observer_fast_cert"
    } else {
        "observer_fallback_cert"
    };
    let commit = if seed & 1 == 0 {
        "commit_fast"
    } else {
        "commit_fallback"
    };

    GlobalType::send(
        "Proposer",
        "Witness",
        Label::new(path),
        GlobalType::send(
            "Witness",
            "Observer",
            Label::new(witness_vote),
            GlobalType::send(
                "Observer",
                "Committer",
                Label::new(observer_cert),
                GlobalType::send("Committer", "Proposer", Label::new(commit), GlobalType::End),
            ),
        ),
    )
}

fn invitation_lifecycle_global(seed: u64) -> GlobalType {
    let invite_label = if seed & 1 == 0 {
        "issue_invitation"
    } else {
        "issue_invitation_alt"
    };
    let decision = if seed & 1 == 0 { "accept" } else { "decline" };
    let commit = if seed & 1 == 0 {
        "commit_accept"
    } else {
        "commit_decline"
    };
    GlobalType::send(
        "Issuer",
        "Invitee",
        Label::new(invite_label),
        GlobalType::send(
            "Invitee",
            "Issuer",
            Label::new(decision),
            GlobalType::send("Issuer", "Context", Label::new(commit), GlobalType::End),
        ),
    )
}

fn recovery_interruption_resume_global(seed: u64) -> GlobalType {
    let start_label = if seed & 1 == 0 {
        "recover_start"
    } else {
        "recover_start_alt"
    };
    let phase = if seed & 1 == 0 {
        "continue_now"
    } else {
        "interrupt_then_resume"
    };
    let requester_followup = if seed & 1 == 0 { "finalize" } else { "resume" };
    let complete = if seed & 1 == 0 {
        "recovered"
    } else {
        "recovered_after_resume"
    };

    GlobalType::send(
        "Requester",
        "GuardianA",
        Label::new(start_label),
        GlobalType::send(
            "GuardianA",
            "GuardianB",
            Label::new("guardian_attest"),
            GlobalType::send(
                "GuardianB",
                "Requester",
                Label::new(phase),
                GlobalType::send(
                    "Requester",
                    "GuardianA",
                    Label::new(requester_followup),
                    GlobalType::send(
                        "GuardianA",
                        "Requester",
                        Label::new(complete),
                        GlobalType::End,
                    ),
                ),
            ),
        ),
    )
}

fn sync_anti_entropy_receipts_global(seed: u64) -> GlobalType {
    let rounds = ((seed % 3) as usize) + 1;
    sync_rounds(rounds)
}

fn mixed_protocol_composition_global(seed: u64) -> GlobalType {
    let suffix = if seed & 1 == 0 { "a" } else { "b" };
    GlobalType::send(
        "Inviter",
        "Invitee",
        Label::new(format!("invite_issue_{suffix}")),
        GlobalType::send(
            "SyncA",
            "SyncB",
            Label::new(format!("sync_delta_{suffix}")),
            GlobalType::send(
                "Invitee",
                "Inviter",
                Label::new(format!("invite_accept_{suffix}")),
                GlobalType::send(
                    "SyncB",
                    "SyncA",
                    Label::new(format!("sync_receipt_{suffix}")),
                    GlobalType::send(
                        "Inviter",
                        "Context",
                        Label::new(format!("invite_commit_{suffix}")),
                        GlobalType::send(
                            "SyncA",
                            "Relay",
                            Label::new(format!("sync_propagate_{suffix}")),
                            GlobalType::send(
                                "Relay",
                                "SyncB",
                                Label::new(format!("sync_ack_{suffix}")),
                                GlobalType::End,
                            ),
                        ),
                    ),
                ),
            ),
        ),
    )
}

fn project_locals(global: &GlobalType) -> BTreeMap<String, LocalTypeR> {
    aura_mpst::upstream::theory::projection::project_all(global)
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
        labels.first().cloned().map_or_else(
            || EffectResult::failure(EffectFailure::contract_violation("no labels available")),
            EffectResult::success,
        )
    }

    fn step(&self, _role: &str, _state: &mut Vec<Value>) -> EffectResult<()> {
        EffectResult::success(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_repro_command(test_name: &str, scenario: &str, seed: u64) -> String {
    format!(
        "AURA_CONFORMANCE_SCENARIO={scenario} AURA_CONFORMANCE_SEED={seed} cargo test -p aura-agent --features choreo-backend-telltale-machine --test telltale_machine_parity {test_name} -- --nocapture"
    )
}

#[cfg(target_arch = "wasm32")]
fn wasm_repro_command(test_name: &str, scenario: &str, seed: u64) -> String {
    format!(
        "AURA_CONFORMANCE_SCENARIO={scenario} AURA_CONFORMANCE_SEED={seed} CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test -p aura-agent --target wasm32-unknown-unknown --features web,choreo-backend-telltale-machine --test telltale_machine_parity {test_name} -- --nocapture"
    )
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{
        native_repro_command, project_locals, selected_scenarios, selected_seed_corpus, NoOpHandler,
    };
    use aura_agent::{
        build_vm_config, AuraChoreoEngine, AuraEnvelopeParityPolicy, AuraVmEffectHandler,
        AuraVmHardeningProfile, AuraVmParityProfile,
    };
    use aura_core::{
        assert_effect_kinds_classified, AuraConformanceArtifactV1, AuraConformanceRunMetadataV1,
        AuraConformanceSurfaceV1, ConformanceSurfaceName,
    };
    use aura_testkit::{compare_artifacts, EnvelopeLawRegistry, ReplayEffectSequence, ReplayTrace};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use telltale_machine::runtime::loader::CodeImage;
    use telltale_machine::{
        runtime::loader::CodeImage as ProtocolMachineCodeImage, CanonicalReplayFragmentV1,
        EffectDeterminismTier as ProtocolMachineEffectDeterminismTier, EffectTraceEntry,
        EnvelopeDiff as ProtocolMachineEnvelopeDiff, ObsEvent, ProtocolMachine as VM,
        RecordingEffectHandler, ReplayEffectHandler, RunStatus,
        RunStatus as ProtocolMachineRunStatus, ThreadedProtocolMachine as ThreadedVM,
        TopologyPerturbation as ProtocolMachineTopologyPerturbation,
    };

    struct ParityRun {
        obs_trace: Vec<ObsEvent>,
        effect_trace: Vec<EffectTraceEntry>,
        scheduler_steps: usize,
        canonical_fragment: CanonicalReplayFragmentV1,
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

    fn run_cooperative_with_rounds(image: &CodeImage, max_rounds: usize) -> (RunStatus, ParityRun) {
        let mut vm = VM::new(build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        ));
        let handler = NoOpHandler;
        vm.load_choreography(image).expect("load choreography");
        let status = vm.run(&handler, max_rounds).expect("cooperative run");
        let run = ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: vm.effect_trace().to_vec(),
            scheduler_steps: vm.scheduler_step_count(),
            canonical_fragment: vm.canonical_replay_fragment(),
        };
        (status, run)
    }

    fn run_cooperative(image: &CodeImage) -> ParityRun {
        let (status, run) = run_cooperative_with_rounds(image, 64);
        assert_eq!(status, RunStatus::AllDone);
        run
    }

    fn run_threaded(image: &CodeImage) -> ParityRun {
        let mut vm = ThreadedVM::with_workers(
            build_vm_config(
                AuraVmHardeningProfile::Ci,
                AuraVmParityProfile::NativeThreaded,
            ),
            2,
        );
        let handler = NoOpHandler;
        vm.load_choreography(image).expect("load choreography");
        let status = vm
            .run_concurrent(&handler, 64, 1)
            .expect("threaded run with canonical concurrency");
        assert_eq!(status, RunStatus::AllDone);
        ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: vm.effect_trace().to_vec(),
            scheduler_steps: vm.trace().len(),
            canonical_fragment: vm.canonical_replay_fragment(),
        }
    }

    fn run_cooperative_replay(
        image: &CodeImage,
        replay_trace: Arc<[EffectTraceEntry]>,
    ) -> ParityRun {
        let mut vm = VM::new(build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        ));
        let handler = NoOpHandler;
        let replay = ReplayEffectHandler::with_fallback(replay_trace, &handler);
        let recorder = RecordingEffectHandler::new(&replay);
        vm.load_choreography(image).expect("load choreography");
        let status = vm.run(&recorder, 64).expect("cooperative replay run");
        assert_eq!(status, RunStatus::AllDone);
        ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: recorder.effect_trace(),
            scheduler_steps: vm.trace().len(),
            canonical_fragment: vm.canonical_replay_fragment(),
        }
    }

    fn run_threaded_replay(image: &CodeImage, replay_trace: Arc<[EffectTraceEntry]>) -> ParityRun {
        let mut vm = ThreadedVM::with_workers(
            build_vm_config(
                AuraVmHardeningProfile::Ci,
                AuraVmParityProfile::NativeThreaded,
            ),
            2,
        );
        let handler = NoOpHandler;
        let replay = ReplayEffectHandler::with_fallback(replay_trace, &handler);
        let recorder = RecordingEffectHandler::new(&replay);
        vm.load_choreography(image).expect("load choreography");
        let status = vm
            .run_concurrent(&recorder, 64, 1)
            .expect("threaded replay run");
        assert_eq!(status, RunStatus::AllDone);
        ParityRun {
            obs_trace: vm.trace().to_vec(),
            effect_trace: recorder.effect_trace(),
            scheduler_steps: vm.trace().len(),
            canonical_fragment: vm.canonical_replay_fragment(),
        }
    }

    fn record_replay_trace(image: &CodeImage) -> Vec<EffectTraceEntry> {
        let mut vm = VM::new(build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        ));
        let handler = NoOpHandler;
        let recorder = RecordingEffectHandler::new(&handler);
        vm.load_choreography(image).expect("load choreography");
        let status = vm.run(&recorder, 64).expect("record replay trace run");
        assert_eq!(status, RunStatus::AllDone);
        recorder.effect_trace()
    }

    fn normalized_fragment_without_handler_identity(
        fragment: &CanonicalReplayFragmentV1,
    ) -> CanonicalReplayFragmentV1 {
        let mut normalized = fragment.clone();
        for entry in &mut normalized.effect_trace {
            entry.handler_identity.clear();
        }
        normalized
    }

    fn replay_relevant_effect_trace(trace: &[EffectTraceEntry]) -> Vec<EffectTraceEntry> {
        normalize_handler_identity(trace)
            .into_iter()
            .filter(|entry| entry.effect_kind != "topology_events")
            .collect()
    }

    fn replay_relevant_fragment(fragment: &CanonicalReplayFragmentV1) -> CanonicalReplayFragmentV1 {
        let mut normalized = normalized_fragment_without_handler_identity(fragment);
        normalized
            .effect_trace
            .retain(|entry| entry.effect_kind != "topology_events");
        normalized
    }

    fn replay_relevant_run(run: &ParityRun) -> ParityRun {
        ParityRun {
            obs_trace: run.obs_trace.clone(),
            effect_trace: replay_relevant_effect_trace(&run.effect_trace),
            scheduler_steps: run.scheduler_steps,
            canonical_fragment: replay_relevant_fragment(&run.canonical_fragment),
        }
    }

    fn protocol_machine_fragment(
        fragment: &CanonicalReplayFragmentV1,
    ) -> CanonicalReplayFragmentV1 {
        fragment.clone()
    }

    fn protocol_machine_image(image: &CodeImage) -> ProtocolMachineCodeImage {
        let image =
            ProtocolMachineCodeImage::from_local_types(&image.local_types, &image.global_type);
        image.validate_runtime_shape().expect("runtime image");
        image
    }

    fn write_replay_lane_metrics(
        scenario: &str,
        seed: u64,
        shared_trace_entries: usize,
        shared_trace_estimated_bytes: usize,
        recorded: &ParityRun,
        replay_coop: &ParityRun,
        replay_threaded: &ParityRun,
    ) {
        let Some(root) = conformance_artifact_root() else {
            return;
        };
        let path = root.join("replay_lane_metrics.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create replay metrics directory");
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open replay metrics artifact");
        let row = serde_json::json!({
            "scenario": scenario,
            "seed": seed,
            "shared_trace_entries": shared_trace_entries,
            "shared_trace_estimated_bytes": shared_trace_estimated_bytes,
            "recorded_scheduler_steps": recorded.scheduler_steps,
            "replay_coop_scheduler_steps": replay_coop.scheduler_steps,
            "replay_threaded_scheduler_steps": replay_threaded.scheduler_steps,
            "recorded_effect_entries": recorded.effect_trace.len(),
            "replay_coop_effect_entries": replay_coop.effect_trace.len(),
            "replay_threaded_effect_entries": replay_threaded.effect_trace.len(),
        });
        use std::io::Write as _;
        writeln!(file, "{row}").expect("write replay metrics row");
    }

    fn fault_signature(status: RunStatus, run: &ParityRun) -> String {
        let effect_kinds = run
            .effect_trace
            .iter()
            .map(|entry| entry.effect_kind.as_str())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "status={status:?};obs={};effects={};kinds={effect_kinds}",
            run.obs_trace.len(),
            run.effect_trace.len()
        )
    }

    fn build_conformance_artifact(
        profile: &str,
        scenario: &str,
        seed: u64,
        run: &ParityRun,
    ) -> AuraConformanceArtifactV1 {
        assert_effect_kinds_classified(
            run.effect_trace
                .iter()
                .map(|entry| entry.effect_kind.as_str()),
        )
        .expect("all effect envelope kinds in parity traces must be explicitly classified");

        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: profile.to_string(),
            scenario: scenario.to_string(),
            seed: Some(seed),
            commit: option_env!("GIT_COMMIT_HASH").map(ToString::to_string),
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
            vm_determinism_profile: None,
        });

        let observable_entries = run
            .obs_trace
            .iter()
            .map(|event| serde_json::to_value(event).expect("serialize observable event"))
            .collect();
        artifact.insert_surface(
            ConformanceSurfaceName::Observable,
            AuraConformanceSurfaceV1::new(observable_entries, None),
        );

        let scheduler_entries = run
            .obs_trace
            .iter()
            .map(|event| serde_json::to_value(event).expect("serialize scheduler event"))
            .collect();
        artifact.insert_surface(
            ConformanceSurfaceName::SchedulerStep,
            AuraConformanceSurfaceV1::new(scheduler_entries, None),
        );

        let normalized_effects = normalize_handler_identity(&run.effect_trace);
        let effect_entries = normalized_effects
            .iter()
            .map(|entry| serde_json::to_value(entry).expect("serialize effect trace entry"))
            .collect();
        artifact.insert_surface(
            ConformanceSurfaceName::Effect,
            AuraConformanceSurfaceV1::new(effect_entries, None),
        );

        artifact
            .validate_required_surfaces()
            .expect("conformance artifacts must include observable/scheduler_step/effect surfaces");
        artifact
            .recompute_digests()
            .expect("recompute conformance digests");
        artifact
    }

    fn conformance_artifact_root() -> Option<PathBuf> {
        if std::env::var("AURA_CONFORMANCE_WRITE_ARTIFACTS")
            .ok()
            .as_deref()
            != Some("1")
        {
            return None;
        }

        Some(
            std::env::var("AURA_CONFORMANCE_ARTIFACT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| Path::new("artifacts").join("conformance")),
        )
    }

    fn write_artifact(
        artifact: &AuraConformanceArtifactV1,
        lane: &str,
        scenario: &str,
        seed: u64,
    ) -> Option<PathBuf> {
        let root = conformance_artifact_root()?;
        let path = root
            .join(lane)
            .join(format!("{scenario}__seed_{seed}.json"));
        let parent = path.parent().expect("artifact parent");
        fs::create_dir_all(parent).expect("create conformance artifact directory");
        let payload = artifact
            .canonical_json()
            .expect("serialize conformance artifact");
        fs::write(&path, payload).expect("write conformance artifact");
        Some(path)
    }

    fn assert_equivalent(
        baseline: &AuraConformanceArtifactV1,
        candidate: &AuraConformanceArtifactV1,
        test_name: &str,
    ) {
        let registry = EnvelopeLawRegistry::from_aura_registry();
        let report = compare_artifacts(baseline, candidate, &registry);
        assert!(
            report.equivalent,
            "conformance mismatch (surface={:?}, step={:?}, law={:?}, detail={})\nrepro: {}",
            report
                .first_mismatch
                .as_ref()
                .map(|m| m.surface)
                .unwrap_or(ConformanceSurfaceName::Observable),
            report.first_mismatch.as_ref().and_then(|m| m.step_index),
            report.first_mismatch.as_ref().and_then(|m| m.law),
            report
                .first_mismatch
                .as_ref()
                .map(|m| m.detail.as_str())
                .unwrap_or("n/a"),
            native_repro_command(
                test_name,
                &baseline.metadata.scenario,
                baseline.metadata.seed.unwrap_or_default()
            )
        );
    }

    #[test]
    fn native_cooperative_and_threaded_traces_match_for_seed_corpus() {
        for scenario in selected_scenarios() {
            for seed in selected_seed_corpus() {
                let global = (scenario.build)(seed);
                let locals = project_locals(&global);
                let image = CodeImage::from_local_types(&locals, &global);

                let cooperative = run_cooperative(&image);
                let threaded = run_threaded(&image);

                let cooperative_artifact =
                    build_conformance_artifact("native_coop", scenario.name, seed, &cooperative);
                let threaded_artifact =
                    build_conformance_artifact("native_threaded", scenario.name, seed, &threaded);

                let _ = write_artifact(&cooperative_artifact, "native_coop", scenario.name, seed);
                let _ = write_artifact(&threaded_artifact, "native_threaded", scenario.name, seed);

                let cooperative_fragment =
                    normalized_fragment_without_handler_identity(&cooperative.canonical_fragment);
                let threaded_fragment =
                    normalized_fragment_without_handler_identity(&threaded.canonical_fragment);
                let cooperative_fragment = protocol_machine_fragment(&cooperative_fragment);
                let threaded_fragment = protocol_machine_fragment(&threaded_fragment);
                let diff = ProtocolMachineEnvelopeDiff::from_replay_fragments(
                    "native_cooperative",
                    "native_threaded",
                    &cooperative_fragment,
                    &threaded_fragment,
                    1,
                    1,
                    1,
                    ProtocolMachineEffectDeterminismTier::StrictDeterministic,
                );

                AuraEnvelopeParityPolicy::commutative_algebraic_only()
                    .validate(&diff)
                    .unwrap_or_else(|err| {
                        panic!(
                            "envelope diff violated parity policy for scenario={} seed={} ({err})\nrepro: {}",
                            scenario.name,
                            seed,
                            native_repro_command(
                                "native_cooperative_and_threaded_traces_match_for_seed_corpus",
                                scenario.name,
                                seed
                            )
                        )
                    });

                assert_equivalent(
                    &cooperative_artifact,
                    &threaded_artifact,
                    "native_cooperative_and_threaded_traces_match_for_seed_corpus",
                );
            }
        }
    }

    #[test]
    fn native_replay_conformance_matches_recorded_trace_for_seed_corpus() {
        for scenario in selected_scenarios() {
            for seed in selected_seed_corpus() {
                let global = (scenario.build)(seed);
                let locals = project_locals(&global);
                let image = CodeImage::from_local_types(&locals, &global);

                let recorded = run_cooperative(&image);
                let recorded_replay_trace = record_replay_trace(&image);
                let mut recorded_replay = replay_relevant_run(&recorded);
                let recorded_replay_effect_trace =
                    replay_relevant_effect_trace(&recorded_replay_trace);
                recorded_replay.effect_trace = recorded_replay_effect_trace.clone();
                recorded_replay.canonical_fragment.effect_trace = recorded_replay_effect_trace;
                let shared_replay = Arc::<[EffectTraceEntry]>::from(recorded_replay_trace.clone());
                let replayed = run_cooperative_replay(&image, Arc::clone(&shared_replay));
                let replayed_threaded = run_threaded_replay(&image, shared_replay);
                let replayed_replay = replay_relevant_run(&replayed);
                let replayed_threaded_replay = replay_relevant_run(&replayed_threaded);

                let shared_trace_entries = recorded.effect_trace.len();
                let shared_trace_estimated_bytes =
                    shared_trace_entries * std::mem::size_of::<EffectTraceEntry>();
                write_replay_lane_metrics(
                    scenario.name,
                    seed,
                    shared_trace_entries,
                    shared_trace_estimated_bytes,
                    &recorded,
                    &replayed,
                    &replayed_threaded,
                );

                let recorded_artifact = build_conformance_artifact(
                    "native_coop",
                    scenario.name,
                    seed,
                    &recorded_replay,
                );
                let replayed_artifact = build_conformance_artifact(
                    "native_coop_replay",
                    scenario.name,
                    seed,
                    &replayed_replay,
                );
                let replayed_threaded_artifact = build_conformance_artifact(
                    "native_threaded_replay",
                    scenario.name,
                    seed,
                    &replayed_threaded_replay,
                );

                let _ = write_artifact(&recorded_artifact, "native_replay", scenario.name, seed);
                let _ = write_artifact(&replayed_artifact, "native_replay", scenario.name, seed);
                let _ = write_artifact(
                    &replayed_threaded_artifact,
                    "native_replay",
                    scenario.name,
                    seed,
                );

                let normalized_recorded_effects =
                    replay_relevant_effect_trace(&recorded_replay_trace);
                let normalized_replayed_effects = replayed_replay.effect_trace.clone();
                let normalized_replayed_threaded_effects =
                    replayed_threaded_replay.effect_trace.clone();

                let expected_replay_trace = ReplayTrace::from_entries(normalized_recorded_effects);
                let mut replay_verifier = ReplayEffectSequence::new(&expected_replay_trace);
                for entry in &normalized_replayed_effects {
                    replay_verifier.verify_next(entry).unwrap_or_else(|err| {
                        panic!(
                            "cooperative replay trace consumption mismatch for scenario={} seed={} ({err})\nrepro: {}",
                            scenario.name,
                            seed,
                            native_repro_command(
                                "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                                scenario.name,
                                seed
                            )
                        )
                    });
                }
                replay_verifier.finish().unwrap_or_else(|err| {
                    panic!(
                        "cooperative replay trace did not fully consume expected entries for scenario={} seed={} ({err})\nrepro: {}",
                        scenario.name,
                        seed,
                        native_repro_command(
                            "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                            scenario.name,
                            seed
                        )
                    )
                });

                let mut threaded_replay_verifier =
                    ReplayEffectSequence::new(&expected_replay_trace);
                for entry in &normalized_replayed_threaded_effects {
                    threaded_replay_verifier
                        .verify_next(entry)
                        .unwrap_or_else(|err| {
                            panic!(
                                "threaded replay trace consumption mismatch for scenario={} seed={} ({err})\nrepro: {}",
                                scenario.name,
                                seed,
                                native_repro_command(
                                    "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                                    scenario.name,
                                    seed
                                )
                            )
                        });
                }
                threaded_replay_verifier.finish().unwrap_or_else(|err| {
                    panic!(
                        "threaded replay trace did not fully consume expected entries for scenario={} seed={} ({err})\nrepro: {}",
                        scenario.name,
                        seed,
                        native_repro_command(
                            "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                            scenario.name,
                            seed
                        )
                    )
                });

                let recorded_fragment =
                    protocol_machine_fragment(&recorded_replay.canonical_fragment);
                let replayed_fragment =
                    protocol_machine_fragment(&replayed_replay.canonical_fragment);
                let replayed_threaded_fragment =
                    protocol_machine_fragment(&replayed_threaded_replay.canonical_fragment);

                let replay_diff = ProtocolMachineEnvelopeDiff::from_replay_fragments(
                    "native_recorded",
                    "native_cooperative_replay",
                    &recorded_fragment,
                    &replayed_fragment,
                    1,
                    1,
                    1,
                    ProtocolMachineEffectDeterminismTier::StrictDeterministic,
                );
                AuraEnvelopeParityPolicy::commutative_algebraic_only()
                    .validate(&replay_diff)
                    .unwrap_or_else(|err| {
                        panic!(
                            "recorded/cooperative replay fragment diff violated parity policy for scenario={} seed={} ({err})\nrepro: {}",
                            scenario.name,
                            seed,
                            native_repro_command(
                                "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                                scenario.name,
                                seed
                            )
                        )
                    });

                let replay_threaded_diff = ProtocolMachineEnvelopeDiff::from_replay_fragments(
                    "native_recorded",
                    "native_threaded_replay",
                    &recorded_fragment,
                    &replayed_threaded_fragment,
                    1,
                    1,
                    1,
                    ProtocolMachineEffectDeterminismTier::StrictDeterministic,
                );
                AuraEnvelopeParityPolicy::commutative_algebraic_only()
                    .validate(&replay_threaded_diff)
                    .unwrap_or_else(|err| {
                        panic!(
                            "recorded/threaded replay fragment diff violated parity policy for scenario={} seed={} ({err})\nrepro: {}",
                            scenario.name,
                            seed,
                            native_repro_command(
                                "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                                scenario.name,
                                seed
                            )
                        )
                    });

                assert_equivalent(
                    &recorded_artifact,
                    &replayed_artifact,
                    "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                );
                assert_equivalent(
                    &recorded_artifact,
                    &replayed_threaded_artifact,
                    "native_replay_conformance_matches_recorded_trace_for_seed_corpus",
                );
            }
        }
    }

    #[test]
    fn mutation_corpus_produces_stable_divergence_classification() {
        let scenario = selected_scenarios()
            .into_iter()
            .next()
            .expect("at least one selected scenario");
        let seed = selected_seed_corpus()
            .into_iter()
            .next()
            .expect("at least one selected seed");

        let global = (scenario.build)(seed);
        let locals = project_locals(&global);
        let image = CodeImage::from_local_types(&locals, &global);
        let run = run_cooperative(&image);
        let baseline = build_conformance_artifact("native_coop", scenario.name, seed, &run);
        let registry = EnvelopeLawRegistry::from_aura_registry();

        let mut mutated_observable = baseline.clone();
        mutated_observable
            .surfaces
            .get_mut(&ConformanceSurfaceName::Observable)
            .expect("observable surface")
            .entries
            .pop();
        let mismatch = compare_artifacts(&baseline, &mutated_observable, &registry)
            .first_mismatch
            .expect("mutation should diverge on observable surface");
        assert_eq!(mismatch.surface, ConformanceSurfaceName::Observable);

        let mut mutated_strict_effect = baseline.clone();
        mutated_strict_effect
            .surfaces
            .get_mut(&ConformanceSurfaceName::Effect)
            .expect("effect surface")
            .entries
            .push(serde_json::json!({
                "effect_kind": "handle_recv",
                "role": "A",
                "partner": "B",
                "label": "mutated"
            }));
        let mismatch = compare_artifacts(&baseline, &mutated_strict_effect, &registry)
            .first_mismatch
            .expect("mutation should diverge on strict effect law");
        assert_eq!(mismatch.surface, ConformanceSurfaceName::Effect);
        assert_eq!(mismatch.law, Some(aura_core::AuraEnvelopeLawClass::Strict));

        let mut mutated_unclassified_effect = baseline.clone();
        mutated_unclassified_effect
            .surfaces
            .get_mut(&ConformanceSurfaceName::Effect)
            .expect("effect surface")
            .entries
            .push(serde_json::json!({"effect_kind": "unknown_effect_kind"}));
        let mismatch = compare_artifacts(&baseline, &mutated_unclassified_effect, &registry)
            .first_mismatch
            .expect("mutation should diverge on unclassified effect kind");
        assert_eq!(mismatch.surface, ConformanceSurfaceName::Effect);
        assert!(mismatch.detail.contains("unclassified effect_kind"));

        let repro = native_repro_command(
            "mutation_corpus_produces_stable_divergence_classification",
            scenario.name,
            seed,
        );
        eprintln!("mutation corpus classification completed; repro: {repro}");
    }

    #[test]
    fn quint_mbt_seed_window_is_deterministic() {
        let first = super::quint_mbt_seed_window();
        let second = super::quint_mbt_seed_window();
        assert_eq!(
            first, second,
            "Quint-derived seed corpus must be deterministic across repeated loads"
        );
    }

    #[test]
    fn deterministic_fault_taxonomy_signature_is_stable() {
        let scenario = selected_scenarios()
            .into_iter()
            .next()
            .expect("at least one selected scenario");
        let seed = selected_seed_corpus()
            .into_iter()
            .next()
            .expect("at least one selected seed");

        let global = (scenario.build)(seed);
        let locals = project_locals(&global);
        let image = CodeImage::from_local_types(&locals, &global);

        let (status_a, run_a) = run_cooperative_with_rounds(&image, 0);
        let (status_b, run_b) = run_cooperative_with_rounds(&image, 0);
        let sig_a = fault_signature(status_a, &run_a);
        let sig_b = fault_signature(status_b, &run_b);

        assert_eq!(
            status_a,
            status_b,
            "fault status must be stable for same scenario/seed input\\nrepro: {}",
            native_repro_command(
                "deterministic_fault_taxonomy_signature_is_stable",
                scenario.name,
                seed
            )
        );
        assert_eq!(
            sig_a,
            sig_b,
            "fault signature must be stable for same scenario/seed input\\nrepro: {}",
            native_repro_command(
                "deterministic_fault_taxonomy_signature_is_stable",
                scenario.name,
                seed
            )
        );
    }

    #[test]
    fn native_topology_events_are_captured_for_parity_lanes() {
        let scenario = selected_scenarios()
            .into_iter()
            .next()
            .expect("at least one selected scenario");
        let seed = selected_seed_corpus()
            .into_iter()
            .next()
            .expect("at least one selected seed");

        let global = (scenario.build)(seed);
        let locals = project_locals(&global);
        let image = protocol_machine_image(&CodeImage::from_local_types(&locals, &global));

        let handler = Arc::new(AuraVmEffectHandler::default());
        let mut engine = AuraChoreoEngine::new(
            build_vm_config(
                AuraVmHardeningProfile::Ci,
                AuraVmParityProfile::NativeCooperative,
            ),
            Arc::clone(&handler),
        );
        let crash = ProtocolMachineTopologyPerturbation::Crash {
            site: "fault-node-a".to_string(),
        };
        let partition = ProtocolMachineTopologyPerturbation::Partition {
            from: "fault-node-a".to_string(),
            to: "fault-node-b".to_string(),
        };
        for tick in 0..=8 {
            handler.schedule_topology_event(tick, partition.clone());
            handler.schedule_topology_event(tick, crash.clone());
        }

        engine
            .open_protocol_machine_session(&image)
            .expect("load choreography");
        let status = engine.run(64).expect("run choreography");
        assert_eq!(status, ProtocolMachineRunStatus::AllDone);

        let topology_events = engine
            .vm_effect_trace()
            .iter()
            .filter_map(|entry| entry.topology.clone())
            .collect::<Vec<_>>();
        assert!(
            !topology_events.is_empty(),
            "expected at least one topology_event capture in effect trace\nrepro: {}",
            native_repro_command(
                "native_topology_events_are_captured_for_parity_lanes",
                scenario.name,
                seed
            )
        );
        assert!(
            topology_events.iter().any(|event| event == &crash),
            "expected crash topology event in effect trace\nrepro: {}",
            native_repro_command(
                "native_topology_events_are_captured_for_parity_lanes",
                scenario.name,
                seed
            )
        );
        assert!(
            topology_events.iter().any(|event| event == &partition),
            "expected partition topology event in effect trace\nrepro: {}",
            native_repro_command(
                "native_topology_events_are_captured_for_parity_lanes",
                scenario.name,
                seed
            )
        );
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{
        project_locals, selected_scenarios, selected_seed_corpus, wasm_repro_command, NoOpHandler,
    };
    use aura_agent::{
        build_vm_config, AuraEnvelopeParityPolicy, AuraVmHardeningProfile, AuraVmParityProfile,
    };
    use aura_core::{
        AuraConformanceArtifactV1, AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1,
        ConformanceSurfaceName,
    };
    use aura_testkit::{compare_artifacts, EnvelopeLawRegistry};
    use telltale_machine::runtime::loader::CodeImage;
    use telltale_machine::{
        canonical_replay_fragment_v1, normalize_trace, CommunicationReplayMode,
        EffectDeterminismTier, ObsEvent, ProtocolMachine as VM, WasmProtocolMachine as WasmVM,
    };
    use telltale_machine::{
        serialization::CanonicalReplayFragmentV1 as ProtocolMachineCanonicalReplayFragmentV1,
        EffectDeterminismTier as ProtocolMachineEffectDeterminismTier,
        EnvelopeDiff as ProtocolMachineEnvelopeDiff,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    fn protocol_machine_fragment(
        fragment: &CanonicalReplayFragmentV1,
    ) -> ProtocolMachineCanonicalReplayFragmentV1 {
        let mut payload = serde_json::to_value(fragment).expect("serialize VM replay fragment");
        payload["schema_version"] =
            serde_json::Value::String("machine.serialization.v1".to_string());
        serde_json::from_value(payload)
            .expect("transcode VM replay fragment to protocol-machine fragment")
    }

    fn build_wasm_artifact(
        profile: &str,
        scenario: &str,
        seed: u64,
        trace: &[ObsEvent],
    ) -> AuraConformanceArtifactV1 {
        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: "wasm".to_string(),
            profile: profile.to_string(),
            scenario: scenario.to_string(),
            seed: Some(seed),
            commit: option_env!("GIT_COMMIT_HASH").map(ToString::to_string),
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
            vm_determinism_profile: None,
        });

        let entries = trace
            .iter()
            .map(|event| serde_json::to_value(event).expect("serialize trace event"))
            .collect::<Vec<_>>();
        artifact.insert_surface(
            ConformanceSurfaceName::Observable,
            AuraConformanceSurfaceV1::new(entries.clone(), None),
        );
        artifact.insert_surface(
            ConformanceSurfaceName::SchedulerStep,
            AuraConformanceSurfaceV1::new(entries.clone(), None),
        );
        artifact.insert_surface(
            ConformanceSurfaceName::Effect,
            AuraConformanceSurfaceV1::new(Vec::new(), None),
        );
        artifact
            .recompute_digests()
            .expect("recompute wasm digests");
        artifact
    }

    #[wasm_bindgen_test]
    fn wasm_cooperative_trace_matches_native_cooperative_trace_for_seed_corpus() {
        for scenario in selected_scenarios() {
            for seed in selected_seed_corpus() {
                let global = (scenario.build)(seed);
                let locals = project_locals(&global);
                let image = CodeImage::from_local_types(&locals, &global);

                let mut native_vm = VM::new(build_vm_config(
                    AuraVmHardeningProfile::Ci,
                    AuraVmParityProfile::NativeCooperative,
                ));
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

                assert_eq!(
                    native_trace,
                    wasm_trace,
                    "native/wasm normalized traces diverged for scenario={} seed={}\nrepro: {}",
                    scenario.name,
                    seed,
                    wasm_repro_command(
                        "wasm_cooperative_trace_matches_native_cooperative_trace_for_seed_corpus",
                        scenario.name,
                        seed
                    )
                );

                let mut native_fragment = native_vm.canonical_replay_fragment();
                native_fragment.effect_trace.clear();
                let wasm_fragment = canonical_replay_fragment_v1(
                    &wasm_trace,
                    &[],
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    EffectDeterminismTier::StrictDeterministic,
                    CommunicationReplayMode::Off,
                    None,
                    Vec::new(),
                );
                let native_fragment = protocol_machine_fragment(&native_fragment);
                let wasm_fragment = protocol_machine_fragment(&wasm_fragment);
                let diff = ProtocolMachineEnvelopeDiff::from_replay_fragments(
                    "native_cooperative",
                    "wasm_cooperative",
                    &native_fragment,
                    &wasm_fragment,
                    1,
                    1,
                    1,
                    ProtocolMachineEffectDeterminismTier::StrictDeterministic,
                );
                AuraEnvelopeParityPolicy::commutative_algebraic_only()
                    .validate(&diff)
                    .unwrap_or_else(|error| {
                        panic!(
                            "native/wasm envelope diff violated parity policy for scenario={} seed={} ({error})",
                            scenario.name, seed
                        )
                    });

                let native_artifact =
                    build_wasm_artifact("native_coop", scenario.name, seed, &native_trace);
                let wasm_artifact =
                    build_wasm_artifact("wasm_coop", scenario.name, seed, &wasm_trace);
                let report = compare_artifacts(
                    &native_artifact,
                    &wasm_artifact,
                    &EnvelopeLawRegistry::from_aura_registry(),
                );
                assert!(
                    report.equivalent,
                    "native/wasm conformance mismatch (surface={:?}, step={:?}, law={:?}, detail={})\nrepro: {}",
                    report.first_mismatch.as_ref().map(|m| m.surface),
                    report.first_mismatch.as_ref().and_then(|m| m.step_index),
                    report.first_mismatch.as_ref().and_then(|m| m.law),
                    report
                        .first_mismatch
                        .as_ref()
                        .map(|m| m.detail.as_str())
                        .unwrap_or("n/a"),
                    wasm_repro_command(
                        "wasm_cooperative_trace_matches_native_cooperative_trace_for_seed_corpus",
                        scenario.name,
                        seed
                    )
                );
            }
        }
    }
}
