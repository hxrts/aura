//! CI scenario-contract gate for Telltale protocol-machine execution profiles.
#![cfg(feature = "choreo-backend-telltale-machine")]
#![allow(clippy::expect_used, clippy::disallowed_methods)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use aura_agent::{
    build_vm_config, AuraChoreoEngine, AuraVmEffectHandler, AuraVmHardeningProfile,
    AuraVmParityProfile,
};
use aura_mpst::upstream::types::{GlobalType, Label, LocalTypeR};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use telltale_machine::runtime::loader::CodeImage;
use telltale_machine::{
    coroutine::Value,
    model::effects::{EffectFailure, EffectHandler, EffectResult},
    runtime::loader::CodeImage as ProtocolMachineCodeImage,
    ObsEvent, ProtocolMachine as VM, RunStatus, RunStatus as ProtocolMachineRunStatus,
    TopologyPerturbation as ProtocolMachineTopologyPerturbation,
};

#[derive(Clone, Copy)]
struct ContractBundleSpec {
    id: &'static str,
    build: fn(u64) -> GlobalType,
    required_labels: &'static [&'static str],
    min_observable_events: usize,
}

const CONTRACT_BUNDLES: &[ContractBundleSpec] = &[
    // Synthetic-fast bundle contracts. A separate realistic lane below runs
    // a real Aura choreography source with AuraVmEffectHandler.
    ContractBundleSpec {
        id: "consensus",
        build: consensus_fast_fallback_global,
        required_labels: &["witness_vote", "observer_cert", "commit"],
        min_observable_events: 8,
    },
    ContractBundleSpec {
        id: "sync",
        build: sync_anti_entropy_global,
        required_labels: &["delta", "receipt", "propagate", "relay_receipt"],
        min_observable_events: 10,
    },
    ContractBundleSpec {
        id: "recovery",
        build: recovery_resume_global,
        required_labels: &["recover_start", "guardian_attest", "resume", "recovered"],
        min_observable_events: 10,
    },
    ContractBundleSpec {
        id: "reconfiguration",
        build: reconfiguration_delegate_global,
        required_labels: &["link_bundle", "delegate_session", "verify_coherence"],
        min_observable_events: 8,
    },
    ContractBundleSpec {
        id: "reconfiguration_device_migration",
        build: reconfiguration_device_migration_global,
        required_labels: &[
            "link_bundle",
            "delegate_session",
            "migrate_device",
            "verify_coherence",
            "commit_tree",
        ],
        min_observable_events: 12,
    },
    ContractBundleSpec {
        id: "reconfiguration_guardian_handoff_faults",
        build: reconfiguration_guardian_handoff_faults_global,
        required_labels: &[
            "link_bundle",
            "inject_partition",
            "inject_crash",
            "inject_delay",
            "recover_session",
            "delegate_handoff",
            "verify_coherence",
        ],
        min_observable_events: 12,
    },
    ContractBundleSpec {
        id: "reconfiguration_relay_delegation",
        build: reconfiguration_relay_delegation_global,
        required_labels: &[
            "link_bundle",
            "delegate_relay",
            "relay_churn",
            "verify_coherence",
        ],
        min_observable_events: 10,
    },
    ContractBundleSpec {
        id: "strict_fail_closed_runtime",
        build: strict_fail_closed_runtime_global,
        required_labels: &[
            "issue_receipt",
            "reject_forged_receipt",
            "issue_owner_capability",
            "reject_forged_authority",
            "issue_timeout_witness",
            "expire_timeout_witness",
            "commit_rejection",
        ],
        min_observable_events: 12,
    },
];

const CONTRACT_SEEDS: &[u64] = &[1, 7, 42];

fn project_locals(global: &GlobalType) -> std::collections::BTreeMap<String, LocalTypeR> {
    aura_mpst::upstream::theory::projection::project_all(global)
        .expect("project global choreography to local session types")
        .into_iter()
        .collect()
}

fn strip_aura_annotations_for_parser(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    #[allow(clippy::while_let_on_iterator)]
    while let Some(ch) = chars.next() {
        let (open, close) = match ch {
            '[' => ('[', ']'),
            '{' => ('{', '}'),
            _ => {
                out.push(ch);
                continue;
            }
        };

        let mut depth = 1usize;
        let mut buf = String::new();
        let mut has_equals = false;

        while let Some(next) = chars.next() {
            if next == open {
                depth += 1;
            } else if next == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            if next == '=' {
                has_equals = true;
            }
            buf.push(next);
        }

        if !has_equals {
            out.push(open);
            out.push_str(&buf);
            out.push(close);
        }
    }

    out
}

fn invitation_exchange_global_from_source() -> GlobalType {
    let source = strip_aura_annotations_for_parser(include_str!(
        "../../aura-invitation/src/protocol.invitation_exchange.tell"
    ));
    let choreography = aura_mpst::upstream::language::parse_choreography_str(&source)
        .expect("parse invitation choreography source");
    let current_global = aura_mpst::upstream::language::ast::choreography_to_global(&choreography)
        .expect("convert invitation choreography to global type");
    serde_json::from_value(
        serde_json::to_value(&current_global).expect("serialize current global type"),
    )
    .expect("transcode current global type to VM-aligned global type")
}

fn protocol_machine_image(
    global: &GlobalType,
    locals: &BTreeMap<String, LocalTypeR>,
) -> ProtocolMachineCodeImage {
    let image = ProtocolMachineCodeImage::from_local_types(locals, global);
    image
        .validate_runtime_shape()
        .expect("validate protocol-machine image");
    image
}

fn consensus_fast_fallback_global(_seed: u64) -> GlobalType {
    GlobalType::send(
        "Proposer",
        "Witness",
        Label::new("proposal"),
        GlobalType::send(
            "Witness",
            "Observer",
            Label::new("witness_vote"),
            GlobalType::send(
                "Observer",
                "Committer",
                Label::new("observer_cert"),
                GlobalType::send(
                    "Committer",
                    "Proposer",
                    Label::new("commit"),
                    GlobalType::End,
                ),
            ),
        ),
    )
}

fn sync_anti_entropy_global(seed: u64) -> GlobalType {
    let round_count = (seed as usize % 2) + 1;
    fn build_rounds(n: usize) -> GlobalType {
        if n == 0 {
            return GlobalType::End;
        }
        GlobalType::send(
            "Primary",
            "Replica",
            Label::new("delta"),
            GlobalType::send(
                "Replica",
                "Primary",
                Label::new("receipt"),
                GlobalType::send(
                    "Primary",
                    "Relay",
                    Label::new("propagate"),
                    GlobalType::send(
                        "Relay",
                        "Replica",
                        Label::new("relay_receipt"),
                        build_rounds(n - 1),
                    ),
                ),
            ),
        )
    }
    build_rounds(round_count)
}

fn recovery_resume_global(_seed: u64) -> GlobalType {
    GlobalType::send(
        "Requester",
        "GuardianA",
        Label::new("recover_start"),
        GlobalType::send(
            "GuardianA",
            "GuardianB",
            Label::new("guardian_attest"),
            GlobalType::send(
                "GuardianB",
                "Requester",
                Label::new("resume"),
                GlobalType::send(
                    "Requester",
                    "GuardianA",
                    Label::new("finalize"),
                    GlobalType::send(
                        "GuardianA",
                        "Requester",
                        Label::new("recovered"),
                        GlobalType::End,
                    ),
                ),
            ),
        ),
    )
}

fn reconfiguration_delegate_global(_seed: u64) -> GlobalType {
    GlobalType::send(
        "Controller",
        "BundleA",
        Label::new("link_bundle"),
        GlobalType::send(
            "Controller",
            "BundleB",
            Label::new("delegate_session"),
            GlobalType::send(
                "BundleB",
                "Controller",
                Label::new("verify_coherence"),
                GlobalType::End,
            ),
        ),
    )
}

fn reconfiguration_device_migration_global(seed: u64) -> GlobalType {
    let rounds = (seed as usize % 3) + 2;

    fn device_round(n: usize) -> GlobalType {
        if n == 0 {
            return GlobalType::End;
        }
        GlobalType::send(
            "Controller",
            "OldDevice",
            Label::new("delegate_session"),
            GlobalType::send(
                "Controller",
                "NewDevice",
                Label::new("migrate_device"),
                GlobalType::send(
                    "NewDevice",
                    "Controller",
                    Label::new("verify_coherence"),
                    GlobalType::send(
                        "Controller",
                        "Tree",
                        Label::new("commit_tree"),
                        device_round(n - 1),
                    ),
                ),
            ),
        )
    }

    GlobalType::send(
        "Controller",
        "BundleA",
        Label::new("link_bundle"),
        device_round(rounds),
    )
}

fn reconfiguration_guardian_handoff_faults_global(seed: u64) -> GlobalType {
    let retries = (seed as usize % 2) + 1;

    fn retries_with_faults(n: usize) -> GlobalType {
        if n == 0 {
            return GlobalType::send(
                "Controller",
                "GuardianOld",
                Label::new("delegate_handoff"),
                GlobalType::send(
                    "GuardianNew",
                    "Controller",
                    Label::new("verify_coherence"),
                    GlobalType::End,
                ),
            );
        }

        GlobalType::send(
            "FaultInjector",
            "Controller",
            Label::new("inject_partition"),
            GlobalType::send(
                "FaultInjector",
                "Controller",
                Label::new("inject_crash"),
                retries_with_faults(n - 1),
            ),
        )
    }

    GlobalType::send(
        "Controller",
        "BundleA",
        Label::new("link_bundle"),
        GlobalType::send(
            "FaultInjector",
            "Controller",
            Label::new("inject_delay"),
            GlobalType::send(
                "Controller",
                "GuardianOld",
                Label::new("recover_session"),
                retries_with_faults(retries),
            ),
        ),
    )
}

fn reconfiguration_relay_delegation_global(seed: u64) -> GlobalType {
    let rounds = (seed as usize % 2) + 2;

    fn relay_rounds(n: usize) -> GlobalType {
        if n == 0 {
            return GlobalType::End;
        }
        GlobalType::send(
            "Controller",
            "RelayA",
            Label::new("delegate_relay"),
            GlobalType::send(
                "RelayA",
                "RelayB",
                Label::new("relay_churn"),
                GlobalType::send(
                    "RelayB",
                    "Controller",
                    Label::new("verify_coherence"),
                    relay_rounds(n - 1),
                ),
            ),
        )
    }

    GlobalType::send(
        "Controller",
        "BundleA",
        Label::new("link_bundle"),
        relay_rounds(rounds),
    )
}

fn strict_fail_closed_runtime_global(seed: u64) -> GlobalType {
    let rounds = (seed as usize % 2) + 1;

    fn rejection_rounds(n: usize) -> GlobalType {
        if n == 0 {
            return GlobalType::send(
                "Runtime",
                "Audit",
                Label::new("commit_rejection"),
                GlobalType::End,
            );
        }

        GlobalType::send(
            "Runtime",
            "Transport",
            Label::new("issue_receipt"),
            GlobalType::send(
                "Transport",
                "Runtime",
                Label::new("reject_forged_receipt"),
                GlobalType::send(
                    "Runtime",
                    "OwnerRegistry",
                    Label::new("issue_owner_capability"),
                    GlobalType::send(
                        "OwnerRegistry",
                        "Runtime",
                        Label::new("reject_forged_authority"),
                        GlobalType::send(
                            "Runtime",
                            "Timer",
                            Label::new("issue_timeout_witness"),
                            GlobalType::send(
                                "Timer",
                                "Runtime",
                                Label::new("expire_timeout_witness"),
                                rejection_rounds(n - 1),
                            ),
                        ),
                    ),
                ),
            ),
        )
    }

    rejection_rounds(rounds)
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

#[derive(Debug, Clone, Serialize)]
struct ContractRunResult {
    bundle: String,
    seed: u64,
    status: String,
    observable_events: usize,
    missing_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ScenarioContractReport {
    schema_version: String,
    profile: String,
    results: Vec<ContractRunResult>,
}

fn artifact_path() -> PathBuf {
    std::env::var("AURA_SCENARIO_CONTRACT_ARTIFACT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("artifacts/conformance/scenario_contracts.json"))
}

fn collect_labels(trace: &[ObsEvent]) -> std::collections::BTreeSet<String> {
    trace
        .iter()
        .filter_map(|event| match event {
            ObsEvent::Sent { label, .. } | ObsEvent::Received { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn scenario_contract_bundles_hold_under_ci_profile() {
    let mut report = ScenarioContractReport {
        schema_version: "aura.scenario_contracts.v1".to_string(),
        profile: "telltale_ci".to_string(),
        results: Vec::new(),
    };

    for bundle in CONTRACT_BUNDLES {
        for seed in CONTRACT_SEEDS {
            let global = (bundle.build)(*seed);
            let locals = project_locals(&global);
            let image = CodeImage::from_local_types(&locals, &global);
            let config = build_vm_config(
                AuraVmHardeningProfile::Ci,
                AuraVmParityProfile::NativeCooperative,
            );
            let mut vm = VM::new(config);
            let handler = NoOpHandler;

            vm.load_choreography(&image).expect("load choreography");
            let status = vm.run(&handler, 128).expect("run choreography");
            let trace = vm.trace().to_vec();
            let labels = collect_labels(&trace);

            let missing_labels = bundle
                .required_labels
                .iter()
                .filter(|label| !labels.contains(**label))
                .map(|label| (*label).to_string())
                .collect::<Vec<_>>();

            report.results.push(ContractRunResult {
                bundle: bundle.id.to_string(),
                seed: *seed,
                status: format!("{status:?}"),
                observable_events: trace.len(),
                missing_labels,
            });
        }
    }

    let artifact = artifact_path();
    if let Some(parent) = artifact.parent() {
        fs::create_dir_all(parent).expect("create scenario-contract artifact directory");
    }
    fs::write(
        &artifact,
        serde_json::to_vec_pretty(&report).expect("serialize scenario-contract report"),
    )
    .expect("write scenario-contract report");

    let violations = report
        .results
        .iter()
        .filter(|result| {
            result.status != format!("{:?}", RunStatus::AllDone)
                || result.observable_events
                    < CONTRACT_BUNDLES
                        .iter()
                        .find(|bundle| bundle.id == result.bundle)
                        .map(|bundle| bundle.min_observable_events)
                        .unwrap_or(0)
                || !result.missing_labels.is_empty()
        })
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "scenario contract violations detected:\n{}",
        serde_json::to_string_pretty(&violations).expect("serialize scenario-contract violations")
    );
}

#[test]
fn scenario_contract_real_invitation_source_runs_with_non_noop_handler() {
    let global = invitation_exchange_global_from_source();
    let locals = project_locals(&global);
    let image = protocol_machine_image(&global, &locals);
    let config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::NativeCooperative,
    );
    let handler = Arc::new(AuraVmEffectHandler::default());
    let mut engine = AuraChoreoEngine::new(config, Arc::clone(&handler));
    for tick in 0..=4 {
        handler.schedule_topology_event(
            tick,
            ProtocolMachineTopologyPerturbation::Crash {
                site: "invitation-topology-node".to_string(),
            },
        );
    }

    engine
        .open_protocol_machine_session(&image)
        .expect("load real invitation choreography");
    let status = engine.run(128).expect("run invitation choreography");
    assert_eq!(status, ProtocolMachineRunStatus::AllDone);

    let effect_kinds = engine
        .vm_effect_trace()
        .into_iter()
        .map(|entry| entry.effect_kind)
        .collect::<BTreeSet<_>>();
    assert!(
        effect_kinds.contains("send_decision"),
        "non-noop handler run should include send_decision effects"
    );
    assert!(
        effect_kinds.contains("handle_recv"),
        "non-noop handler run should include handle_recv effects"
    );
    assert!(
        effect_kinds.contains("topology_event"),
        "scheduled topology event should appear in effect trace"
    );
}
