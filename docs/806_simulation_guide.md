# Simulation Guide

This guide covers Aura's simulation engine built on the effect system architecture. The simulation provides deterministic, reproducible testing of distributed protocols through effect handlers, fault injection, and middleware-based architecture.

## Core Simulation Philosophy

Aura's simulation approach is built on four key principles:

1. **Production Code Testing** - Run actual protocol implementations through real effect handlers
2. **Effect System Control** - All impure operations (time, randomness, I/O) controlled via effect traits
3. **Middleware Pattern** - Fault injection and monitoring via middleware layer
4. **Deterministic Execution** - Controlled effects enable fully reproducible simulations

## Two Simulation Systems

Aura has **two separate simulation systems** that serve complementary purposes:

### 1. TOML Scenario System (Human-Written Integration Tests)

```
scenarios/*.toml → aura-terminal/handlers/scenarios/execution.rs → SimulationScenarioHandler
```

| Aspect | Description |
|--------|-------------|
| **Location** | `scenarios/` directory (e.g., `scenarios/core_protocols/dkd_basic.toml`) |
| **Parser** | `aura-terminal/src/handlers/scenarios/execution.rs` |
| **Executor** | `SimulationScenarioHandler` in `aura-simulator/src/handlers/scenario.rs` |
| **Actions** | `run_choreography`, `verify_property`, `simulate_data_loss`, `apply_network_condition` |
| **Purpose** | Human-readable end-to-end integration tests with fault injection |

Example TOML scenario:
```toml
[metadata]
name = "dkd_basic_derivation"
description = "Basic P2P deterministic key derivation scenario"

[[phases]]
name = "key_derivation"
actions = [
    { type = "run_choreography", choreography = "p2p_dkd", participants = ["alice", "bob"] }
]

[[properties]]
name = "derived_keys_match"
property_type = "safety"
```

### 2. Quint Action System (Model-Based Testing)

```
verification/quint/*.qnt → ITF traces → ActionRegistry → domain_handlers.rs
```

| Aspect | Description |
|--------|-------------|
| **Location** | `crates/aura-simulator/src/quint/` |
| **Registry** | `action_registry.rs` with `ActionHandler` trait |
| **Handlers** | `domain_handlers.rs` (implements `protocol_capability_properties.qnt` actions) |
| **State** | `aura_state_extractors.rs` with `QuintSimulationState` |
| **Actions** | `initContext`, `initAuthority`, `completeTransportOp`, `attenuateToken` |
| **Purpose** | Quint-driven generative exploration and conformance testing |

### When to Use Each System

| Use Case | System |
|----------|--------|
| Human-readable integration tests | TOML Scenarios |
| End-to-end choreography testing | TOML Scenarios |
| Fault injection with named scenarios | TOML Scenarios |
| Model-based testing from formal specs | Quint Actions |
| ITF trace conformance testing | Quint Actions |
| Generative state space exploration | Quint Actions |

These systems are **not redundant** - TOML scenarios provide high-level integration tests while Quint actions enable formal verification-driven testing.

**Critical**: Simulation determinism depends on [effect system](105_effect_system_and_runtime.md) compliance. Protocol code must use effect traits (`TimeEffects`, `RandomEffects`, etc.) instead of direct system calls (`SystemTime::now()`, `thread_rng()`, etc.). This enables controlled time, seeded randomness, and predictable I/O for reliable simulation results.

The simulation system leverages Aura's stateless effect architecture, providing simulation capabilities through specialized handlers rather than a separate simulation runtime.

## Time system constraints

Simulation must never read OS clocks (`SystemTime::now()`, `Instant::now()`). All time must be supplied by effect handlers (for example, `SimulationTimeHandler` implementing `PhysicalTimeEffects`) so runs are deterministic and reproducible. Use the correct time domain for each purpose: `PhysicalClock` for wall-time semantics, `LogicalClock` for causality, `OrderClock` for privacy-preserving ordering, and `Range` for validity windows. When attested time is required, prefer `ProvenancedTime`/`TimeAttestationEffects` rather than embedding OS timestamps in simulation state.

When simulations must compare timestamps across domains, use `TimeStamp::compare(policy)` so the ordering policy is explicit and deterministic.

## Effect System Foundation

Simulation determinism requires that all simulated code uses effect traits instead of direct system calls:

```rust
// ✅ CORRECT: Protocol code using effects (simulatable)
async fn protocol_step<T: TimeEffects + RandomEffects>(
    ctx: &EffectContext,
    effects: &T,
) -> Result<ProtocolMessage> {
    let timestamp = effects.current_time().await;     // Controllable time
    let nonce = effects.random_bytes(32).await?;      // Seeded randomness
    
    ProtocolMessage { timestamp, nonce, /* ... */ }
}

// ❌ FORBIDDEN: Direct system calls (breaks simulation)
async fn broken_protocol_step() -> Result<ProtocolMessage> {
    let timestamp = SystemTime::now();                // ❌ Uncontrolled time
    let nonce = thread_rng().gen::<[u8; 32]>();      // ❌ Uncontrolled randomness
    
    ProtocolMessage { timestamp, nonce, /* ... */ }
}
```

When protocol code follows effect system guidelines, simulation handlers can control all impure operations for deterministic execution.

### Guard Chain and Simulation

Aura's guard chain uses the **GuardSnapshot pattern** which separates pure evaluation from async execution. This is particularly beneficial for simulation:

```rust
// 1. Async: Prepare snapshot (simulation handlers control time/state)
let snapshot = prepare_guard_snapshot(&ctx, &effects).await?;

// 2. Sync: Pure guard evaluation (completely deterministic, no I/O)
let commands = guard_chain.evaluate(&snapshot)?;

// 3. Async: Interpret commands (simulation handlers control effects)
for cmd in commands {
    execute_effect_command(&effects, cmd).await?;
}
```

Because guard evaluation is pure and synchronous, you can:
- Unit test guard logic without async runtime or simulation
- Verify authorization decisions deterministically
- Inject specific `GuardSnapshot` states to test edge cases

For more details, see [Testing Guide](805_testing_guide.md) and [System Architecture](001_system_architecture.md) §2.1.

### Simulation-Controlled Surfaces (must be injected)

To keep the simulator in control, code must avoid direct use of:

- Time: `SystemTime::now()`, `Instant::now()`, `tokio::time::sleep`, `std::thread::sleep`
- Randomness: `rand::random`, `thread_rng()`, `OsRng`
- IO/user input: `stdin().read_line`, direct blocking reads
- Thread/process spawn: `std::thread::spawn` in protocol/domain layers

Use the effect traits instead:

- `PhysicalTimeEffects::sleep_ms/current_time` (simulated by `SimulationTimeHandler`)
- `RandomEffects` (seeded in simulation)
- `ConsoleEffects`/test harnesses for input/output
- `TestingEffects`/scenario hooks for concurrency control

When adding retries/backoff, pass the simulator’s `SimulationTimeHandler` into retry helpers so delays advance simulated time rather than wall-clock.

## Simulation Infrastructure

Aura's simulation system is built on handler composition and middleware interception.

### Core Components

The simulator consists of several key components:

1. **Simulation Handlers** - Specialized effect handlers for simulation (time control, fault injection, etc.)
2. **Middleware System** - Intercept and modify effect calls
3. **Effect Composer** - Compose simulation handlers into complete environment
4. **Scenario System** - Define and execute test scenarios

### Handler-Based Architecture

```rust
use aura_simulator::handlers::{
    SimulationTimeHandler,
    SimulationFaultHandler,
    SimulationScenarioHandler,
    SimulationEffectComposer,
};
use aura_core::DeviceId;

// Create simulation environment using handler composition
// Use deterministic ID for reproducible tests (see docs/805_testing_guide.md)
let device_id = DeviceId::new_from_entropy([1u8; 32]);

// Compose simulation handlers (async)
let composer = SimulationEffectComposer::for_testing(device_id).await?;

// Or build custom simulation environment
let environment = composer
    .with_time_control()
    .with_fault_injection()
    .with_scenario_support()
    .build()?;
```

### Simulation Factory Abstraction

The simulator uses `SimulationEnvironmentFactory` trait (defined in `aura-core`) to decouple from `AuraEffectSystem` internals. This enables stable simulation code even when effect system implementation changes:

```rust
use aura_core::effects::{SimulationEnvironmentFactory, SimulationEnvironmentConfig};
use aura_agent::EffectSystemFactory;

// Create factory
let factory = EffectSystemFactory::new();

// Configure simulation
let config = SimulationEnvironmentConfig {
    seed: 42,
    authority_id,
    device_id: Some(device_id),
    test_mode: true,
};

// Create effect system through factory
let effects = factory.create_simulation_environment(config).await?;
```

The factory pattern ensures:
- **Decoupling**: Simulator depends on traits, not concrete types
- **Stability**: Changes to `AuraEffectSystem` only require updating the factory
- **Testability**: Factory can be mocked for isolated testing

## Simulation Handlers

### Time Control Handler

The `SimulationTimeHandler` provides deterministic time control:

```rust
use aura_simulator::handlers::SimulationTimeHandler;
use aura_core::effects::TimeEffects;
use std::time::Duration;

#[tokio::test]
async fn test_with_time_control() {
    let time_handler = SimulationTimeHandler::new();

    // Get current simulated time
    let now = time_handler.current_timestamp().await.unwrap();

    // Advance simulated time
    time_handler.advance(Duration::from_secs(10));

    let later = time_handler.current_timestamp().await.unwrap();
    assert_eq!(later - now, 10_000); // 10 seconds in milliseconds
}
```

### Fault Injection Handler

The `SimulationFaultHandler` enables Byzantine behavior and network faults:

```rust
use aura_simulator::handlers::SimulationFaultHandler;
use aura_simulator::middleware::FaultType;

#[tokio::test]
async fn test_with_fault_injection() {
    let mut fault_handler = SimulationFaultHandler::new();

    // Configure fault injection
    fault_handler.inject_fault(FaultType::NetworkDelay {
        min_ms: 100,
        max_ms: 500,
    });

    fault_handler.inject_fault(FaultType::PacketDrop {
        probability: 0.1, // 10% packet loss
    });

    // Execute protocol with faults
    // Faults will be applied through middleware
}
```

### Scenario Handler

The `SimulationScenarioHandler` manages test scenarios:

```rust
use aura_simulator::handlers::{
    SimulationScenarioHandler,
    ScenarioDefinition,
    TriggerCondition,
    InjectionAction,
};

#[tokio::test]
async fn test_with_scenario() {
    let mut scenario_handler = SimulationScenarioHandler::new();

    // Define scenario
    let scenario = ScenarioDefinition {
        name: "network_partition".to_string(),
        trigger: TriggerCondition::AfterTime(Duration::from_secs(5)),
        action: InjectionAction::PartitionNetwork {
            group_a: vec![device1, device2],
            group_b: vec![device3, device4],
        },
    };

    scenario_handler.add_scenario(scenario);

    // Scenario will trigger during protocol execution
}
```

## Middleware System

The middleware system intercepts effect calls for monitoring and modification.

### Using Middleware

```rust
use aura_simulator::middleware::{
    SimulatorMiddleware,
    SimulatorConfig,
    NetworkConfig,
};
use aura_core::DeviceId;

#[tokio::test]
async fn test_with_middleware() {
    // Use deterministic ID for reproducibility
    let device_id = DeviceId::new_from_entropy([1u8; 32]);

    let config = SimulatorConfig {
        device_id,
        network: NetworkConfig {
            latency_ms: (10, 100), // 10-100ms latency range
            packet_loss_rate: 0.02, // 2% packet loss
            bandwidth_limit: Some(1_000_000), // 1MB/s
        },
        enable_fault_injection: true,
        deterministic_time: true,
    };

    let middleware = SimulatorMiddleware::new(config)?;

    // Use middleware to wrap effect handlers
    // (middleware intercepts calls to inject faults, delays, etc.)
}
```

### Middleware Configuration

```rust
use aura_simulator::middleware::{NetworkConfig, TimeConfig};

// Configure network conditions
let network_config = NetworkConfig {
    latency_ms: (20, 200),
    packet_loss_rate: 0.01,
    bandwidth_limit: Some(10_000_000),
};

// Configure time behavior
let time_config = TimeConfig {
    start_time: 0,
    time_scale: 1.0, // Real-time
    deterministic: true,
};
```

## Complete Simulation Examples

### Basic Protocol Simulation

```rust
use aura_macros::aura_test;
use aura_simulator::handlers::SimulationEffectComposer;
use aura_simulator::middleware::FaultType;
use aura_core::DeviceId;

#[aura_test]
async fn simulate_basic_protocol() -> aura_core::AuraResult<()> {
    // Setup participants with deterministic IDs
    let device1 = DeviceId::new_from_entropy([1u8; 32]);
    let device2 = DeviceId::new_from_entropy([2u8; 32]);

    // Create simulation environments
    let env1 = SimulationEffectComposer::for_testing(device1).await?;
    let env2 = SimulationEffectComposer::for_testing(device2).await?;

    // Execute protocol
    // (protocol uses effect handlers from simulation environments)

    Ok(())
}
```

### Network Fault Simulation

```rust
use aura_simulator::middleware::{SimulatorConfig, NetworkConfig};

#[aura_test]
async fn simulate_with_network_faults() -> aura_core::AuraResult<()> {
    let device_id = DeviceId::new_from_entropy([1u8; 32]);

    // Configure realistic WAN conditions
    let config = SimulatorConfig {
        device_id,
        network: NetworkConfig {
            latency_ms: (50, 150), // 50-150ms latency
            packet_loss_rate: 0.02, // 2% packet loss
            bandwidth_limit: Some(5_000_000), // 5MB/s
        },
        enable_fault_injection: true,
        deterministic_time: true,
    };

    // Create simulation with network faults
    // ...

    Ok(())
}
```

### Time-Based Scenario Testing

```rust
use aura_simulator::handlers::{
    SimulationTimeHandler,
    SimulationScenarioHandler,
    ScenarioDefinition,
    TriggerCondition,
    InjectionAction,
};

#[aura_test]
async fn simulate_time_based_scenario() -> aura_core::AuraResult<()> {
    let time_handler = SimulationTimeHandler::new();
    let mut scenario_handler = SimulationScenarioHandler::new();

    // Define delayed fault injection
    let scenario = ScenarioDefinition {
        name: "delayed_partition".to_string(),
        trigger: TriggerCondition::AfterTime(Duration::from_secs(10)),
        action: InjectionAction::Custom("partition_network".to_string()),
    };

    scenario_handler.add_scenario(scenario);

    // Start time at 0
    let start = time_handler.current_timestamp().await?;

    // Advance time to trigger scenario
    time_handler.advance(Duration::from_secs(15));

    // Scenario should have triggered at t=10s

    Ok(())
}
```

## Testkit Integration

The simulator integrates with aura-testkit through the testkit bridge:

```rust
use aura_simulator::testkit_bridge::{
    TestkitSimulatorBridge,
    MiddlewareConfig,
};
use aura_testkit::*;

#[aura_test]
async fn test_with_simulator_bridge() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    // Bridge simulator with testkit
    let middleware_config = MiddlewareConfig {
        fault_injection: true,
        network_simulation: true,
        time_control: true,
    };

    let bridge = TestkitSimulatorBridge::new(middleware_config);

    // Use bridge to coordinate simulation with test fixture

    Ok(())
}
```

## Quint Integration

The simulator includes integration with Quint for formal verification:

```rust
use aura_simulator::quint::cli_runner::QuintCliRunner;
use aura_simulator::quint::bridge::QuintAuraBridge;

#[tokio::test]
async fn test_with_quint_verification() {
    let runner = QuintCliRunner::new("path/to/quint/spec.qnt");

    // Run Quint verification
    let traces = runner.generate_traces(100).await.unwrap();

    // Convert Quint traces to Aura execution
    let bridge = QuintAuraBridge::new();

    for trace in traces {
        let aura_scenario = bridge.convert_trace(&trace).unwrap();

        // Execute scenario in simulator
    }
}
```

## Generative Simulation

Generative simulation bridges Quint formal specifications with actual Aura effect execution. Unlike basic Quint integration that only verifies abstract properties, generative simulation executes Quint actions through real effect handlers with full state synchronization.

### Two Approaches to Model-Based Testing

There are two distinct approaches for testing Rust code against Quint specifications:

#### Approach 1: Direct Conformance Testing (Recommended for New Protocols)

ITF traces from `quint run` contain **expected states** at each step. For conformance testing, we don't need Rust handlers that re-implement Quint logic - we simply:

1. Load ITF trace with expected states
2. Parse Quint state via `QuintMappable` into Rust types
3. Apply action using **production Rust code**
4. Compare Rust result to Quint expected state

```rust
// Direct conformance pattern
#[test]
fn test_protocol_matches_quint() {
    let trace = ITFLoader::load("protocol_fast_path.itf.json")?;

    for (i, states) in trace.states.windows(2).enumerate() {
        let pre_state = ProtocolState::from_quint(&states[0])?;
        let action = states[1].action_taken.as_ref().unwrap();

        // Apply action using PRODUCTION code
        let new_state = protocol::core::apply_action(&pre_state, action)?;

        // Compare to Quint expected state
        assert_eq!(new_state.to_quint(), states[1].variables);
    }
}
```

**Advantages:**
- Tests production code directly
- No logic duplication (Quint is single source of truth)
- Simple implementation

#### Approach 2: Generative Exploration (For State Space Exploration)

For exploring state spaces where Rust drives non-deterministic choices, use the `ActionRegistry` pattern with domain handlers. This requires handlers that re-implement Quint logic in Rust.

```rust
// Generative exploration pattern
let mut registry = ActionRegistry::new();
registry.register(InitContextHandler);  // Re-implements Quint's initContext

let result = registry.execute("initContext", &params, &state).await?;
```

**When to use:**
- Exploring large state spaces with seeded randomness
- Testing Rust-specific edge cases not in Quint traces
- Fuzzing with effect execution

### Current Infrastructure

The existing `domain_handlers.rs` implements handlers for `protocol_capability_properties.qnt` specifically:

| File | Purpose | Extend for New Protocols? |
|------|---------|---------------------------|
| `itf_loader.rs` | Load ITF traces | **Reuse** |
| `aura-core/effects/quint.rs` | `QuintMappable` trait | **Extend** with new types |
| `domain_handlers.rs` | Capability property handlers | **Do not extend** - protocol-specific |
| `aura_state_extractors.rs` | `QuintSimulationState` | **Do not extend** - protocol-specific |

For new protocols (e.g., consensus), prefer **Direct Conformance Testing** to avoid duplicating Quint logic in Rust.

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Generative Simulation Flow               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Quint Spec (.qnt)                                          │
│       │                                                     │
│       ▼ quint run --mbt (Model-Based Testing)               │
│    ITF Trace (actions + nondet_picks)                       │
│       │                                                     │
│       ▼ TraceConverter                                      │
│    ExecutionPlan (Aura-typed actions)                       │
│       │                                                     │
│       ▼ GenerativeSimulator.replay_trace()                  │
│    ┌─────────────────────────────────────────┐              │
│    │         ActionRegistry Dispatch         │              │
│    │  ┌─────────┐ ┌─────────┐ ┌───────────┐  │              │
│    │  │ Journal │ │Transport│ │  Crypto   │  │              │
│    │  │ Effects │ │ Effects │ │  Effects  │  │              │
│    │  └────┬────┘ └────┬────┘ └─────┬─────┘  │              │
│    └───────┼───────────┼────────────┼────────┘              │
│            │           │            │                       │
│            ▼           ▼            ▼                       │
│    ┌──────────────────────────────────────────┐             │
│    │       Aura State (Real Handlers)         │             │
│    │  • JournalState  • TreeState             │             │
│    │  • FlowBudgetState  • GuardState         │             │
│    └──────────────────────────────────────────┘             │
│            │                                                │
│            ▼ StateMapper.aura_to_quint()                    │
│    Quint-Compatible State (for property eval)               │
│            │                                                │
│            ▼ PropertyEvaluator.check_invariants()           │
│    Verification Results                                     │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

#### GenerativeSimulator

Orchestrates generative simulation runs:

```rust
use aura_simulator::quint::generative_simulator::{
    GenerativeSimulator,
    GenerativeSimConfig,
};

// Create simulator with custom configuration
let config = GenerativeSimConfig {
    max_steps: 1000,
    check_invariants_every: 10,
    seed: Some(42), // Deterministic seed
};

let simulator = GenerativeSimulator::new(config)?;

// Replay an ITF trace through real effects
let result = simulator.replay_trace(&itf_trace).await?;

// Or explore state space with property checking
let exploration = simulator.explore(&property_spec, 500).await?;
```

#### ActionRegistry

Maps Quint action names to effect handler invocations:

```rust
use aura_simulator::quint::action_registry::{
    ActionRegistry,
    ActionHandler,
};

// Create registry with default action handlers
let mut registry = ActionRegistry::default();

// Register custom action handler
registry.register("myProtocolAction", Box::new(MyActionHandler));

// Execute action by name
let result = registry.execute(
    "initContext",
    &params,
    &effects,
).await?;
```

#### StateMapper

Bidirectional state mapping between Aura and Quint:

```rust
use aura_simulator::quint::state_mapper::StateMapper;

let mapper = StateMapper::default();

// Convert Aura state to Quint JSON for property evaluation
let quint_state = mapper.aura_to_quint(&aura_state)?;

// Update Aura state from Quint changes (for non-deterministic picks)
mapper.quint_to_aura(&quint_state, &mut aura_state)?;
```

### Trace Replay

Replay ITF traces generated by `quint run --mbt`:

```rust
use aura_simulator::quint::itf_fuzzer::ITFBasedFuzzer;

#[tokio::test]
async fn test_trace_replay() {
    let fuzzer = ITFBasedFuzzer::new();

    // Generate traces from Quint spec
    let traces = fuzzer.generate_traces(
        "verification/quint/protocol_capability_properties.qnt",
        100, // number of traces
    ).await?;

    // Replay each trace through real effects
    for trace in traces {
        let result = fuzzer.replay_trace_with_effects(&trace).await?;

        assert!(result.invariants_held, "Invariant violation: {:?}", result.violations);
    }
}
```

### State Space Exploration

Explore state space with effects and property checking:

```rust
use aura_simulator::quint::generative_simulator::GenerativeSimulator;

#[tokio::test]
async fn test_exploration() {
    let simulator = GenerativeSimulator::default();

    // Explore with effect execution
    let result = simulator.explore_with_effects(
        "verification/quint/recovery.qnt",
        500, // max steps
    ).await?;

    println!("States explored: {}", result.states_explored);
    println!("Unique states: {}", result.unique_states);
    println!("Violations: {:?}", result.violations);
}
```

### Test Case Generation

Generate validated test cases from Quint traces:

```rust
use aura_simulator::quint::itf_fuzzer::ITFBasedFuzzer;

#[tokio::test]
async fn generate_tests_from_quint() {
    let fuzzer = ITFBasedFuzzer::new();

    // Generate test cases validated through effects
    let test_cases = fuzzer.generate_validated_test_cases(
        "verification/quint/keys/dkg.qnt",
        50, // number of test cases
    ).await?;

    for test in test_cases {
        println!("Test: {}", test.name);
        println!("  Steps: {}", test.steps.len());
        println!("  Validated: {}", test.validated);
    }
}
```

### Model-Based Testing (MBT) Integration

Full MBT pipeline with Quint:

```rust
use aura_simulator::quint::itf_fuzzer::ITFBasedFuzzer;

#[tokio::test]
async fn test_mbt_pipeline() {
    let fuzzer = ITFBasedFuzzer::new();

    // Run complete MBT pipeline:
    // 1. Generate traces via `quint run --mbt`
    // 2. Replay through real effects
    // 3. Validate properties
    // 4. Generate test cases
    let result = fuzzer.run_mbt_with_effects(
        "verification/quint/authorization.qnt",
        100, // trace count
    ).await?;

    println!("Traces executed: {}", result.traces_executed);
    println!("Properties verified: {}", result.properties_verified);
    println!("Test cases generated: {}", result.test_cases.len());
}
```

### Non-Deterministic Choice Handling

Quint `nondet_picks` are wired to seeded `RandomEffects`:

```rust
use aura_simulator::quint::generative_simulator::GenerativeSimulator;

// Simulator seeds RandomEffects from ITF nondet_picks
// This ensures deterministic replay of non-deterministic choices

let config = GenerativeSimConfig {
    seed: Some(42), // Base seed
    ..Default::default()
};

let simulator = GenerativeSimulator::new(config)?;

// Each nondet_pick in the trace deterministically seeds
// the corresponding RandomEffects call
let result = simulator.replay_trace(&trace).await?;
```

### Available Quint Specs

The following Quint specs are available for generative simulation:

| Spec | Purpose | Location |
|------|---------|----------|
| `authorization.qnt` | Guard chain security | `verification/quint/` |
| `core.qnt` | Protocol lifecycle | `verification/quint/` |
| `recovery.qnt` | Guardian recovery | `verification/quint/` |
| `keys/dkg.qnt` | DKG ceremony | `verification/quint/` |
| `journal/core.qnt` | Journal CRDT | `verification/quint/` |
| `sessions/core.qnt` | Session management | `verification/quint/` |
| `sessions/groups.qnt` | Group membership | `verification/quint/` |
| `sbb.qnt` | Gossip transport | `verification/quint/` |

See `verification/quint/` for spec details and invariants.

## Simulation Best Practices

### Start Simple

Begin with basic handler composition before adding fault injection:

```rust
#[aura_test]
async fn test_simple_simulation() -> aura_core::AuraResult<()> {
    // Start with just time control
    let time_handler = SimulationTimeHandler::new();

    // Execute protocol
    // ...

    // Add faults incrementally in later tests
    Ok(())
}
```

### Use Deterministic Configuration

Always use deterministic settings for debugging:

```rust
let config = SimulatorConfig {
    device_id: DeviceId::new_from_entropy([1u8; 32]), // Deterministic for reproducibility
    network: NetworkConfig::default(),
    enable_fault_injection: false,
    deterministic_time: true, // Critical for reproducibility
};
```

### Validate Effect System Compliance

Before running simulations, ensure protocol code follows effect guidelines:

```bash
# Run architectural compliance checker
just check-arch

# Look for effect system violations
# - Direct time usage: SystemTime::now, Instant::now
# - Direct randomness: thread_rng(), rand::random()
# - Direct I/O: File::open(), std::net::TcpStream
```

Non-compliant code will break simulation determinism and should be refactored to use effect traits.

### Monitor Simulation State

Track simulation progress for debugging:

```rust
use aura_simulator::middleware::PerformanceMetrics;

let metrics = middleware.get_metrics();

println!("Messages sent: {}", metrics.messages_sent);
println!("Faults injected: {}", metrics.faults_injected);
println!("Simulation time: {:?}", metrics.simulation_duration);
```

## Limitations and Architecture Notes

### Current Architecture

The Aura simulator uses a **handler/middleware pattern** rather than a monolithic simulation engine. This means:

1. **No Central Simulation Engine**: Unlike some frameworks, there's no single `Simulator` object that owns all state
2. **Distributed Handler Model**: Each participant uses their own effect handlers
3. **Middleware for Coordination**: Cross-cutting concerns (network simulation, fault injection) are handled via middleware

### Differences from Documented Examples

Previous documentation described an `AsyncSimulationEngine` API with methods like:
- `add_participants(count)` - Not implemented
- `add_byzantine_participant(interceptor)` - Use `SimulationFaultHandler` instead
- `run_until_idle()` - Not available; tests control execution explicitly

### Current Best Practices

Instead of a simulation engine, use handler composition:

```rust
// Instead of: sim.add_participants(5)
// Use deterministic IDs for reproducibility:
let mut participants = Vec::new();
for i in 0..5 {
    let device_id = DeviceId::new_from_entropy([i as u8 + 1; 32]);
    participants.push(SimulationEffectComposer::for_testing(device_id).await?);
}

// Instead of: sim.add_byzantine_participant(interceptor)
// Use:
let mut fault_handler = SimulationFaultHandler::new();
fault_handler.inject_fault(FaultType::ByzantineBehavior {
    corruption_rate: 0.5,
});

// Instead of: sim.run_until_idle()
// Use explicit protocol execution:
execute_protocol(&participants).await?;
```

## Summary

Aura's simulation infrastructure provides:

- **Handler-Based Architecture** - Simulation through composable effect handlers
- **Middleware System** - Fault injection and monitoring via middleware
- **Deterministic Execution** - Controlled time and seeded randomness
- **Quint Integration** - Formal verification support
- **Testkit Bridge** - Integration with test infrastructure

The simulation emphasizes composition and explicit control rather than implicit coordination. This aligns with Aura's stateless effect architecture and provides flexibility for different testing scenarios.

For testing infrastructure that complements simulation, see [Testing Guide](805_testing_guide.md). Learn about the [effect system](105_effect_system_and_runtime.md) and [architectural patterns](001_system_architecture.md).
