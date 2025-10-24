# Proposal: A Simulation Engine on an Injected Effects Architecture

## 1. Motivation & Opportunity

The recent architectural refactoring of Aura to use injectable interfaces for time, randomness, and side effects is a game-changer for testing. It allows us to run the production `DeviceAgent` and protocol logic, unmodified, inside a deterministic, in-process simulation harness.

This document specifies the design for that harness: `aura-sim`. This engine will act as a specialized, local "runtime" for the side effects produced by the core logic, allowing us to simulate a multi-participant, distributed network within a single, repeatable test.

---

## 2. Core Components

The simulation engine will be composed of a few key components that work together to manage the simulated world.

### 2.1. The `Simulation` Struct

The top-level harness that owns the entire simulated world and provides the API for test scripts. In addition to running scenarios it exposes helpers (e.g., `ledger_snapshot`) so tests can inspect replica state without violating encapsulation.

```rust
// The main simulation harness
pub struct Simulation {
    // The runtime that intercepts and processes side effects
    effect_runtime: SideEffectRuntime,
    // All participants in the simulation
    participants: HashMap<DeviceId, SimulatedParticipant>,
    // A single, seeded PRNG for all deterministic randomness
    rng: ChaCha8Rng,
    // A single, controllable source of time
    time_source: SimulatedTime,
}

impl Simulation {
    /// Convenience helper for tests to inspect an honest participant's ledger view.
    pub fn ledger_snapshot(&self, participant: DeviceId) -> AccountLedger {
        self.participants
            .get(&participant)
            .expect("participant must exist")
            .agent
            .ledger()
            .clone()
    }
}
```

### 2.2. The `SimulatedParticipant`

This is a lightweight wrapper around a real `DeviceAgent`. Its key role is to instantiate the agent with the simulation's injectable components.

```rust
pub struct SimulatedParticipant {
    agent: DeviceAgent,
}

impl SimulatedParticipant {
    pub fn new(sim: &Simulation, config: AgentConfig) -> Self {
        let agent = DeviceAgent::new(
            config,
            // Inject the simulation's time source
            sim.time_source.clone(),
            // Inject a deterministic RNG derived from the main simulation seed
            sim.rng.fork(),
            // Inject the channel to send effects to the central runtime
            sim.effect_runtime.effect_sink_for(DeviceId::from(config.id.clone())),
        );
        Self { agent }
    }
}
```

### 2.3. The `SideEffectRuntime` and `SimulatedNetwork`

This is the heart of the simulation. The `SideEffectRuntime` receives `Effect`s from all participants and translates them into operations on the `SimulatedNetwork`. The network, in turn, simulates latency and partitions.

```rust
/// Central runtime that receives effects and routes them to the simulated transport.
pub struct SideEffectRuntime {
    network: SimulatedNetwork,
    scheduler: Scheduler,
}

/// Logical network fabric; no canonical oracle state.
pub struct SimulatedNetwork {
    /// Messages to deliver, keyed by delivery tick
    inflight_messages: BTreeMap<u64, Vec<Envelope>>,
    /// Per-participant transport inboxes
    peer_mailboxes: HashMap<DeviceId, Vec<Envelope>>,
    /// Partition / latency rules
    latency_range: Range<u64>,
    partitions: Vec<HashSet<DeviceId>>,
}

### 2.4. Fault Injection & Effect Interception

Malicious participants operate by intercepting the effect stream rather than patching core logic. The simulator provides deterministic hooks:

```rust
pub struct EffectContext {
    pub operation: Operation,
    pub sender: DeviceId,
    pub recipients: Vec<DeviceId>,
    pub tick: u64,
}

pub trait EffectInterceptor: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync {}
```

- `intercept_outgoing` runs before effects leave the participant. Returning `Some(effect)` forwards (possibly mutated) output; returning `None` drops it. Because the hook receives `EffectContext`, tests can match on the specific protocol phase.
- Equivalent inbound hooks (`intercept_incoming`) allow tests to drop or mangle deliveries before the agent sees them.
- Hooks are pure functions; they must not mutate shared state other than through their return value so that runs remain deterministic for a given seed.
```

---

## 3. The Simulation Loop: How It Works

Understanding the flow of information is key.

1.  **Initiation:** A test script calls a method on a `SimulatedParticipant` (e.g., `participant_a.initiate_recovery()`).

2.  **Logic Execution:** The participant's `DeviceAgent` runs its internal logic. Because time and randomness are injected and deterministic, this execution is perfectly repeatable.

3.  **Effect Production:** The agent emits `Effect`s (e.g., `Effect::Send(MessageEnvelope)`, `Effect::WriteToLocalLedger(Event)`), each tagged with the target transport semantics it intended. Registered interceptors (honest or malicious) have an opportunity to rewrite/drop these effects before they leave the participant, ensuring fault injection happens at the same boundary as production side effects. The resulting stream is then sent to the central `SideEffectRuntime`.

4.  **Effect Interpretation:** The `SideEffectRuntime` inspects the effect type:
    *   `Effect::WriteToLocalLedger(event)` is handed back to the originating participant, which applies it to its own `AccountLedger`.
    *   `Effect::Send(envelope)` is forwarded to the `SimulatedNetwork` unchanged. The runtime never fabricates broadcasts; it only executes what the agent requested (broadcast, fan-out, direct RPC, etc.).

5.  **Network Simulation:** The `SimulatedNetwork` enqueues the envelope according to latency/partition rules. The deterministically-seeded RNG picks a delivery delay and records the explicit recipient set embedded in the envelope (single peer, subset, broadcast).

6.  **Advancing Time:** The test harness advances ticks via `sim.tick()` / `sim.run_until_idle()`. The scheduler pulls due envelopes and deposits them into each recipient’s mailbox.

7.  **Message Delivery:** Each participant drains its mailbox, applying incoming deltas to its own ledger and invoking whatever protocol logic the message triggers. No shared canonical ledger exists; every replica evolves independently and converges through CRDT merges just like production.

8.  **Reaction:** Processing a message may generate new `Effect`s, returning to Step 3.

![Simulation Loop Diagram](https://i.imgur.com/r5b3e9G.png)

---

## 4. Example Test Scenario (Updated)

This demonstrates how a test script uses the simulation harness.

```rust
#[test]
fn test_byzantine_resharing_is_aborted() {
    // 1. Setup: All randomness and time are controlled by the Simulation.
    let mut sim = Simulation::new(Seed::from_u64(42));

    let alice = sim.add_participant("alice");
    let byzantine_bob = sim.add_malicious_participant("bob"); // A special participant type
    let carol = sim.add_participant("carol");

    // 2. Define Malicious Behavior
    // Tell the simulator that whenever Bob is asked to produce a resharing sub-share,
    // he should produce a corrupted one.
    byzantine_bob.intercept_outgoing(|ctx: &EffectContext, effect: Effect| {
        match effect {
            Effect::Send(mut envelope) if ctx.matches(Operation::ProduceResharingSubShare) => {
                envelope.payload = generate_corrupted_subshare();
                Some(Effect::Send(envelope))
            }
            _ => Some(effect), // fall back to default behavior
        }
    });

    // 3. Script the Scenario
    // Alice initiates a resharing protocol to add a new device.
    sim.tell(alice, Action::InitiateResharing { new_participants: vec![alice, byzantine_bob, carol] });

    // 4. Run the Simulation
    // The engine runs until no more messages are in flight.
    sim.run_until_idle();

    // 5. Assert Final State
    // Check an honest participant's ledger view to ensure the protocol was aborted.
    let alice_view = sim.ledger_snapshot(alice);
    let last_event = alice_view.get_last_event();
    assert_matches!(last_event.payload, EventPayload::ResharingAborted { reason: AbortReason::InvalidShare });

    let state = alice_view.state();
    assert!(state.is_blamed(byzantine_bob.id()));
}
```

## 5. Conclusion

By building the core Aura library with injectable interfaces for its side effects, you have unlocked a testing paradigm that is far superior to traditional integration testing.

*   **The Code Under Test is Production Code:** You are not testing mocks. You are running the actual `DeviceAgent` and protocol logic.
*   **Testing is Deterministic:** Bugs can be reproduced with 100% reliability by simply reusing the simulation seed.
*   **Complex Scenarios are Trivial to Script:** Simulating network partitions, message delays, or Byzantine faults becomes as simple as adding a few lines to a test script.

This simulation engine is the natural and powerful consequence of your new architecture. It will be the key to building a truly robust and secure distributed system.
