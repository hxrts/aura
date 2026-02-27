# Simulation Guide

This guide covers how to use Aura's simulation infrastructure for testing distributed protocols under controlled conditions.

## When to Use Simulation

Simulation suits scenarios that unit tests cannot address. Use simulation for fault injection testing. Use it for multi-participant protocol testing. Use it for time-dependent behavior validation.

Do not use simulation for simple unit tests. Direct effect handler testing is faster and simpler for single-component validation.

See [Simulation Infrastructure Reference](118_simulator.md) for the complete architecture documentation.

## Two Simulation Systems

Aura provides two complementary simulation systems.

TOML scenarios suit human-written integration tests. They provide readable scenario definitions with named phases and fault injection. Use them for end-to-end protocol testing.

Quint actions suit model-based testing. They enable generative state space exploration driven by formal specifications. Use them for conformance testing against Quint models.

| Use Case | System |
|----------|--------|
| End-to-end integration | TOML scenarios |
| Named fault injection | TOML scenarios |
| Conformance testing | Quint actions |
| State space exploration | Quint actions |

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

let time = SimulationTimeHandler::new();
let start = time.current_timestamp().await?;
time.advance(Duration::from_secs(60));
let later = time.current_timestamp().await?;
```

Simulated time advances only through explicit calls. This enables testing timeout behavior without delays.

### Fault Injection

```rust
use aura_simulator::handlers::SimulationFaultHandler;
use aura_simulator::middleware::FaultType;

let mut faults = SimulationFaultHandler::new();
faults.inject_fault(FaultType::NetworkDelay { min_ms: 100, max_ms: 500 });
faults.inject_fault(FaultType::PacketDrop { probability: 0.1 });
```

Faults apply probabilistically during protocol execution.

### Triggered Scenarios

```rust
use aura_simulator::handlers::{
    SimulationScenarioHandler,
    ScenarioDefinition,
    TriggerCondition,
    InjectionAction,
};

let mut handler = SimulationScenarioHandler::new();
handler.add_scenario(ScenarioDefinition {
    name: "late_partition".to_string(),
    trigger: TriggerCondition::AfterTime(Duration::from_secs(30)),
    action: InjectionAction::PartitionNetwork {
        group_a: vec![device1, device2],
        group_b: vec![device3],
    },
});
```

Triggered scenarios inject faults at specific times or protocol states.

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

## Quint Integration

Quint actions enable model-based testing. See [Verification and MBT Guide](807_verification_guide.md) for complete workflows.

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

## Conformance Workflow

Simulation feeds native/WASM conformance testing. See [Conformance and Parity Reference](119_conformance.md) for details.

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

`SimulationScenarioHandler` supports portable checkpoint snapshots:

- `export_checkpoint_snapshot(label)` exports a serializable `ScenarioCheckpointSnapshot`.
- `import_checkpoint_snapshot(snapshot)` restores a checkpoint into a fresh simulator instance.

This enables:

- Baseline checkpoint persistence for representative choreography suites.
- Restore-and-continue regression tests.
- Upgrade smoke tests that resume from pre-upgrade snapshots.

Use checkpoints when validating runtime upgrades or migration safety, not only end-to-end success.

### Scenario Contract Workflow

Conformance CI includes scenario contracts for consensus, sync, recovery, and reconfiguration. Each bundle is validated over a seed corpus for:

- Terminal status (`AllDone`).
- Required labels observed in trace.
- Minimum observable event count.

Contract results are written as JSON artifacts and CI fails on any violation with structured output.

### Shared Replay Workflow

Replay-heavy parity lanes should use shared replay APIs:

- `run_replay_shared(...)`
- `run_concurrent_replay_shared(...)`

These APIs reduce duplicate replay state across lanes and keep replay artifacts compatible with canonical trace fragments. Conformance lanes also emit deterministic replay metrics artifacts so regressions in replay footprint are visible during CI review.

## Best Practices

Start with simple scenarios. Add faults incrementally. Use deterministic seeds. Capture metrics for analysis.

Prefer TOML scenarios for human-readable tests. Prefer Quint actions for specification conformance. Combine both for comprehensive coverage.

## Related Documentation

See [Simulation Infrastructure Reference](118_simulator.md) for handler APIs. See [Verification and MBT Guide](807_verification_guide.md) for Quint workflows. See [Conformance and Parity Reference](119_conformance.md) for parity testing.
