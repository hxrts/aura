# Simulation Effect System

This directory contains the simulation-specific effect system implementations for the Aura simulator.

## Components

### `guard_interpreter.rs` - SimulationEffectInterpreter

The `SimulationEffectInterpreter` implements the `EffectInterpreter` trait for deterministic simulation of guard evaluation and effect execution. This is essential for:

- **Deterministic Testing**: All operations use a seeded RNG for reproducible results
- **Event Recording**: Every effect command is recorded as a `SimulationEvent`
- **State Inspection**: Full visibility into internal state for debugging
- **Replay Capability**: Recorded events can be replayed to reproduce exact execution

#### Key Features

1. **Deterministic Execution**
   - Uses `ChaCha8Rng` with configurable seed
   - Nonce generation is reproducible
   - Time progression is controlled

2. **State Management**
   - Flow budgets tracked per authority
   - Journal entries stored in order
   - Metadata key-value store
   - Message queue for network simulation
   - Leakage tracking for privacy analysis

3. **Event Recording**
   - `BudgetCharged`: Flow budget consumption
   - `JournalAppended`: Journal modifications
   - `LeakageRecorded`: Privacy metadata tracking
   - `MetadataStored`: Key-value updates
   - `EnvelopeQueued`: Network message simulation
   - `NonceGenerated`: Cryptographic randomness

4. **Shared State**
   - Multiple interpreters can share state via `Arc<Mutex<SimulationState>>`
   - Enables multi-party protocol simulation
   - Thread-safe concurrent access

#### Usage Example

```rust
use aura_simulator::effects::SimulationEffectInterpreter;
use aura_core::effects::guard_effects::{EffectCommand, EffectInterpreter};
use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use aura_core::effects::NetworkAddress;

// Create interpreter with deterministic seed
let interpreter = SimulationEffectInterpreter::new(
    42, // seed
    TimeStamp::now_physical(),
    AuthorityId::new(),
    NetworkAddress::from_parts("test", "node1"),
);

// Set initial conditions
interpreter.set_initial_budget(authority, 1000);

// Execute effects
let result = interpreter.execute(EffectCommand::ChargeBudget {
    authority,
    amount: 100,
}).await?;

// Inspect state
let state = interpreter.snapshot_state();
assert_eq!(state.get_budget(&authority), 900);

// Get recorded events
let events = interpreter.events();
assert_eq!(events.len(), 1);

// Replay in new interpreter
let replay = SimulationEffectInterpreter::new(42, time, authority, addr);
replay.replay(events).await?;
```

### `system.rs` - SimulationEffectSystem

The main effect system composition for simulation, providing:
- Fault injection capabilities
- Time control
- State inspection
- Deterministic network simulation

### `sync_bridge.rs` - SimulationSyncExecutor

Event-driven synchronization effects for simulation, enabling:
- Controlled message delivery
- Network partition simulation
- Latency injection

## Testing Strategies

### 1. Deterministic Unit Tests

```rust
#[tokio::test]
async fn test_deterministic_execution() {
    // Same seed produces same results
    let interp1 = SimulationEffectInterpreter::new(42, time, auth, addr);
    let interp2 = SimulationEffectInterpreter::new(42, time, auth, addr);
    
    let nonce1 = interp1.execute(GenerateNonce { bytes: 32 }).await?;
    let nonce2 = interp2.execute(GenerateNonce { bytes: 32 }).await?;
    
    assert_eq!(nonce1, nonce2);
}
```

### 2. Multi-Party Protocol Simulation

```rust
#[tokio::test]
async fn test_threshold_signing_simulation() {
    // Create shared state for multiple parties
    let state = Arc::new(Mutex::new(SimulationState::new(42, time)));
    
    let alice = SimulationEffectInterpreter::from_state(state.clone(), alice_id, alice_addr);
    let bob = SimulationEffectInterpreter::from_state(state.clone(), bob_id, bob_addr);
    let carol = SimulationEffectInterpreter::from_state(state, carol_id, carol_addr);
    
    // Simulate protocol execution...
}
```

### 3. Failure Scenario Testing

```rust
#[tokio::test]
async fn test_budget_exhaustion() {
    let interp = SimulationEffectInterpreter::new(42, time, auth, addr);
    interp.set_initial_budget(auth, 50);
    
    // Should succeed
    interp.execute(ChargeBudget { authority: auth, amount: 30 }).await?;
    
    // Should fail
    let result = interp.execute(ChargeBudget { authority: auth, amount: 30 }).await;
    assert!(result.is_err());
}
```

### 4. Event Analysis

```rust
// Filter and analyze specific event types
let leakage_events = interpreter.events_of_type(|e| {
    matches!(e, SimulationEvent::LeakageRecorded { .. })
});

let total_leakage: u32 = leakage_events.iter()
    .filter_map(|e| match e {
        SimulationEvent::LeakageRecorded { bits, .. } => Some(bits),
        _ => None,
    })
    .sum();
```

## Integration with Guard Chain

The `SimulationEffectInterpreter` is designed to work seamlessly with Aura's guard chain architecture:

1. **Authorization Guard** → Checks capabilities, produces effects
2. **Flow Budget Guard** → Ensures spam protection, charges budget
3. **Leakage Guard** → Tracks privacy metadata exposure
4. **Journal Guard** → Persists state changes

Each guard produces `EffectCommand`s that are executed by the interpreter, enabling full simulation of the guard chain behavior.

## Best Practices

1. **Always use deterministic seeds** for reproducible tests
2. **Record and assert on events** to verify execution flow
3. **Use shared state** for multi-party simulations
4. **Leverage replay** for debugging complex scenarios
5. **Inspect state snapshots** to verify invariants

## Future Extensions

- Network latency simulation
- Byzantine behavior injection
- Checkpoint/restore functionality
- Visualization of event traces
- Property-based testing integration