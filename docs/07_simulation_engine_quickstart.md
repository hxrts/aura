# Simulation Engine Quickstart

**Status**: Phase 1 Complete  
**Target Audience**: Test Authors, Protocol Developers  
**Related**: `docs/06_simulation_engine_using_injected_effects.md`, `work/SIMULATION_ENGINE_PHASE_1_COMPLETE.md`

---

## What is the Simulation Engine?

The Aura simulation engine is a deterministic, in-process testing harness that allows you to:
- Run distributed protocols in a single process
- Control time and randomness for perfect reproducibility
- Inject Byzantine faults to test protocol robustness
- Fast-forward through protocol execution
- Inspect internal state at any point

The key insight: **production code runs unmodified** - we just inject controlled effects.

---

## Basic Usage

### 1. Create a Simulation

```rust
use aura_sim::Simulation;

#[tokio::test]
async fn test_basic_protocol() {
    // Create simulation with seed for reproducibility
    let mut sim = Simulation::new(42);
    
    // Add participants
    let alice = sim.add_participant("alice").await;
    let bob = sim.add_participant("bob").await;
    let carol = sim.add_participant("carol").await;
    
    // ... run protocol ...
}
```

### 2. Control Time

```rust
// Get current logical time
let now = sim.current_timestamp().unwrap();

// Fast-forward time by 1 hour
sim.advance_time(3600).unwrap();

// Check new time
let later = sim.current_timestamp().unwrap();
assert_eq!(later, now + 3600);
```

### 3. Configure Network

```rust
// Set latency range (1-10 ticks)
sim.set_latency_range(1, 10);

// Set message drop rate (10% of messages dropped)
sim.set_drop_rate(0.1);

// Create network partition (alice isolated from others)
sim.partition_network(vec![
    vec![alice],           // Island 1
    vec![bob, carol],      // Island 2
]);

// Later, heal the partition
sim.heal_partitions();
```

### 4. Run the Simulation

```rust
// Run until no more messages or effects are pending
let ticks = sim.run_until_idle().await.unwrap();
println!("Protocol completed in {} ticks", ticks);

// Or run for a specific number of ticks
sim.run_for_ticks(100).await.unwrap();

// Or manually step through tick by tick
for _ in 0..50 {
    let delivered = sim.tick().await.unwrap();
    println!("Tick: {}, Messages delivered: {}", sim.current_tick(), delivered);
}
```

### 5. Inspect State

```rust
// Get a snapshot of a participant's ledger
let alice_ledger = sim.ledger_snapshot(alice).await.unwrap();
let state = alice_ledger.state();

// Check network statistics
let stats = sim.network_stats();
println!("Messages in flight: {}", stats.inflight_message_count);
println!("Total mailbox messages: {}", stats.total_mailbox_count);

// Check if simulation is quiescent
if sim.is_idle() {
    println!("All protocols have completed!");
}
```

---

## Byzantine Testing

### Creating a Malicious Participant

```rust
use aura_sim::{Simulation, Interceptors, byzantine, Operation};

#[tokio::test]
async fn test_malicious_dkd_commitment() {
    let mut sim = Simulation::new(42);
    
    let alice = sim.add_participant("alice").await;
    
    // Create a malicious Bob that drops all DKD commitment messages
    let bob = sim.add_malicious_participant(
        "bob",
        Interceptors::with_outgoing(byzantine::drop_operation(Operation::DkdCommitment))
    ).await;
    
    let carol = sim.add_participant("carol").await;
    
    // ... run protocol ...
    // Expected: Protocol should timeout or detect Byzantine behavior
}
```

### Common Byzantine Patterns

```rust
// 1. Silent participant (drops everything)
let silent = sim.add_malicious_participant(
    "silent",
    Interceptors::with_outgoing(byzantine::silent())
).await;

// 2. Corrupt messages for a specific operation
let corrupt = sim.add_malicious_participant(
    "corrupt",
    Interceptors::with_outgoing(byzantine::corrupt_operation(Operation::ResharingDistribution))
).await;

// 3. Crash after N ticks
let crash = sim.add_malicious_participant(
    "crash",
    Interceptors::with_outgoing(byzantine::crash_after_ticks(50))
).await;

// 4. Custom Byzantine behavior
use aura_sim::{Effect, EffectContext};

let custom = sim.add_malicious_participant(
    "custom",
    Interceptors::with_outgoing(|ctx: &EffectContext, effect: Effect| {
        // Drop every other message
        if ctx.tick % 2 == 0 {
            None // Drop
        } else {
            Some(effect) // Forward
        }
    })
).await;
```

---

## Complete Example

```rust
use aura_sim::Simulation;

#[tokio::test]
async fn test_three_party_protocol() {
    // 1. Setup
    let mut sim = Simulation::new(12345); // Fixed seed
    
    let alice = sim.add_participant("alice").await;
    let bob = sim.add_participant("bob").await;
    let carol = sim.add_participant("carol").await;
    
    // 2. Configure realistic network
    sim.set_latency_range(5, 15); // 5-15 tick latency
    sim.set_drop_rate(0.01);      // 1% packet loss
    
    // 3. Initiate protocol (stub - will be implemented in Phase 2)
    // let alice_participant = sim.get_participant(alice).unwrap();
    // alice_participant.initiate_dkd(vec![alice, bob, carol]).await.unwrap();
    
    // 4. Run simulation with timeout
    sim.run_for_ticks(1000).await.unwrap();
    
    // 5. Verify results
    let alice_ledger = sim.ledger_snapshot(alice).await.unwrap();
    let bob_ledger = sim.ledger_snapshot(bob).await.unwrap();
    let carol_ledger = sim.ledger_snapshot(carol).await.unwrap();
    
    // All participants should have same final state (CRDT convergence)
    assert_eq!(
        alice_ledger.state().session_epoch,
        bob_ledger.state().session_epoch
    );
    assert_eq!(
        bob_ledger.state().session_epoch,
        carol_ledger.state().session_epoch
    );
    
    println!("[x] Protocol completed successfully!");
}
```

---

## Advanced Features

### Reproducibility

The same seed always produces the same execution:

```rust
// Run 1
let mut sim1 = Simulation::new(42);
// ... run protocol ...
let result1 = sim1.ledger_snapshot(alice).await.unwrap();

// Run 2 (exact replay)
let mut sim2 = Simulation::new(42);
// ... run same protocol ...
let result2 = sim2.ledger_snapshot(alice).await.unwrap();

assert_eq!(result1.state(), result2.state());
```

### Debugging Failed Tests

When a test fails, re-run with the same seed to get identical behavior:

```rust
#[tokio::test]
async fn test_flaky_protocol() {
    // Found a bug with seed 9876? Use it to debug:
    let mut sim = Simulation::new(9876);
    
    // Add logging
    env_logger::init();
    
    // Step through slowly
    for i in 0..100 {
        println!("=== Tick {} ===", i);
        sim.tick().await.unwrap();
        
        // Inspect state after each tick
        let state = sim.ledger_snapshot(alice).await.unwrap();
        println!("Alice state: {:?}", state);
    }
}
```

---

## Performance Tips

1. **Use `run_until_idle()` for most tests** - it's the fastest way to completion
2. **Manual stepping is for debugging** - slower but gives you full control
3. **Keep network latency low in tests** - unless you're specifically testing timing
4. **Disable message drops in most tests** - only use for stress testing

---

## Current Limitations (Phase 1)

- Protocol integration is not yet complete (stubs in place)
- `DeviceAgent` integration pending
- Actual DKD/Resharing/Recovery protocols not wired up yet

These will be addressed in Phase 2.

---

## Testing Checklist

When writing tests with the simulation engine:

- [ ] Use a fixed seed for reproducibility
- [ ] Add honest majority for Byzantine tests
- [ ] Verify CRDT convergence (all participants reach same state)
- [ ] Test both happy path and failure scenarios
- [ ] Use `run_until_idle()` with a reasonable timeout
- [ ] Assert on final ledger state, not intermediate steps
- [ ] Test network partitions (partition then heal)
- [ ] Verify protocol timeout/abort on Byzantine majority

---

## Next Steps

1. See `work/SIMULATION_ENGINE_PHASE_1_COMPLETE.md` for implementation details
2. See `docs/06_simulation_engine_using_injected_effects.md` for architectural overview
3. Wait for Phase 2 protocol integration to write end-to-end protocol tests

---

## Questions?

The simulation engine is based on the **injectable effects** pattern documented in `docs/06_simulation_engine_using_injected_effects.md`. All production code (`DeviceAgent`, protocol coordinators) receives time and randomness as parameters, making them fully testable in this deterministic harness.

