# Simulation Guide

The AMP lifecycle harness uses a checked-in deterministic Quint trace.
`just ci-amp-lifecycle-trace` checks regeneration, and
`cargo test -p aura-simulator --test amp_channel_itf` replays it with isolated
storage. The creation action commits the protocol checkpoint and complete
canonical chat creation before an invitation resolves its context. See
[Testing Guide](804_testing_guide.md#amp-lifecycle-replay) for regeneration.

This guide covers how to use Aura's simulation infrastructure for testing distributed protocols under controlled conditions.

## When to Use Simulation

Simulation suits scenarios that unit tests cannot address. Use simulation for fault injection testing. Use it for multi-participant protocol testing. Use it for time-dependent behavior validation.

Use direct effect-handler tests for single-component validation. For distributed runtime and app workflows, the primary feedback loop is deterministic in-process execution of the real implementation. Use controlled transport, selected virtual-time providers and real cryptographic, journal and ownership paths. A model-only adapter or a mock success response cannot establish complete-flow correctness.

See [Simulation Infrastructure Reference](119_simulator.md) for the complete architecture documentation.

## Simulation vs Harness

The simulation architecture is specified in [Simulation Infrastructure Reference](119_simulator.md). The harness architecture is specified in [User Flow Harness](121_user_flow_harness.md).

Validate complete runtime and app workflows in-process before escalating to frontend or cross-machine execution. The harness remains the executor for real frontend scenarios. Local browser and TUI conformance tests establish submission, rendering, subscriptions and platform storage behavior. Cross-machine P2P runs are a final, batched integration check for real transport, discovery and process/platform boundaries.

If a required flow cannot execute completely in-process, identify the missing production adapter or reusable fixture and extend that seam before using LAN runs as the development loop. Keep genuinely platform-specific checks in their appropriate local lane. Share semantic actions and expected outcomes through `aura-app`; do not create a second protocol implementation or frontend executor.

### Validation sequence

| Stage | Required evidence |
|---|---|
| Narrow compile and component tests | Production APIs compile; focused invariants and error sources hold |
| Complete in-process flow | Real participants, protocol, app owner, persistence, cancellation, disposal and restart under selected virtual time |
| Local frontend conformance | Actual renderer/input wiring, browser or native storage, subscription and teardown behavior |
| Checkpoint gates and main smoke | Required policy and integration checks on the resulting source revision |
| Batched cross-machine confirmation | Prebuilt identical revisions; declared E2E flows and real network/platform integration |

Declare package, features, exact test inventory and expected count before building. Discover intended fixtures before executing them; zero or ignored matches fail validation. Reuse valid evidence for unchanged scopes and rerun affected scopes when their dependencies change. Record the first failing typed action, original operation/owner, seed, provider, endpoint and fault boundary so a later LAN failure can become a deterministic regression.

Select stages that cover the affected contract. A pure component or documentation change does not require an unrelated full-flow run. Preserve explicit task and policy requirements, and never use a narrow result to claim broader lifecycle or platform coverage.

### Assign coverage before execution

Use the lowest stage that executes the affected contract. Record unsupported stages as fixture or adapter work, with a named owner and dependency, before scheduling broader runs.

| Contract under test | Primary evidence | Remaining integration evidence |
|---|---|---|
| Admission, signing, journal convergence, replay and cancellation | Actual runtime/app execution with controlled effects and deterministic fault boundaries | Final network integration of locally proven flows |
| Required persistence, task disposal and profile reopen | Real storage and owned runtime lifecycle in-process | Platform-specific persistence and process integration |
| Browser storage, renderer events and TUI input wiring | Actual local browser/TUI conformance | Cross-machine user-flow integration |
| Socket discovery, real transport and independent processes | Focused network diagnostics when necessary | One declared, batched both-host E2E run |

A passing runtime fixture does not close an app terminal-publication or browser persistence gap. Keep those dependencies explicit while reusing the same participant setup and semantic contract. Likewise, a passing model or scenario adapter does not certify production execution. Report discovered and executed fixtures separately for each stage.

### When a complete flow is missing

Record the first unsupported stage, its canonical production owner and the
smallest adapter or fixture needed to execute it. Implement that seam with its
focused regression in the active batch, compile it, then reuse it in the actual
flow. Keep the broader task open until that flow executes. Do not substitute a
mock success, copied protected state or repeated LAN attempts for the missing
stage. Reuse participant setup, fault controls and cleanup across scenarios.

Before implementation, settle ownership, provider, original deadline and
cancellation behavior. Keep one coherent integration batch per worktree and at
most one next draft. Compile before expanding adjacent functionality; review the
implementation using compiler and focused test feedback. Classify new findings
as current blockers or subsequent work.

### First reusable complete flow

Enrollment is the first target for extending complete-flow coverage. Reuse genuine account/profile setup, signed setup export, explicit independent verifier transfer, original issuer registration, actual acceptance/rejection, app terminal publication, host persistence acknowledgment, required disposal and same-profile reopen. Obtain all capabilities and receipts from their canonical producers. Named fault boundaries should cover preparation, required writes, host finalization, terminal observation and shutdown.

Include a failure after actual issuance in the fixture inventory. Route early errors through cleanup of every assembled owner, retain the primary native error separately from cleanup errors, and observe task disposal before attempting same-profile reacquisition. A dropped fixture or empty task snapshot alone does not acknowledge cleanup.

Keep effect providers and reusable fault controls in `aura-testkit`, runtime assembly in sanctioned `aura-agent` test support/factories, and scenario orchestration in the simulator or existing integration tests. Preserve downward production dependencies. This is a coverage target, not a claim that a complete reusable enrollment fixture already exists. Extend the same pattern to other flows once this vertical slice compiles and executes.

## Two Simulation Systems

The two simulation systems (TOML scenarios and Quint actions) are specified in [Simulation Infrastructure Reference](119_simulator.md).

| Use Case | System |
|----------|--------|
| End-to-end integration | TOML scenarios |
| Named fault injection | TOML scenarios |
| Conformance testing | Quint actions |
| State space exploration | Quint actions |

When you need user-facing coverage, promote the scenario into the real-runtime harness lane after it is stable in simulation. Treat simulation as a substrate for controlled runtime conditions, not as the final UI executor.

## TOML Scenario Authoring

### Creating Scenarios

Scenario files live in the `scenarios/` directory.

```toml
[metadata]
name = "recovery_basic"
description = "Basic guardian recovery flow"

[[phases]]
name = "setup"
actions = [
    { type = "create_participant", id = "owner" },
    { type = "create_participant", id = "guardian1" },
    { type = "create_participant", id = "guardian2" },
]

[[phases]]
name = "recovery"
actions = [
    { type = "run_choreography", choreography = "guardian_recovery", participants = ["owner", "guardian1", "guardian2"] },
]

[[properties]]
name = "owner_recovered"
property_type = "safety"
```

Each scenario has metadata, ordered phases, and property definitions.

### Defining Phases

Phases execute in order. Each phase contains a list of actions.

```toml
[[phases]]
name = "fault_injection"
actions = [
    { type = "apply_network_condition", condition = "partition", duration = "5s" },
    { type = "advance_time", duration = "10s" },
]
```

Actions within a phase execute sequentially. Use multiple phases to organize complex scenarios.

### Adding Fault Injection

Fault injection actions simulate adverse conditions.

```toml
[[phases]]
name = "chaos"
actions = [
    { type = "simulate_data_loss", participant = "guardian1", percentage = 50 },
    { type = "apply_network_condition", condition = "high_latency", duration = "30s" },
]
```

Available conditions include `partition`, `high_latency`, `packet_loss`, and `byzantine`.

### Running Scenarios

```bash
cargo run --package aura-terminal -- scenario run scenarios/recovery_basic.toml
```

The scenario handler parses and executes the TOML file. Results report property pass/fail status.

## Working with Handlers

### Basic Handler Composition

```rust
use aura_simulator::handlers::SimulationEffectComposer;
use aura_core::DeviceId;

let device_id = DeviceId::new_from_entropy([1u8; 32]);
let composer = SimulationEffectComposer::for_testing(device_id).await?;
let env = composer
    .with_time_control()
    .with_fault_injection()
    .build()?;
```

The composer builds a complete effect environment from handler components.

### Time Control

```rust
use aura_simulator::handlers::SimulationTimeHandler;

let mut time = SimulationTimeHandler::new();
let start = time.physical_time().await?;
time.jump_to_time(Duration::from_secs(60));
let later = time.physical_time().await?;
```

This handler advances time through explicit calls and its own sleep operations. For concurrent real-runtime fixtures, inject `ManualPhysicalClock` before service assembly when the test controls exact boundaries, or use the shared `QuiescentClock` with paused Tokio time for whole-flow scheduling. Sleeps on these runtime providers wait for controlled progress; one background sleeper must not advance every participant past a ceremony deadline. Retain original providers and durable endpoints across restart. Wall clock never decides distributed ordering, and timeout widening cannot repair readiness.

### Fault Injection

```rust
use aura_simulator::handlers::SimulationFaultHandler;
use aura_core::{AuraFault, AuraFaultKind, FaultEdge};

let faults = SimulationFaultHandler::new(42);

faults.inject_fault(
    AuraFault::new(AuraFaultKind::MessageDelay {
        edge: FaultEdge::new("alice", "bob"),
        min: Duration::from_millis(100),
        max: Duration::from_millis(500),
    }),
    None,
)?;

faults.inject_fault(
    AuraFault::new(AuraFaultKind::MessageDrop {
        edge: FaultEdge::new("alice", "bob"),
        probability: 0.1,
    }),
    None,
)?;
```

`AuraFault` is the canonical simulator fault model. Legacy scenario fault forms should be converted to `AuraFault` before injection or replay.

### Triggered Scenarios

```rust
use aura_simulator::handlers::{
    SimulationScenarioHandler,
    ScenarioDefinition,
    TriggerCondition,
    InjectionAction,
};
use aura_core::{AuraFault, AuraFaultKind};

let handler = SimulationScenarioHandler::new(42);
handler.register_scenario(ScenarioDefinition {
    id: "late_partition".to_string(),
    name: "Late Partition".to_string(),
    trigger: TriggerCondition::AfterTime(Duration::from_secs(30)),
    actions: vec![InjectionAction::TriggerFault {
        fault: AuraFault::new(AuraFaultKind::NetworkPartition {
            partition: vec![vec!["device1".into(), "device2".into()], vec!["device3".into()]],
            duration: Some(Duration::from_secs(15)),
        }),
    },
    duration: Some(Duration::from_secs(45)),
    priority: 10,
});
```

Triggered scenarios inject faults at specific times or protocol states.

## Integrating Feature Crates

Layer 5 feature crates (sync, recovery, chat, etc.) integrate with simulation through the effect system. This section covers patterns for wiring feature crates into simulation environments.

### Required Effects

Feature crates are generic over effect traits. Common requirements include:

| Effect Trait | Purpose |
|--------------|---------|
| `NetworkEffects` | Transport and peer communication |
| `JournalEffects` | Fact retrieval and commits |
| `CryptoEffects` | Hashing and signature verification |
| `PhysicalTimeEffects` | Timeouts and scheduling |
| `RandomEffects` | Nonce generation |

Pass the effect system from `aura-simulator` or `aura-testkit` for deterministic testing. In production, use `aura-agent`'s runtime effects.

### Configuration for Simulation

Feature crates typically provide testing configurations that minimize timeouts and remove jitter:

```rust
use aura_sync::SyncConfig;

// Production: conservative timeouts, adaptive scheduling
let prod_config = SyncConfig::for_production();

// Testing: fast timeouts, no jitter, predictable behavior
let test_config = SyncConfig::for_testing();

// Validate before use
test_config.validate()?;
```

Environment variables (prefixed per-crate, e.g., `AURA_SYNC_*`) allow per-process tuning without code changes.

### Guard Chain Integration

The guard chain sequence is specified in [Authorization](106_authorization.md).

For simulation, capability checks rely on Biscuit tokens evaluated by `AuthorizationEffects`. Guard evaluators must be provided by the runtime before sync operations. Validation occurs before sending or applying any protocol data.

### Observability in Simulation

Connect feature crates to `MetricsCollector` for simulation diagnostics:

```rust
use aura_core::metrics::MetricsCollector;

let metrics = MetricsCollector::new();
// Protocol timings, retries, and failure reasons flow to metrics
// Log transport and authorization failures for debugging
```

### Safety Requirements

Feature crates must follow effect system rules. All I/O and timing must flow through effects with no direct runtime calls. Validate Biscuit tokens before accepting peer data. Enforce flow budgets and leakage constraints at transport boundaries.

Verify compliance before simulation:

```bash
just ci-effects
```

## Debugging Simulations

### Deterministic Configuration

Always use deterministic settings for reproducible debugging.

```rust
let config = SimulatorConfig {
    device_id: DeviceId::new_from_entropy([1u8; 32]),
    network: NetworkConfig::default(),
    enable_fault_injection: false,
    deterministic_time: true,
};
```

Deterministic identifiers and time enable exact failure reproduction.

### Effect System Compliance

Verify protocol code follows effect guidelines before simulation.

```bash
just check-arch
```

The architecture checker flags direct time, randomness, or I/O usage. Non-compliant code breaks simulation determinism.

### Monitoring State

```rust
let metrics = middleware.get_metrics();
println!("Messages: {}", metrics.messages_sent);
println!("Faults: {}", metrics.faults_injected);
println!("Duration: {:?}", metrics.simulation_duration);
```

Middleware metrics help identify unexpected behavior.

### Common Issues

Flaky simulation results indicate non-determinism. Check for direct system calls. Check for uncontrolled concurrency. Check for ordering assumptions.

Slow simulations indicate inefficient fault configuration. Reduce fault rates for initial debugging. Increase rates for stress testing.

## Online Property Monitoring

Aura simulator supports per-tick property monitoring through `aura_simulator::AuraProperty`, `aura_simulator::AuraPropertyMonitor`, and `aura_simulator::default_property_suite(...)`.

The monitor checks properties on each simulation tick using `PropertyStateSnapshot` input. This includes events, buffer sizes, local-type depths, flow budgets, and optional session, coroutine, and journal snapshots.

```rust
use aura_simulator::{
    AuraPropertyMonitor, ProtocolPropertyClass, ProtocolPropertySuiteIds,
    PropertyMonitoringConfig, SimulationScenarioConfig,
};

let monitoring = PropertyMonitoringConfig::new(
    ProtocolPropertyClass::Consensus,
    ProtocolPropertySuiteIds { session, context },
)
.with_check_interval(1)
.with_snapshot_provider(|tick| build_snapshot_for_tick(tick));

let config = SimulationScenarioConfig {
    property_monitoring: Some(monitoring),
    ..SimulationScenarioConfig::default()
};

let results = env.run_scenario("consensus".into(), "with property checks".into(), config).await?;
assert!(results.property_violations.is_empty());
```

Default suites are available for consensus, sync, chat, and recovery protocol classes. Scenario results include `properties_checked` and `property_violations` for CI reporting.

## Adaptive Privacy Simulation

Adaptive privacy validation uses simulation for cross-layer evidence. The simulator exercises anonymous path establishment, established-path reuse and expiry, move batching, cover traffic, selector-based retrieval, hold retention, provider saturation, partitions, churn, and long-offline bootstrap re-entry.

The adaptive privacy matrix should include small, medium, and large reachable sets. It should also include clustered social topologies, partition and heal cycles, sparse sync opportunities, low organic traffic, provider saturation, and stale-node return with dead remembered descriptors.

The evidence path tunes policy constants for production. Production ships one fixed adaptive policy. Development and simulation may vary constants such as cover floor, path-diversity floor, delay gain, hold retention, and retrieval rotation, but those tuning surfaces must not become user-facing production configuration.

The current fixed-policy evidence is anchored by `crates/aura-simulator/tests/adaptive_privacy_phase_six.rs`. That lane checks the tuned policy, archived control-plane reports, bootstrap-observer reports, Telltale-backed anonymous path establishment, and reply-block accountability.

## Quint Integration

Quint actions enable model-based testing. See [Verification and MBT Guide](806_verification_guide.md) for complete workflows.

### When to Use Quint

Use Quint actions for conformance testing against formal specifications. Use them for generative state exploration. Do not use them for simple integration tests.

### Basic Trace Replay

```rust
use aura_simulator::quint::itf_loader::ITFLoader;
use aura_simulator::quint::generative_simulator::GenerativeSimulator;

let trace = ITFLoader::load("trace.itf.json")?;
let simulator = GenerativeSimulator::new(config)?;
let result = simulator.replay_trace(&trace).await?;
assert!(result.all_properties_passed());
```

Trace replay validates implementation against Quint model behavior.

AMP epoch-transition simulation has a focused regression lane in
`crates/aura-simulator/tests/amp_transition_scenarios.rs`. It covers delayed
witnesses, partitions that create conflicting `A2` certificates, subtractive
old-epoch receive cutover, emergency quarantine, emergency cryptoshred, replay,
abort, supersession, and alarm-spam cases. Run it with
`cargo test -p aura-simulator --test amp_transition_scenarios`; broader local
development can continue to use `just test-crate aura-simulator` without
requiring an exhaustive Quint/Apalache matrix.

## Conformance Workflow

Simulation feeds native/WASM conformance testing. See [Testing Guide](804_testing_guide.md) for conformance lanes and corpus policy.

### Generating Corpus

```bash
quint run --out-itf=trace.itf.json verification/quint/consensus/core.qnt
```

ITF traces from Quint become conformance test inputs.

### Running Conformance

```bash
just ci-conformance
```

Conformance lanes compare execution across platforms using simulation-controlled environments.

## Checkpoints, Contracts, and Shared Replay

Phase 0 hardening uses three simulator workflows as CI gates.

### Checkpoint Snapshot Workflow

`SimulationScenarioHandler` supports portable checkpoint snapshots. The `export_checkpoint_snapshot(label)` function exports a serializable `ScenarioCheckpointSnapshot`. The `import_checkpoint_snapshot(snapshot)` function restores a checkpoint into a fresh simulator instance.

This enables baseline checkpoint persistence for representative choreography suites. It also supports restore-and-continue regression tests. Upgrade smoke tests can resume from pre-upgrade snapshots.

Use checkpoints when validating runtime upgrades or migration safety, not only end-to-end success.

### Scenario Contract Workflow

Conformance CI includes scenario contracts for consensus, sync, recovery, and reconfiguration. Each bundle is validated over a seed corpus. Validation checks terminal status (`AllDone`), required labels observed in trace, and minimum observable event count.

Contract results are written as JSON artifacts. CI fails on any violation with structured output.

### Shared Replay Workflow

Replay-heavy parity lanes should use shared replay APIs. Use `run_replay_shared(...)` and `run_concurrent_replay_shared(...)` for this purpose.

These APIs reduce duplicate replay state across lanes and keep replay artifacts compatible with canonical trace fragments. Conformance lanes also emit deterministic replay metrics artifacts so regressions in replay footprint are visible during CI review.

For fault-aware replays, persist `entries + faults` bundles and re-inject faults before replay. Use `aura_testkit::ReplayTrace::load_file(...)` to load traces. Use `ReplayTrace::replay_faults(...)` and `aura_simulator::AsyncSimulatorHostBridge::replay_expected_with_faults(...)` to replay with faults.

### Differential Replay Workflow

Use `aura_simulator::DifferentialTester` to compare baseline and candidate conformance artifacts. Two profiles are available. The `strict` profile requires byte-identical surfaces. The `envelope_bounded` profile uses Aura law-aware comparison with commutative and algebraic envelopes. In supported Telltale 11-backed report lanes, this is the low-level surface comparator, not the theorem-facing semantic authority: use the parity report's upstream semantic summary and run context.

For Telltale 11-backed parity lanes, prefer
`aura_simulator::run_telltale_parity_with_runner(...)` or
`aura_simulator::run_telltale_control_plane_with_runner(...)` over manually
assembling candidate upstream sidecar paths. These helpers invoke the
configured Telltale runner command, attach the generated candidate run-output
sidecar, and then emit the normal Aura parity report. Supported file lanes
still require a baseline upstream run sidecar, and they attach optional
decision/sweep sidecars when available. Override the command with
`AURA_TELLTALE_SIMULATOR_RUNNER` when needed.

For environment-oriented simulation state, Aura now exposes a bridge layer that
uses Telltale 11-aligned names for migrated slices. Prefer the scenario-run
artifact lane over ad hoc handler inspection: after `run_scenario(...)`,
inspect `SimulationResults::environment_artifacts` and the persisted
`environment_snapshot.json` / `environment_trace.json` files under the
configured simulator artifact root. When richer Aura-only environment detail is
needed, read the optional `environment_overlay.json` supplement instead of
expanding the core bridge schema.

For comparative experiment work, use
`aura_simulator::run_adaptive_privacy_policy_sweep(...)` plus
`aura_simulator::compare_policy_sweeps(...)` instead of ad hoc local sweep
scripts. Those APIs keep Aura-specific bindings on top of the shared Telltale
execution machinery while emitting Aura-owned `AuraSweepArchiveV1` and
`AuraPolicyDiffReportV1` artifacts.

For reusable regression bundles, use `aura_simulator::run_suite_catalog(...)`
and `aura_simulator::compare_suite_catalogs(...)`. The suite catalogs are
Aura-owned, but execution still goes through the shared Telltale harness and
the archived comparison surface is `AuraSuiteTournamentReportV1`.

For theorem-aware failure evidence, convert parity reports with
`aura_simulator::counterexample_from_parity_report(...)` or
`aura_simulator::counterexample_from_control_plane_report(...)`. These helpers
preserve the shared Telltale counterexample witness and record whether the
observed mismatch is schedule noise only.

For parity debugging, run:

```bash
just ci-choreo-parity
aura replay --trace-file artifacts/choreo-parity/native_replay/<scenario>__seed_<seed>.json
```

The `replay` command validates required conformance surfaces and verifies stored step/run digests against recomputed values.

## Best Practices

Start with simple scenarios and add faults incrementally. Use deterministic seeds. Capture metrics for analysis.

Prefer TOML scenarios for human-readable tests. Prefer Quint actions for specification conformance. Combine both for comprehensive coverage.

## Related Documentation

- [Simulation Infrastructure Reference](119_simulator.md) - Handler APIs
- [Verification and MBT Guide](806_verification_guide.md) - Quint workflows
- [Testing Guide](804_testing_guide.md) - Conformance testing
