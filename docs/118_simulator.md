# Simulation Infrastructure Reference

This document describes the architecture of `aura-simulator`, the simulation crate that provides deterministic protocol testing through effect handler composition, fault injection, and scenario execution.

## Overview

The `aura-simulator` crate occupies Layer 6 in the Aura architecture. It enables testing distributed protocols under controlled conditions. The simulator uses a handler-based architecture rather than a monolithic simulation engine.

The crate provides four capabilities. It offers specialized effect handlers for simulation control. It includes a middleware system for fault injection. It supports TOML-based scenario definitions. It integrates with Quint for model-based testing.

## Handler-Based Architecture

The simulator composes effect handlers rather than wrapping them in a central engine. Each simulated participant uses its own handler instances. This approach aligns with Aura's stateless effect architecture.

```mermaid
graph TD
    A[Protocol Code] --> B[Effect Traits]
    B --> C[SimulationTimeHandler]
    B --> D[SimulationFaultHandler]
    B --> E[Other Handlers]
    C --> F[Simulated State]
    D --> F
    E --> F
```

Handlers implement effect traits from `aura-core`. Protocol code calls effect methods without knowing whether handlers are production or simulation instances.

## Simulation Handlers

### SimulationTimeHandler

This handler provides deterministic time control.

```rust
use aura_simulator::handlers::SimulationTimeHandler;
use std::time::Duration;

let time = SimulationTimeHandler::new();
time.advance(Duration::from_secs(10));
let now = time.current_timestamp().await?;
```

Time starts at zero and advances only through explicit calls. The `sleep_ms` method returns immediately after advancing simulated time. This enables testing timeout behavior without wall-clock delays.

### SimulationFaultHandler

This handler injects faults into protocol execution.

```rust
use aura_simulator::handlers::SimulationFaultHandler;
use aura_simulator::middleware::FaultType;

let mut faults = SimulationFaultHandler::new();
faults.inject_fault(FaultType::NetworkDelay {
    min_ms: 100,
    max_ms: 500,
});
faults.inject_fault(FaultType::PacketDrop {
    probability: 0.1,
});
```

Fault types include network delays, packet drops, message corruption, and Byzantine behavior. Faults apply probabilistically based on configuration.

### SimulationScenarioHandler

This handler manages scenario-driven testing.

```rust
use aura_simulator::handlers::{
    SimulationScenarioHandler,
    ScenarioDefinition,
    TriggerCondition,
    InjectionAction,
};

let mut scenarios = SimulationScenarioHandler::new();
scenarios.add_scenario(ScenarioDefinition {
    name: "partition".to_string(),
    trigger: TriggerCondition::AfterTime(Duration::from_secs(5)),
    action: InjectionAction::PartitionNetwork {
        group_a: vec![device1, device2],
        group_b: vec![device3],
    },
});
```

Scenarios define triggered actions based on time or protocol state. They enable testing recovery from transient failures.

### SimulationEffectComposer

This type composes handlers into complete simulation environments.

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

The composer provides a builder pattern for handler configuration. It produces an effect system instance suitable for simulation.

## TOML Scenario System

TOML scenarios provide human-readable integration tests with fault injection.

### File Format

Scenario files live in the `scenarios/` directory.

```toml
[metadata]
name = "dkd_basic_derivation"
description = "Basic P2P deterministic key derivation"
version = "1.0"

[[phases]]
name = "setup"
actions = [
    { type = "create_participant", id = "alice" },
    { type = "create_participant", id = "bob" },
]

[[phases]]
name = "derivation"
actions = [
    { type = "run_choreography", choreography = "p2p_dkd", participants = ["alice", "bob"] },
]

[[phases]]
name = "verification"
actions = [
    { type = "verify_property", property = "derived_keys_match" },
]

[[properties]]
name = "derived_keys_match"
property_type = "safety"
expression = "alice.derived_key == bob.derived_key"
```

Each scenario has metadata, ordered phases, and property definitions. Phases contain action sequences that execute in order.

### Action Types

| Action | Parameters | Description |
|--------|------------|-------------|
| `create_participant` | `id` | Create a simulated participant |
| `run_choreography` | `choreography`, `participants` | Execute a choreographic protocol |
| `verify_property` | `property` | Check a named property |
| `simulate_data_loss` | `participant`, `percentage` | Delete random stored data |
| `apply_network_condition` | `condition`, `duration` | Apply network fault |
| `advance_time` | `duration` | Advance simulated time |

### Execution

The `SimulationScenarioHandler` executes TOML scenarios.

```rust
use aura_simulator::handlers::SimulationScenarioHandler;

let handler = SimulationScenarioHandler::new();
let result = handler.execute_file("scenarios/core_protocols/dkd_basic.toml").await?;
assert!(result.all_properties_passed());
```

Execution proceeds phase by phase. Failures stop execution and report the failing action.

## Middleware System

The middleware system intercepts effect calls for monitoring and modification.

### SimulatorMiddleware

```rust
use aura_simulator::middleware::{SimulatorMiddleware, SimulatorConfig, NetworkConfig};

let config = SimulatorConfig {
    device_id: DeviceId::new_from_entropy([1u8; 32]),
    network: NetworkConfig {
        latency_ms: (10, 100),
        packet_loss_rate: 0.02,
        bandwidth_limit: Some(1_000_000),
    },
    enable_fault_injection: true,
    deterministic_time: true,
};
let middleware = SimulatorMiddleware::new(config)?;
```

Middleware wraps effect handlers to inject delays, drop messages, and record metrics.

### NetworkConfig

Network configuration controls simulated network conditions.

| Field | Type | Description |
|-------|------|-------------|
| `latency_ms` | `(u64, u64)` | Min and max latency range |
| `packet_loss_rate` | `f64` | Probability of dropping messages |
| `bandwidth_limit` | `Option<u64>` | Bytes per second limit |
| `partition_groups` | `Vec<Vec<DeviceId>>` | Network partition configuration |

### PerformanceMetrics

Middleware collects execution metrics.

```rust
let metrics = middleware.get_metrics();
println!("Messages: {}", metrics.messages_sent);
println!("Faults: {}", metrics.faults_injected);
println!("Duration: {:?}", metrics.simulation_duration);
```

Metrics help identify performance issues and verify fault injection behavior.

## Async Host Boundary

The `AsyncSimulatorHostBridge` provides an async request/resume interface for Telltale integration.

### Design

```rust
use aura_simulator::{AsyncHostRequest, AsyncSimulatorHostBridge};

let mut host = AsyncSimulatorHostBridge::new(42);
host.submit(AsyncHostRequest::VerifyAllProperties);
let entry = host.resume_next().await?;
```

The bridge maintains deterministic ordering through FIFO processing and monotone sequence IDs.

### Determinism Constraints

The async host boundary enforces several constraints. Requests process in submission order. Each request receives a unique monotone sequence ID. No wall-clock time affects host decisions. Transcript entries enable replay comparison.

### Transcript Artifacts

```rust
use aura_simulator::AsyncHostTranscriptEntry;

let entry = host.resume_next().await?;
assert_eq!(entry.sequence, 0);
assert!(entry.request.is_verify_properties());
```

Transcript entries record request/response pairs. They enable sync-versus-async host parity testing.

## Factory Abstraction

The `SimulationEnvironmentFactory` trait decouples simulation from effect system internals.

```rust
use aura_core::effects::{SimulationEnvironmentFactory, SimulationEnvironmentConfig};

let config = SimulationEnvironmentConfig {
    seed: 42,
    authority_id,
    device_id: Some(device_id),
    test_mode: true,
};
let effects = factory.create_simulation_environment(config).await?;
```

This abstraction enables stable simulation code across effect system changes. Only the factory implementation requires updates when internals change.

## Quint Integration

The `quint` module provides integration with Quint formal specifications. See [Formal Verification Reference](119_verification.md) for complete details.

### ITF Trace Format

ITF (Informal Trace Format) traces come from Quint model checking. Each trace captures a sequence of states and transitions.

```json
{
  "#meta": {
    "format": "ITF",
    "source": "quint",
    "version": "1.0"
  },
  "vars": ["phase", "participants", "messages"],
  "states": [
    {
      "#meta": { "index": 0 },
      "phase": "Setup",
      "participants": [],
      "messages": []
    },
    {
      "#meta": { "index": 1, "action": "addParticipant" },
      "phase": "Setup",
      "participants": ["alice"],
      "messages": []
    }
  ]
}
```

Each state represents a model state. Transitions between states correspond to actions.

ITF traces capture non-deterministic choices for replay:

```json
{
  "#meta": {
    "index": 3,
    "action": "selectLeader",
    "nondet_picks": { "leader": "bob" }
  }
}
```

The `nondet_picks` field records choices made by Quint. Replay uses these values to seed `RandomEffects`.

### ITFLoader

```rust
use aura_simulator::quint::itf_loader::ITFLoader;

let trace = ITFLoader::load("trace.itf.json")?;
for (i, state) in trace.states.iter().enumerate() {
    let action = state.meta.action.as_deref();
    let picks = &state.meta.nondet_picks;
}
```

The loader validates trace format and extracts typed state.

### GenerativeSimulator

```rust
use aura_simulator::quint::generative_simulator::GenerativeSimulator;

let simulator = GenerativeSimulator::new(config)?;
let result = simulator.replay_trace(&trace).await?;
```

The generative simulator replays ITF traces through real effect handlers.

## Module Structure

```
aura-simulator/
├── src/
│   ├── handlers/           # Simulation effect handlers
│   │   ├── time_control.rs
│   │   ├── fault_simulation.rs
│   │   ├── scenario.rs
│   │   └── effect_composer.rs
│   ├── middleware/         # Effect interception
│   ├── quint/              # Quint integration
│   │   ├── itf_loader.rs
│   │   ├── action_registry.rs
│   │   ├── state_mapper.rs
│   │   └── generative_simulator.rs
│   ├── scenarios/          # Scenario execution
│   ├── async_host.rs       # Async host boundary
│   └── testkit_bridge.rs   # Testkit integration
├── tests/                  # Integration tests
└── examples/               # Usage examples
```

## Related Documentation

See [Simulation Guide](806_simulation_guide.md) for how to write simulations. See [Testing Guide](805_testing_guide.md) for conformance testing. See [Formal Verification Reference](119_verification.md) for Quint integration details.
