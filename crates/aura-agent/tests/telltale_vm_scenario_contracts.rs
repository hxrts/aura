//! CI scenario-contract gate for telltale VM execution profiles.
#![cfg(feature = "choreo-backend-telltale-vm")]
#![allow(clippy::expect_used, clippy::disallowed_methods)]

use std::fs;
use std::path::PathBuf;

use aura_agent::{build_vm_config, AuraVmHardeningProfile, AuraVmParityProfile};
use serde::Serialize;
use telltale_types::{GlobalType, Label, LocalTypeR};
use telltale_vm::coroutine::Value;
use telltale_vm::effect::EffectHandler;
use telltale_vm::loader::CodeImage;
use telltale_vm::vm::{ObsEvent, RunStatus, VM};

#[derive(Clone, Copy)]
struct ContractBundleSpec {
    id: &'static str,
    build: fn(u64) -> GlobalType,
    required_labels: &'static [&'static str],
    min_observable_events: usize,
}

const CONTRACT_BUNDLES: &[ContractBundleSpec] = &[
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
];

const CONTRACT_SEEDS: &[u64] = &[1, 7, 42];

fn project_locals(global: &GlobalType) -> std::collections::BTreeMap<String, LocalTypeR> {
    telltale_theory::projection::project_all(global)
        .expect("project global choreography to local session types")
        .into_iter()
        .collect()
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
