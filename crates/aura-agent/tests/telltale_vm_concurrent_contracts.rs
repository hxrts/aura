//! Concurrent mixed-protocol contract lane for Telltale VM backends.
#![cfg(feature = "choreo-backend-telltale-vm")]
#![allow(clippy::expect_used, clippy::disallowed_methods, missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use aura_agent::{build_vm_config, AuraVmHardeningProfile, AuraVmParityProfile};
use serde::Serialize;
use telltale_types::{GlobalType, Label, LocalTypeR};
use telltale_vm::coroutine::Value;
use telltale_vm::effect::EffectHandler;
use telltale_vm::loader::CodeImage;
use telltale_vm::threaded::ThreadedVM;
use telltale_vm::vm::{ObsEvent, RunStatus, VMConfig, VM};

const MIX_SEEDS: &[u64] = &[3, 11, 29];
const REQUIRED_LABELS: &[&str] = &[
    "proposal",
    "witness_vote",
    "observer_cert",
    "commit",
    "invite_issue",
    "invite_accept",
    "invite_commit",
    "delta",
    "receipt",
    "relay_receipt",
    "link_bundle",
    "delegate_session",
    "verify_coherence",
];
const RECONFIGURATION_LABELS: &[&str] = &["link_bundle", "delegate_session", "verify_coherence"];

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
struct RunSummary {
    backend: &'static str,
    seed: u64,
    status: String,
    observable_events: usize,
    scheduler_steps: usize,
    sent_labels: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
struct ConcurrentContractArtifact {
    schema_version: &'static str,
    reports: Vec<RunSummary>,
}

fn project_locals(global: &GlobalType) -> BTreeMap<String, LocalTypeR> {
    telltale_theory::projection::project_all(global)
        .expect("project choreography")
        .into_iter()
        .collect()
}

fn image_for(global: GlobalType) -> CodeImage {
    let locals = project_locals(&global);
    CodeImage::from_local_types(&locals, &global)
}

fn mixed_protocol_pressure_global(seed: u64) -> GlobalType {
    let commit_label = if seed & 1 == 0 {
        "commit_fast"
    } else {
        "commit_fallback"
    };
    let invite_issue = if seed & 1 == 0 {
        "invite_issue_primary"
    } else {
        "invite_issue_alternate"
    };
    let rounds = ((seed % 2) as usize) + 1;

    fn reconfiguration_rounds(remaining: usize) -> GlobalType {
        if remaining == 0 {
            return GlobalType::End;
        }
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
                    reconfiguration_rounds(remaining - 1),
                ),
            ),
        )
    }

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
                    "Inviter",
                    "Invitee",
                    Label::new(invite_issue),
                    GlobalType::send(
                        "Invitee",
                        "Inviter",
                        Label::new("invite_accept"),
                        GlobalType::send(
                            "Primary",
                            "Replica",
                            Label::new("delta"),
                            GlobalType::send(
                                "Replica",
                                "Primary",
                                Label::new("receipt"),
                                GlobalType::send(
                                    "Committer",
                                    "Proposer",
                                    Label::new(commit_label),
                                    GlobalType::send(
                                        "Inviter",
                                        "Context",
                                        Label::new("invite_commit"),
                                        GlobalType::send(
                                            "Primary",
                                            "Relay",
                                            Label::new("propagate"),
                                            GlobalType::send(
                                                "Relay",
                                                "Replica",
                                                Label::new("relay_receipt"),
                                                reconfiguration_rounds(rounds),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    )
}

fn load_mixed_protocol_workload<F>(mut load: F, seed: u64)
where
    F: FnMut(&CodeImage),
{
    let image = image_for(mixed_protocol_pressure_global(seed));
    for _ in 0..8 {
        load(&image);
    }
}

fn collect_sent_labels(trace: &[ObsEvent]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for event in trace {
        if let ObsEvent::Sent { label, .. } = event {
            *counts.entry(label.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn run_cooperative(seed: u64) -> RunSummary {
    let handler = NoOpHandler;
    let config = build_vm_config(
        AuraVmHardeningProfile::Ci,
        AuraVmParityProfile::NativeCooperative,
    );
    let mut vm = VM::new(config);
    load_mixed_protocol_workload(
        |image| {
            vm.load_choreography(image).expect("load choreography");
        },
        seed,
    );
    let status = vm
        .run_concurrent(&handler, 4096, 1)
        .expect("run cooperative workload");
    let trace = vm.trace().to_vec();
    RunSummary {
        backend: "cooperative",
        seed,
        status: format!("{status:?}"),
        observable_events: trace.len(),
        scheduler_steps: trace.len(),
        sent_labels: collect_sent_labels(&trace),
    }
}

fn run_threaded(seed: u64) -> RunSummary {
    let handler = NoOpHandler;
    let config = VMConfig::default();
    let mut vm = ThreadedVM::with_workers(config, 4);
    load_mixed_protocol_workload(
        |image| {
            vm.load_choreography(image).expect("load choreography");
        },
        seed,
    );
    let status = vm
        .run_concurrent(&handler, 4096, 4)
        .expect("run threaded workload");
    let trace = vm.trace().to_vec();
    RunSummary {
        backend: "threaded",
        seed,
        status: format!("{status:?}"),
        observable_events: trace.len(),
        scheduler_steps: trace.len(),
        sent_labels: collect_sent_labels(&trace),
    }
}

fn assert_required_labels_present(report: &RunSummary) {
    let observed = report
        .sent_labels
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for label in REQUIRED_LABELS {
        assert!(
            observed
                .iter()
                .any(|candidate| candidate.starts_with(label)),
            "missing required label prefix `{label}` for backend={} seed={} observed={observed:?}",
            report.backend,
            report.seed,
        );
    }
}

fn reconfiguration_counts(report: &RunSummary) -> BTreeMap<String, usize> {
    report
        .sent_labels
        .iter()
        .filter(|(label, _)| {
            RECONFIGURATION_LABELS
                .iter()
                .any(|required| label.starts_with(required))
        })
        .map(|(label, count)| (label.clone(), *count))
        .collect()
}

fn maybe_write_artifact(reports: &[RunSummary]) {
    let Ok(path) = std::env::var("AURA_CONCURRENT_CONTRACT_ARTIFACT") else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create concurrent-contract artifact directory");
    }
    let artifact = ConcurrentContractArtifact {
        schema_version: "aura.concurrent-contracts.v1",
        reports: reports.to_vec(),
    };
    fs::write(
        path,
        serde_json::to_vec_pretty(&artifact).expect("serialize concurrent-contract artifact"),
    )
    .expect("write concurrent-contract artifact");
}

#[test]
fn mixed_protocol_classes_complete_under_concurrent_load() {
    let mut reports = Vec::new();

    for seed in MIX_SEEDS {
        let cooperative = run_cooperative(*seed);
        assert_eq!(cooperative.status, format!("{:?}", RunStatus::AllDone));
        assert_required_labels_present(&cooperative);
        reports.push(cooperative);
    }

    maybe_write_artifact(&reports);
}

#[test]
fn reconfiguration_contracts_preserve_backend_parity_under_concurrent_load() {
    let mut reports = Vec::new();

    for seed in MIX_SEEDS {
        let cooperative = run_cooperative(*seed);
        let threaded = run_threaded(*seed);

        assert_eq!(cooperative.status, format!("{:?}", RunStatus::AllDone));
        assert_eq!(threaded.status, format!("{:?}", RunStatus::AllDone));
        assert_required_labels_present(&cooperative);
        assert_required_labels_present(&threaded);
        assert_eq!(
            cooperative.sent_labels, threaded.sent_labels,
            "concurrent label counts diverged across backends for seed={seed}",
        );
        assert_eq!(
            reconfiguration_counts(&cooperative),
            reconfiguration_counts(&threaded),
            "reconfiguration outcomes diverged across backends for seed={seed}",
        );

        reports.push(cooperative);
        reports.push(threaded);
    }

    maybe_write_artifact(&reports);
}
