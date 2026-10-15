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

For enrollment, `aura-simulator --test enrollment_lifecycle` uses a
simulation-feature agent adapter with genuine signed setup, transferred verifier
pins and native profile owners. Its public-app fixtures cover an acknowledged
nonsecret host completion write followed by same-profile reopen and a real
filesystem write refusal with the original app instance and storage source.
Run exact test discovery and execution together inside one guarded build. The
fixture does not prove production identity persistence, browser host behavior,
or network delivery to a pre-adoption device; those require their own adapters.

After the in-process fixture passes, `scripts/harness/lan/single-host-enrollment.sh
<fresh-token> <clean-peer-checkout>` runs the private paired checklist through
two separate native owner processes on one host. Both clean checkouts must
contain the same commit; the caller supplies prebuilt `tool_repl` and `aura`
binaries through `AURA_E2E_TOOL_REPL` and `AURA_E2E_AURA_BIN` or their defaults.
The runner checks their hashes, uses disjoint private profiles and loopback
ports, and records the authenticated broker ACK, exact operation instance,
host acknowledgment, process identities, reopen, and independent shutdown.
Its result states `cross_host_asserted:false`. The final two-host batch remains
the evidence for cross-machine delivery and platform behavior.

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

Use narrow compiles and exact deterministic regressions while implementing each
slice. Batch full-project tests, broad ownership aggregates and Clippy at the
reviewable integration checkpoint rather than rerunning them after each fixture
change. Deferred gates remain required acceptance evidence. Reuse compiled
fixtures and valid results for unchanged scopes, then run main smoke on the
integrated revision before final network confirmation.

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

### Declare the next checkpoint

Before editing, attach this short declaration to the active task or batch:

```text
Source revision and affected contract:
Canonical producer and retained owner:
Package / features / test binary / exact names / expected count:
In-process stages covered:
Missing reusable seam and blocking task:
Local frontend evidence required:
Final P2P assertions remaining:
Required gates:
```

After the check, report compilation, discovery and execution separately. Extend
the existing fixture for a missing stage before growing the dependent feature.
Preserve valid receipts for unchanged dependencies; do not repeatedly run a broad
suite while a known shared compiler blocker remains.

### Inventory lifecycle outcomes

For each real flow, record success, rejection, failure after partial progress,
cancellation, persistence, disposal and same-profile restart independently.
Identify the canonical producer, deterministic control and actual acknowledgment
for every applicable stage. Mark a stage not applicable with a reason; name the
missing reusable seam when it is unsupported. Compile and execute one coherent
slice before adding the next stage. This inventory guides fixture development
and keeps the final network batch focused on remaining integration assertions.

### First reusable complete flow

Enrollment is the first target for extending complete-flow coverage. Reuse genuine account/profile setup, signed setup export, explicit independent verifier transfer, original issuer registration, actual acceptance/rejection, app terminal publication, host persistence acknowledgment, required disposal and same-profile reopen. Obtain all capabilities and receipts from their canonical producers. Named fault boundaries should cover preparation, required writes, host finalization, terminal observation and shutdown.

Include a failure after actual issuance in the fixture inventory. Route early errors through cleanup of every assembled owner, retain the primary native error separately from cleanup errors, and observe task disposal before attempting same-profile reacquisition. A dropped fixture or empty task snapshot alone does not acknowledge cleanup.

Keep effect providers and reusable fault controls in `aura-testkit`, runtime assembly in sanctioned `aura-agent` test support/factories, and scenario orchestration in the simulator or existing integration tests. Preserve downward production dependencies. This is a coverage target, not a claim that a complete reusable enrollment fixture already exists. Extend the same pattern to other flows once this vertical slice compiles and executes.

### Bootstrap delivery checkpoint

For the recipient and purpose contract in [Rendezvous](113_rendezvous.md), start
with the existing in-process broker and canonical signed invitation producers.
Register two actual devices of one authority with different retrieval credentials.
Verify that each receives only its own deliveries, a foreign device's credential
cannot retrieve them, and refreshing one registration cannot replace the other.
Check empty retrieval separately from the one-use credential consumed by an
actual nonempty delivery.

Deliver enrollment and contact invitations together. Require the existing
bootstrap owner to retain both, dispatch each by its declared purpose, and
preserve explicit enrollment approval and canonical contact verification.
Exercise wrong-purpose, foreign-recipient, verification-failure and cancellation
outcomes without losing unrelated deliveries. Observe workflow completion and
required disposal independently of the broker's transport receipt.

Declare and confirm the exact discovered tests before execution. After this
checkpoint, use the existing local browser fixture for real HTTP, cross-origin
headers and browser dispatch. Reserve cross-machine transport and full frontend
integration for the final batched Air run. These are required coverage cases;
the current broker client receipts alone do not prove them.

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

For a manually driven runtime replay, install
`ManualPhysicalClock::observe_next_relative_sleep` on the selected provider before
polling the build or replay operation. Its one-shot receiver reports the actual
typed relative-sleep target after wake notification registration. Clones share
the observer slot; a second installed observer is rejected. Verify the expected
wait was admitted (for example, the real five-millisecond reactive batch wait),
then explicitly drive the same provider to that target while continuing to poll
the original operation. The observation grants no processing acknowledgment:
await the real replay completion and retain all original provider/window and
failure custody. Do not auto-advance sleeps or renew a budget to make the fixture
finish. After forward progress, use an actual provider read for refusal
assertions, so an artificial earlier timestamp cannot create the rollback being
asserted.

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

`SimulationScenarioHandler` exports and imports scenario-counter checkpoints. An imported snapshot is registered in a fresh handler; `restore_checkpoint` then restores only its captured scenario tick. Missing or invalid tick data fails closed. It does not restore live runtime owners, credentials, broker queues, storage, tasks, or capabilities.

For production-flow regression, record the seed, selected physical-time inputs, original operation and owner, broker stage transitions, injected fault, and typed outcome. Recreate the original profile and replay those inputs through the real runtime and app owners. A scenario-counter checkpoint alone is not an upgrade or durable-state migration test.

The `simulator-checkpoint` Cargo feature exposes native test infrastructure;
it is not a terminal, web, RPC, or production feature. To inspect the current
owner-level checkpoint contract, run the exact `hxrts-aura-effects` tests:

```bash
scripts/dev/run-exact-fixtures.sh --root "$PWD" --lane debug \
  --package hxrts-aura-effects --lib --features simulator-checkpoint \
  --expected-count 2 \
  --name profile_checkpoint::tests::quiescent_filesystem_checkpoint_resets_authenticated_ordinary_branch_record \
  --name profile_checkpoint::tests::quiescent_filesystem_checkpoint_refuses_unauthenticated_ordinary_branch_record
```

The native setup owner `SimulatedEnrollment::prepare_quiescent_native_signing_profiles`
assembles each original agent with the selected `PhysicalTimeEffects` provider,
obtains a one-use proof of the **runtime-selected** `ProfileOwned(FilesystemFallback)`
handler, and acknowledges both shutdowns before returning. A configuration
default or an existing fallback key does not prove this selection. Consume each
proof with `QuiescentFilesystemProfileCheckpoint::capture`, using the same
canonical original profile path, a private 0700 artifact directory outside
that profile, and a new artifact filename. Capture creates the file at 0600
and refuses an existing name. Bind capture to the source revision and participant.
Keep the artifact in private test storage; the wrapping key stays under the
original profile owner. Neither is a portable credential export. A branch must
reacquire that original path exclusively and call `restore(...).await` with the exact revision and
participant before constructing fresh agent, app, and broker owners. Restore
authenticates the artifact and complete file inventory before mutation, then
uses an incomplete-restore marker so interrupted reset blocks ordinary profile
acquisition. It refuses foreign, plaintext, and undecryptable branch files.

The simulator fixture can inject a `SimulatorNonceRetirementLedger` into the
normal threshold-signing owner through `NativeEnrollmentFixture::prepare_with_nonce_retirement`.
Keep that ledger outside the checkpoint and share it across sequential branches;
it rejects reuse of the same actual FROST commitment for one signer and epoch.
The current 1-of-1 signed-enrollment fixture uses its normal single-signer path,
so it does not prove a FROST retirement or deterministic nonce-counter replay.
No branch may manufacture an approval signature. Record the seed, original
operation and owners, selected clock inputs and deadlines, broker stage IDs,
named fault, typed outcome, and exact original-message ACK in a private receipt.

**Current limit:** the genuine signed enrollment branch creates a protected
allocation-lifetime record in a reserved secure-provider namespace. Its
simulator-only allocation-lifetime owner authenticates the complete quiescent
index and exact added birth files, then retires only those branch-created
records after the durable incomplete-restore marker. The generic checkpoint
never deletes or copies them through ordinary storage or secure-location APIs.
The signed fixture exercises this reset, reacquires fresh original owners,
revalidates normal signed issuance, and checks the original broker message's
receiver ACK and typed host outcome on each branch. Run its exact inventory:

```bash
scripts/dev/run-exact-fixtures.sh --root "$PWD" --lane debug \
  --package aura-simulator --test enrollment_lifecycle \
  --features simulator-checkpoint --expected-count 4 \
  --name original_signed_host_refusal_replays_from_protected_selected_profiles \
  --name restored_original_agent_issues_new_normal_signed_enrollment \
  --name selected_replay_refuses_foreign_owner_and_stale_epoch_before_mutation \
  --name protected_selected_profile_replay_benchmarks_three_branches_against_full_setups
```

The benchmark keeps a redacted 0600 JSON receipt under an owner-only temporary
directory in the checkout's ignored `artifacts/`. It reports capture plus
three restores and signed branches beside three independent full setups;
elapsed wall time is diagnostic, not a CI pass threshold. The build wrapper's
receipt supplies the full source fingerprint and exact discovered/executed
inventory. The replay receipt records only a fixture source digest, seed,
selected clock, operation/owner, named fault, ordered broker milestones,
typed result, exact first-message ACK, reopen result, and elapsed times.
The current three-branch benchmark is diagnostic: recent local runs measured
roughly 7.8–8.0 seconds with checkpoint capture and three restores against
6.9–7.2 seconds for three fresh setups. Restore of the two original profile
inventories costs roughly 0.5–0.9 seconds per branch, so this fixture does
not yet establish a speedup. Preserve the effect owner's authenticated
inventory, exclusive lease, and incomplete-restore marker when optimizing it.

Use the receipt's `checkpoint_capture_ms`, `restore_ms`, `signed_branch_ms`,
and `full_setup_ms` to calculate the break-even branch count for the same
compiled fixture. For `n` branches, compare `capture + n × (restore + branch)`
with `n × full_setup`, using representative phase timings from repeated runs.
If `restore + branch >= full_setup`, this checkpoint has no positive
break-even count. Recent unoptimized receipts have representative phases of
about 0.54 seconds capture, 0.73 seconds restore, 1.70 seconds signed branch,
and 2.31 seconds full setup; the per-branch checkpoint path is slower even
before capture. An unmerged exact-byte retention experiment reduced restore
time, but its four-run median total was still about 7.65 seconds versus
7.02 seconds for three fresh setups. Its phase medians suggest a tentative
break-even near twelve branches, too variable to make it a default speed
lane. Prefer a genuinely quiescent boundary after a more expensive signed
setup or a workflow needing many branches, and remeasure capture, restore,
branch, and full setup on one fixed source. Restore only through fresh canonical
owners when their durable continuation and broker state can be authenticated;
fail closed if that later boundary cannot be recovered.

The named `BrokerOwnerShutdownAfterFirstQueue` branch reuses the same protected
quiescent profiles. It starts the ordinary signed VM/app/broker owners,
observes the original first envelope in the authenticated broker queue, and
stops that broker before receiver retrieval. The shared app maintenance pass
retains the native broker connection-refusal source through its typed runtime
bridge call. Dropping the accepted app observer then requests its real
cancellation; the host callback remains uncalled. The fixture compares two
restored branches' ordered stage names and typed terminal result, checks
strict shutdown, and reacquires the same profiles under fresh owners. Its
private 0600 receipt records only stage names, typed cause and terminal
categories, the selected clock input, fixture binding, and reopen result:

```bash
scripts/dev/run-exact-fixtures.sh --root "$PWD" --lane debug \
  --package aura-simulator --test enrollment_lifecycle \
  --features simulator-checkpoint --expected-count 1 \
  --name protected_original_broker_delivery_fault_retains_cause_and_cancels
```

This branch proves a local broker-owner failure and app cancellation before
receiver ACK; it does not claim that an independent process or host delivered
the first envelope. The `HostPathIsFile` branch above remains the genuine
required host-write refusal after exact receiver ACK and same-profile reopen.

For actual FROST one-use custody, the separately scoped threshold-service
fixture injects the same retirement effect into original and restored owners
and refuses an identical real public commitment in the second owner:

```bash
scripts/dev/run-exact-fixtures.sh --root "$PWD" --lane debug \
  --package hxrts-aura-agent --lib --features simulator-checkpoint \
  --expected-count 1 \
  --name runtime::services::threshold_signing::device_quorum::tests::injected_original_service_refuses_reused_actual_frost_nonce_across_branches
```

There is no simulator nonce-counter selector yet, so there is no duplicate
counter warning or byte-identical crypto replay claim. Reusing an actual
one-time commitment is a hard refusal. The 1-of-1 enrollment fast path uses
normal signing but does not exercise FROST retirement. This checkpoint is only
for quiescent pre-issuance selected filesystem profiles with an empty broker;
each branch creates its broker registration and queue through the normal issue
path. Post-issuance owners, nonempty broker queues, live capabilities, OS
keyrings, browser storage, independent processes, and two-host delivery remain
outside this checkpoint contract. The runtime-selected filesystem provider
does not prove production platform keyring custody.
Before reviewing a deployable change, run
`nix develop --command bash scripts/check/simulator-checkpoint-reachability.sh`;
it checks the terminal/RPC, app-host, and web release feature graphs for the
absence of simulator checkpoint and test-keyring code.

The local broker has a simulator-checkpoint stop gate. It seals admission,
stops and awaits its original task group, and refuses live owners, unsettled
delivery, retained credential residue, ACK replay metadata, and test-only
one-shot hooks. A versioned registration-only codec can encode this stopped
owner's candidates and receiver/return registrations, including their original
retention deadlines and selected-time high-water mark. Private decoding rejects
duplicates, malformed scope, expiry, backward time, noncanonical bytes and versions.
The canonical signed setup fixture confirms its original return registration
appears in that codec. The bytes are **unsealed** and confer no restore
authority: valid-state byte substitution needs the later effect-owned AEAD
binding. There is no fresh broker owner import, broker artifact, or replay
route yet. The fixture authorization token is never a checkpoint encryption key.
The next simulator-only prerequisite preserves the original native broker
handle through signed setup and permits a one-shot stop acknowledgment. The
canonical rendezvous manager likewise takes its broker child through its actor
and refuses outstanding clones. This is still no protected broker artifact,
fresh-owner restore, VM continuation replay, or measured speedup.

The selected filesystem checkpoint also has a simulator-only V2 auxiliary
channel. It encrypts one bounded opaque payload with the original profile
inventory and authenticates payload kind/version, owner, epoch, fixture
configuration, selected physical-clock observation, source revision and
profile identity before restoration. The V1 restore path refuses V2 artifacts,
and the V2 path refuses V1 or missing payloads. The broker codec is not yet
sealed into this auxiliary channel or accepted by a fresh owner. An older valid artifact with the
same context cannot be detected without a separate monotonic anchor. Do not
use fixture bearer tokens as encryption keys or count this channel as a
complete broker co-checkpoint.

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
