# Quint-Driven Chaos Testing Integration Guide

This guide explains how to integrate Quint formal specifications with Aura's chaos testing framework for automated scenario generation and property verification.

## Overview

The Quint integration enables:
- **Formal Specification**: Define protocols using Quint's TLA+-based specification language
- **Automated Scenario Generation**: Generate chaos testing scenarios from Quint specifications
- **Property Verification**: Verify temporal logic properties during simulation execution
- **Byzantine Behavior Modeling**: Model and test byzantine fault scenarios
- **Trace Analysis**: Analyze execution traces against formal specifications

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Quint Integration Layer                      │
├─────────────────────────────────────────────────────────────────┤
│  Quint Specs  │  Bridge    │  Generator  │  Verifier  │ Mapper  │
│  (.qnt files) │  Component │  Engine     │  Engine    │ System  │
└─────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                  Aura Simulation Framework                      │
├─────────────────────────────────────────────────────────────────┤
│  Scenario     │  Execution  │  Property   │  Chaos     │ Debug  │
│  Engine       │  Runtime    │  Monitor    │  Injection │ Tools  │
└─────────────────────────────────────────────────────────────────┘
```

## Quick Start

### 1. Install Quint

```bash
npm install -g @informalsystems/quint
```

### 2. Create a Quint Specification

Create `specs/quint/threshold_signing.qnt`:

```scala
module threshold_signing {
  // State variables
  var participants: Set[PARTICIPANT]
  var signatures: PARTICIPANT -> Option[SIGNATURE]
  var threshold: Int
  var aggregated_signature: Option[SIGNATURE]

  // Constants
  const PARTICIPANTS: Set[PARTICIPANT]
  const THRESHOLD: Int

  // Initialization
  action init = all {
    participants' = PARTICIPANTS,
    signatures' = PARTICIPANTS.mapBy(_ => None),
    threshold' = THRESHOLD,
    aggregated_signature' = None
  }

  // Actions
  action sign(p: PARTICIPANT, sig: SIGNATURE) = all {
    p.in(participants),
    signatures'[p] = Some(sig),
    // ... other state updates
  }

  action aggregate = all {
    signatures.values().filter(s => s != None).size() >= threshold,
    aggregated_signature' = Some(/* aggregation logic */),
    // ...
  }

  // Invariants
  val ValidThreshold = threshold >= 1 and threshold <= participants.size()
  val NoDoubleSignature = participants.forall(p => signatures[p].isSome() implies /* uniqueness */)

  // Temporal properties
  temporal AggregatableEventually = always(
    signatures.values().filter(s => s != None).size() >= threshold implies eventually(aggregated_signature != None)
  )

  temporal SafetyProperty = always(
    aggregated_signature != None implies signatures.values().filter(s => s != None).size() >= threshold
  )
}
```

### 3. Generate Scenarios from Quint Specs

```bash
# Generate scenarios from Quint specification
aura scenarios generate \
  --spec specs/quint/threshold_signing.qnt \
  --module threshold_signing \
  --output scenarios/quint_generated/ \
  --scenario-count 10 \
  --chaos-enabled
```

### 4. Run Generated Scenarios

```bash
# Run all generated scenarios
aura scenarios run --directory scenarios/quint_generated/

# Run with specific properties
aura scenarios run \
  --directory scenarios/quint_generated/ \
  --verify-properties \
  --property-checker quint
```

## Specification Writing Guide

### Basic Structure

```scala
module protocol_name {
  // State variables - represent system state
  var state_var1: TYPE1
  var state_var2: TYPE2

  // Constants - immutable parameters
  const CONSTANT1: TYPE1
  const CONSTANT2: TYPE2

  // Initialization predicate
  action init = all {
    state_var1' = initial_value1,
    state_var2' = initial_value2
  }

  // Protocol actions
  action action_name(params) = all {
    // Preconditions
    precondition1,
    precondition2,
    // State updates
    state_var1' = new_value1,
    state_var2' = new_value2
  }

  // Invariants - always true
  val InvariantName = condition

  // Temporal properties
  temporal PropertyName = temporal_logic_formula
}
```

### Modeling Byzantine Behavior

```scala
module byzantine_protocol {
  // Participant types
  type PARTICIPANT_TYPE = "honest" | "byzantine"

  var participants: PARTICIPANT -> PARTICIPANT_TYPE
  var messages: Set[MESSAGE]
  var byzantine_strategy: BYZANTINE_STRATEGY

  // Byzantine actions
  action byzantine_send_conflicting(p: PARTICIPANT, msgs: Set[MESSAGE]) = all {
    participants[p] == "byzantine",
    // Send conflicting messages
    messages' = messages.union(msgs),
    // ... byzantine logic
  }

  // Safety despite byzantine behavior
  val ByzantineSafety =
    participants.filter(p => participants[p] == "byzantine").size() <= threshold implies
    safety_property_holds
}
```

### Property Types

#### Safety Invariants
```scala
// Something bad never happens
val SafetyProperty = always(bad_condition.not())

// State consistency
val ConsistencyProperty = always(
  forall(p1, p2 in honest_participants: state[p1] == state[p2])
)
```

#### Liveness Properties
```scala
// Something good eventually happens
temporal LivenessProperty = always(
  precondition implies eventually(good_condition)
)

// Progress guarantee
temporal ProgressProperty = always(
  request_made implies eventually(response_received)
)
```

#### Fairness Properties
```scala
// Fair scheduling
temporal FairnessProperty = always(
  infinitely_often(participant_gets_turn(p)) for all p in participants
)
```

## Configuration

### Quint Bridge Configuration

Create `aura-config.toml`:

```toml
[quint]
# Quint executable path
quint_path = "/usr/local/bin/quint"

# Specification directory
spec_directory = "specs/quint"

# Generation settings
[quint.generation]
default_scenario_count = 5
max_trace_length = 100
property_checking_enabled = true
byzantine_modeling_enabled = true

# Mapping configuration
[quint.mapping]
# Map Quint types to Rust types
type_mappings = [
  { quint = "PARTICIPANT", rust = "ParticipantId" },
  { quint = "SIGNATURE", rust = "Signature" },
  { quint = "MESSAGE", rust = "ProtocolMessage" }
]

# Map Quint actions to protocol operations
action_mappings = [
  { quint = "sign", protocol = "frost_sign" },
  { quint = "aggregate", protocol = "frost_aggregate" }
]
```

### Scenario Generation Configuration

```toml
[generation]
# Trace-based scenario generation
trace_based_generation = true
max_traces_per_scenario = 10

# Chaos injection settings
chaos_intensity = 0.3
byzantine_participant_ratio = 0.2

# Property verification
verify_safety_properties = true
verify_liveness_properties = true
verify_fairness_properties = false

[generation.strategies]
# Generation strategies
enabled_strategies = [
  "exhaustive_small_traces",
  "random_walk",
  "targeted_property_violation",
  "byzantine_behavior_exploration"
]
```

## CLI Commands

### Generate Scenarios

```bash
# Basic generation
aura scenarios generate --spec threshold_signing.qnt

# Advanced generation with options
aura scenarios generate \
  --spec threshold_signing.qnt \
  --module threshold_signing \
  --traces 20 \
  --max-length 50 \
  --byzantine-ratio 0.3 \
  --output scenarios/generated/ \
  --chaos-intensity 0.4
```

### Verify Properties

```bash
# Verify properties during execution
aura scenarios run \
  --directory scenarios/ \
  --verify-properties \
  --property-checker quint \
  --check-invariants \
  --check-temporal

# Debug property violations
aura debug analyze \
  --violation property_violation.json \
  --spec threshold_signing.qnt \
  --trace-analysis
```

### Analyze Traces

```bash
# Convert execution traces to Quint format
aura quint trace-convert \
  --execution-trace execution.json \
  --spec threshold_signing.qnt \
  --output quint_trace.itf

# Verify trace against specification
quint verify \
  --spec threshold_signing.qnt \
  --trace quint_trace.itf \
  --invariant SafetyProperty
```

## Integration Patterns

### Pattern 1: Protocol Validation

```scala
// Define protocol specification
module dkd_protocol {
  var alice_key: Option[KEY]
  var bob_key: Option[KEY]
  var context: CONTEXT

  action derive_key(participant: PARTICIPANT, ctx: CONTEXT) = all {
    context == ctx,
    participant == "alice" implies alice_key' = Some(derive(ctx)),
    participant == "bob" implies bob_key' = Some(derive(ctx))
  }

  val KeyConsistency = always(
    alice_key != None and bob_key != None implies alice_key == bob_key
  )
}
```

```bash
# Generate DKD test scenarios
aura scenarios generate --spec dkd_protocol.qnt --focus-property KeyConsistency
```

### Pattern 2: Byzantine Fault Testing

```scala
module byzantine_consensus {
  var votes: PARTICIPANT -> Option[VOTE]
  var decision: Option[DECISION]
  var participant_type: PARTICIPANT -> PARTICIPANT_TYPE

  action honest_vote(p: PARTICIPANT, v: VOTE) = all {
    participant_type[p] == "honest",
    votes'[p] = Some(v)
  }

  action byzantine_vote(p: PARTICIPANT, v: VOTE) = all {
    participant_type[p] == "byzantine",
    // Byzantine participant can vote anything
    votes'[p] = Some(v)
  }

  val Agreement = always(
    decision != None implies
    forall(p1, p2 in honest_participants: votes[p1] == votes[p2])
  )
}
```

### Pattern 3: Performance Analysis

```scala
module performance_model {
  var message_count: Int
  var latency: Int
  var throughput: Real

  action send_message = all {
    message_count' = message_count + 1,
    latency' = latency + network_delay,
    throughput' = message_count' / time
  }

  val PerformanceBound = always(
    latency <= MAX_LATENCY and throughput >= MIN_THROUGHPUT
  )
}
```

## Debugging Quint Integration Issues

### Common Issues

1. **Type Mapping Errors**
   ```bash
   # Check type compatibility
   aura quint check-types --spec protocol.qnt --rust-types types.rs
   ```

2. **Property Violations**
   ```bash
   # Analyze property violations
   aura debug analyze --violation violation.json --quint-spec protocol.qnt
   ```

3. **Trace Conversion Errors**
   ```bash
   # Validate trace format
   aura quint validate-trace --trace execution.json --spec protocol.qnt
   ```

### Debugging Workflow

1. **Validate Specification**
   ```bash
   quint parse specs/quint/protocol.qnt
   quint typecheck specs/quint/protocol.qnt
   ```

2. **Test Scenario Generation**
   ```bash
   aura scenarios generate --spec protocol.qnt --dry-run --verbose
   ```

3. **Verify Property Mapping**
   ```bash
   aura quint map-properties --spec protocol.qnt --show-mapping
   ```

4. **Debug Execution**
   ```bash
   aura scenarios run --quint-debug --trace-level verbose
   ```

## Best Practices

### Specification Design

1. **Start Simple**: Begin with basic safety properties, then add liveness
2. **Modular Design**: Split complex protocols into smaller modules
3. **Clear Naming**: Use descriptive names for states, actions, and properties
4. **Documentation**: Comment complex temporal logic formulas

### Scenario Generation

1. **Property-Focused**: Generate scenarios targeting specific properties
2. **Incremental Complexity**: Start with simple cases, increase complexity
3. **Byzantine Coverage**: Include various byzantine behavior patterns
4. **Performance Bounds**: Include performance-related properties

### Testing Strategy

1. **Smoke Tests**: Quick validation of basic functionality
2. **Property Tests**: Systematic verification of all specified properties
3. **Stress Tests**: Test under high load and chaos conditions
4. **Byzantine Tests**: Comprehensive byzantine fault tolerance testing

## Advanced Topics

### Custom Property Checkers

```rust
// Implement custom property checker
pub struct CustomPropertyChecker {
    spec: QuintSpec,
    state_mapper: StateMapper,
}

impl PropertyChecker for CustomPropertyChecker {
    fn check_property(&self, state: &SimulationState, property: &Property) -> bool {
        // Custom property checking logic
        let quint_state = self.state_mapper.map_state(state);
        self.spec.evaluate_property(property, &quint_state)
    }
}
```

### Trace-Based Testing

```rust
// Generate scenarios from Quint traces
pub fn generate_from_traces(spec: &QuintSpec, traces: Vec<QuintTrace>) -> Vec<Scenario> {
    traces.into_iter()
        .map(|trace| {
            let scenario = ScenarioBuilder::new()
                .from_quint_trace(trace)
                .with_chaos_injection()
                .build();
            scenario
        })
        .collect()
}
```

## Integration Examples

See `scenarios/` directory for complete examples:
- `scenarios/quint_generated/` - Auto-generated scenarios
- `scenarios/templates/quint_template.toml` - Quint-based scenario template
- `scenarios/integration/quint_dkd_integration.toml` - DKD protocol integration

For more advanced usage, refer to the [Aura Simulation Engine Documentation](simulation_engine_guide.md) and [Chaos Testing Strategies](chaos_testing_guide.md).
