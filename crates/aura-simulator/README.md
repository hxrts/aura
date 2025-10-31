# Aura Simulator

The Aura Simulator is a comprehensive testing and validation framework for the Aura threshold identity and encrypted storage platform. It provides advanced simulation capabilities for distributed protocols, Byzantine fault tolerance testing, and property verification.

## Features Overview

### Core Simulation Engine
- Unified Scenario Engine: Declarative TOML-based scenario execution
- Choreographic Programming: Session-typed protocol coordination
- Time Travel Debugging: Checkpoint-based simulation state management
- Property Monitoring: Real-time invariant and temporal property checking

### Advanced Analysis Tools
- Failure Analysis: Causal chain identification and root cause analysis
- Minimal Reproduction: Automated discovery of minimal failure conditions
- Focused Testing: Systematic exploration of failure boundaries
- Debug Reporting: Comprehensive developer reports with actionable insights

### Security Testing
- Byzantine Fault Tolerance: Comprehensive adversarial testing
- Network Partition Simulation: Realistic network failure scenarios
- Attack Scenario Modeling: Coordinated attack simulations
- Invariant Verification: Session type compliance and safety properties

## Quick Start

### Basic Scenario Execution

```rust
use aura_simulator::{
    UnifiedScenarioEngine, UnifiedScenarioLoader, 
    choreography_actions::register_standard_choreographies
};

// Create scenario engine
let mut engine = UnifiedScenarioEngine::new("./temp")?;
register_standard_choreographies(&mut engine);

// Load and execute a scenario
let loader = UnifiedScenarioLoader::new("./scenarios");
let scenario = loader.load_scenario("core_protocols/dkd_basic.toml")?;
let result = engine.execute_scenario(&scenario)?;

println!("Scenario completed: {}", result.success);
```

### Property Monitoring

```rust
use aura_simulator::testing::PropertyMonitor;

// Create property monitor
let mut monitor = PropertyMonitor::new();

// Add invariant properties
monitor.add_invariant("threshold_security", |state| {
    state.participants.iter()
        .filter(|p| p.has_key_shares)
        .count() >= state.threshold
});

// Monitor during simulation
let violations = monitor.check_properties(&simulation_state)?;
```

## Core Architecture

The simulator uses a unified scenario engine with TOML-based configuration. Scenarios define participants, network conditions, protocols, and properties to verify.

```rust
// Basic engine setup
let mut engine = UnifiedScenarioEngine::new("./temp")?;
register_standard_choreographies(&mut engine);

// Load and execute scenarios
let loader = UnifiedScenarioLoader::new("./scenarios");
let scenario = loader.load_scenario("core_protocols/dkd_basic.toml")?;
let result = engine.execute_scenario(&scenario)?;
```

## Key Features

### Property Monitoring
Monitor invariant, temporal, and safety properties during simulation execution.

### Failure Analysis
Automated causal chain analysis and root cause identification for property violations.

### Minimal Reproduction
Discover the simplest conditions that reproduce failures for easier debugging.

### Time Travel Debugging
Checkpoint-based simulation state management for failure investigation.

## Testing Capabilities

### Byzantine Fault Tolerance
```toml
[scenario]
extends = "threshold_protocol_template"
mixins = ["byzantine_tolerance_mixin"]

[mixin_params]
byzantine_strategies = ["delay_attack", "equivocation"]
attack_intensity = 0.7
```

### Network Partitions
```toml
[network]
type = "adversarial"
partition_scenarios = [
    { participants = [0, 1], duration_ms = 5000 },
    { participants = [2, 3, 4], duration_ms = 3000 }
]
```

## Example Scenarios

The simulator includes scenarios in `scenarios/` directory:
- Core Protocols: DKD, FROST key generation, session management
- Invariant Testing: Safety properties, CRDT convergence, threshold security
- Adversarial Testing: Byzantine attacks, replay prevention
- Integration Testing: Full account lifecycle, multi-protocol coordination

## Usage Guidelines

1. Start with scenario templates for common patterns
2. Use mixins to compose reusable behaviors
3. Define relevant safety and liveness properties
4. Enable failure analysis for systematic debugging
5. Include Byzantine and network failure scenarios

The Aura Simulator provides comprehensive testing for distributed protocols with advanced analysis capabilities, enabling systematic validation of correctness, security, and performance properties.