#![allow(clippy::expect_used, clippy::disallowed_methods)]
#![allow(missing_docs)]
//! Cooperative vs threaded Telltale VM benchmarks for Category C protocol shapes.

use std::collections::BTreeMap;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use aura_mpst::upstream::types::{GlobalType, Label, LocalTypeR};
use telltale_vm::coroutine::Value;
use telltale_vm::effect::EffectHandler;
use telltale_vm::loader::CodeImage;
use telltale_vm::threaded::ThreadedVM;
use telltale_vm::vm::{RunStatus, VMConfig, VM};

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

fn project_locals(global: &GlobalType) -> BTreeMap<String, LocalTypeR> {
    aura_mpst::upstream::theory::projection::project_all(global)
        .expect("project choreography")
        .into_iter()
        .collect()
}

// Representative Category C shape: coordinator fans out proposal and collects witness acks.
fn category_c_consensus_global() -> GlobalType {
    GlobalType::send(
        "Coordinator",
        "Witness0",
        Label::new("proposal"),
        GlobalType::send(
            "Coordinator",
            "Witness1",
            Label::new("proposal"),
            GlobalType::send(
                "Witness0",
                "Coordinator",
                Label::new("ack"),
                GlobalType::send(
                    "Witness1",
                    "Coordinator",
                    Label::new("ack"),
                    GlobalType::End,
                ),
            ),
        ),
    )
}

// Representative Category C shape: requester commits recovery after quorum evidence.
fn category_c_recovery_global() -> GlobalType {
    GlobalType::send(
        "Requester",
        "Guardian0",
        Label::new("recover"),
        GlobalType::send(
            "Requester",
            "Guardian1",
            Label::new("recover"),
            GlobalType::send(
                "Guardian0",
                "Requester",
                Label::new("approve"),
                GlobalType::send(
                    "Guardian1",
                    "Requester",
                    Label::new("approve"),
                    GlobalType::send(
                        "Requester",
                        "Storage",
                        Label::new("commit"),
                        GlobalType::End,
                    ),
                ),
            ),
        ),
    )
}

// Mixed-class workload: invitation, sync, consensus, and reconfiguration-style steps
// are interleaved in one choreography and then loaded concurrently as many sessions.
fn mixed_protocol_pressure_global() -> GlobalType {
    GlobalType::send(
        "Coordinator",
        "Witness",
        Label::new("proposal"),
        GlobalType::send(
            "Witness",
            "Coordinator",
            Label::new("ack"),
            GlobalType::send(
                "Inviter",
                "Invitee",
                Label::new("invite_issue"),
                GlobalType::send(
                    "Primary",
                    "Replica",
                    Label::new("delta"),
                    GlobalType::send(
                        "Invitee",
                        "Inviter",
                        Label::new("invite_accept"),
                        GlobalType::send(
                            "Controller",
                            "BundleA",
                            Label::new("link_bundle"),
                            GlobalType::send(
                                "Replica",
                                "Primary",
                                Label::new("receipt"),
                                GlobalType::send(
                                    "Controller",
                                    "BundleB",
                                    Label::new("delegate_session"),
                                    GlobalType::send(
                                        "BundleB",
                                        "Controller",
                                        Label::new("verify_coherence"),
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
                                                    GlobalType::End,
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
        ),
    )
}

fn run_cooperative(image: &CodeImage, sessions: usize, max_rounds: usize) {
    let handler = NoOpHandler;
    let mut vm = VM::new(VMConfig::default());
    for _ in 0..sessions {
        vm.load_choreography(image).expect("load choreography");
    }
    let status = vm
        .run_concurrent(&handler, max_rounds, 1)
        .expect("run cooperative");
    assert_eq!(status, RunStatus::AllDone);
}

fn run_threaded(image: &CodeImage, sessions: usize, max_rounds: usize, workers: usize) {
    let handler = NoOpHandler;
    let mut vm = ThreadedVM::with_workers(VMConfig::default(), workers);
    for _ in 0..sessions {
        vm.load_choreography(image).expect("load choreography");
    }
    let status = vm
        .run_concurrent(&handler, max_rounds, workers.max(1))
        .expect("run threaded");
    assert_eq!(status, RunStatus::AllDone);
}

fn bench_pair(c: &mut Criterion, bench_name: &str, global: GlobalType) {
    bench_pair_with_params(c, bench_name, global, 32, 1024);
}

fn bench_pair_with_params(
    c: &mut Criterion,
    bench_name: &str,
    global: GlobalType,
    sessions: usize,
    max_rounds: usize,
) {
    let locals = project_locals(&global);
    let image = CodeImage::from_local_types(&locals, &global);

    let mut group = c.benchmark_group(bench_name);
    group.throughput(Throughput::Elements(sessions as u64));
    group.bench_function("cooperative", |b| {
        b.iter(|| run_cooperative(&image, sessions, max_rounds));
    });
    group.bench_function("threaded_4_workers", |b| {
        b.iter(|| run_threaded(&image, sessions, max_rounds, 4));
    });
    group.finish();
}

fn telltale_vm_runtime_benches(c: &mut Criterion) {
    bench_pair(
        c,
        "category_c_consensus_runtime",
        category_c_consensus_global(),
    );
    bench_pair(
        c,
        "category_c_recovery_runtime",
        category_c_recovery_global(),
    );
    bench_pair_with_params(
        c,
        "mixed_class_throughput_runtime",
        mixed_protocol_pressure_global(),
        32,
        8192,
    );
    bench_pair_with_params(
        c,
        "mixed_class_latency_runtime",
        mixed_protocol_pressure_global(),
        1,
        4096,
    );
    bench_pair_with_params(
        c,
        "mixed_class_contention_runtime",
        mixed_protocol_pressure_global(),
        64,
        16384,
    );
}

criterion_group!(telltale_vm_backends, telltale_vm_runtime_benches);
criterion_main!(telltale_vm_backends);
