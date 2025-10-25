# 080 · Choreographic Protocol Integration

**Status:** Integration Specification  
**Target:** Phase 2 (Session-Type Architecture)

## Implementation Status

| Component | Status | Location | Notes |
|-----------|--------|----------|-------|
| **Core Runtime Components** |
| Instruction Set | [VERIFIED] IMPLEMENTED | `crates/coordination/src/execution/types.rs` | Complete with all documented instructions + extensions |
| ProtocolContext | [VERIFIED] IMPLEMENTED | `crates/coordination/src/execution/context.rs` | Full instruction interpreter with error handling |
| TimeSource (Production) | [VERIFIED] IMPLEMENTED | `crates/coordination/src/execution/time.rs` | ProductionTimeSource with tokio::Notify |
| TimeSource (Simulation) | [VERIFIED] IMPLEMENTED | `crates/coordination/src/execution/time.rs` | Deterministic with SimulationScheduler |
| Event Watching | [VERIFIED] IMPLEMENTED | Integrated in ProtocolContext | Event filtering and threshold collection |
| **Protocol Choreographies** |
| DKD | [VERIFIED] IMPLEMENTED | `crates/coordination/src/choreography/dkd.rs` | Complete 4-phase choreography |
| Resharing | [VERIFIED] IMPLEMENTED | `crates/coordination/src/choreography/resharing.rs` | Complete with HPKE encryption |
| Recovery | [VERIFIED] IMPLEMENTED | `crates/coordination/src/choreography/recovery.rs` | Complete with share reconstruction via Lagrange interpolation |
| Locking | [VERIFIED] IMPLEMENTED | `crates/coordination/src/choreography/locking.rs` | Complete with deterministic lottery verification |
| **Simulation Integration** |
| Simulation Engine | [VERIFIED] IMPLEMENTED | `crates/simulator/` | Deterministic with effect injection |
| ProtocolExecutor | [VERIFIED] IMPLEMENTED | `crates/simulator/src/runners/protocol.rs` | Tokio integration with tick advancement |
| Network Simulation | [VERIFIED] IMPLEMENTED | `crates/simulator/src/network/` | Latency, partitions, byzantine testing |
| Choreographic Tests | [VERIFIED] IMPLEMENTED | `crates/simulator/tests/` | Multi-party protocol testing |
| **API Surface** |
| DeviceAgent API | [VERIFIED] IMPLEMENTED | `crates/agent/src/agent.rs` | High-level choreographic integration |
| Simulation API | 🚧 PARTIAL | `crates/simulator/` | Good coverage, some convenience methods missing |
| **Integration Components** |
| Event Signing | [VERIFIED] IMPLEMENTED | Throughout | Device certificate + FROST when required |
| Transport Abstraction | 🚧 PARTIAL | `crates/transport/` | Stub implementation, production transport pending |
| Ledger Integration | [VERIFIED] IMPLEMENTED | `crates/journal/` | CRDT events with threshold signatures |

**Legend:**
- [VERIFIED] **IMPLEMENTED**: Fully working as documented
- 🚧 **PARTIAL**: Exists but incomplete
- [NOT IMPLEMENTED] **NOT_IMPLEMENTED**: Missing or stub only
- 📝 **NEEDS_UPDATE**: Implemented but documentation outdated

This document explains how Aura’s Phase 2 “choreographic” runtime stitches together
threshold identities, the CRDT ledger, and the simulation engine.  It supersedes
earlier write‑ups that treated protocols as bespoke state machines and replaces
them with a single methodology based on *session types* and *global scripts*.

---

## 1. Motivation

Aura’s first prototypes implemented DKD, resharing, recovery, and locking as ad‑hoc
async flows that directly manipulated the ledger.  Those flows were difficult to test,
race‑prone, and hard to evolve.  Phase 2 introduces:

1. **Choreographic Protocols** – each protocol is written once from a *global* viewpoint
   and projected automatically to every device.
2. **Session Types** – the ordering of events (broadcast, threshold collection, finalise)
   is encoded in the type of the choreography.  Misordered instructions refuse to compile.
3. **Instruction Interpreter** – the `ProtocolContext` mediates between pure scripts and
   side effects (`WriteToLedger`, `AwaitThreshold`, `WaitEpochs`, etc.).
4. **Deterministic Simulation** – the same scripts run unmodified inside `aura-simulator`, using
   a simulated `TimeSource` and deterministic network to reproduce bugs exactly.

The result is a unified execution model: protocol authors write linear async code, the
context handles ledger I/O and wakeups, and both production and simulation use the same
runtime.

---

## 2. Architectural Overview

### 2.1 Choreographic Execution Stack

```
┌───────────────────────────────────────────────┐
│ Protocol Choreography (global script)         │
│   dkd_choreography(), resharing_choreography()│
└───────────────▲───────────────────────────────┘
                │ Instruction::*
┌───────────────┴───────────────────────────────┐
│ ProtocolContext (local projection)            │
│   • Tracks participants, threshold             │
│   • Signs events with device cert              │
│   • Executes instructions (ledger / awaits)    │
└───────────────▲───────────────────────────────┘
                │ Uses
┌───────────────┴───────────────────────────────┐
│ TimeSource & SimulationScheduler              │
│   • Production: wall clock + Notify           │
│   • Simulation: tick-based scheduler          │
│   • Wake conditions: NewEvents, EpochReached  │
└───────────────▲───────────────────────────────┘
                │ Interacts with
┌───────────────┴───────────────────────────────┐
│ AccountLedger (Automerge CRDT) & Transport    │
│   • Instruction::WriteToLedger → CRDT event   │
│   • Instruction::Await* → event watcher       │
│   • Transport is injectable (QUIC, WebRTC…)   │
└───────────────────────────────────────────────┘
```

### 2.2 Session Types in Practice

Each choreography documents its session type.  For example DKD:

```
Initiate(SessionId) .
Commit{p ∈ Participants}(Commitment_p) .
Reveal{p ∈ Participants}(Point_p) .
Aggregate(DerivedKey) .
Finalize(DerivedKey)
```

Resharing and recovery scripts encode their own flows (proposal → sub-share →
ack → finalize, guardian approvals → cooldown → resharing → complete, etc.).

### 2.3 Protocol Context Lifecycle

1. `DeviceAgent` constructs a `ProtocolContext` (per session) with device key, ledger,
   transport, participants, and threshold.
2. The choreography executes, yielding `Instruction`s.
3. The context performs the side effect, possibly waiting on ledger events.
4. Awaiting uses the `SimulationScheduler` (simulation) or `Notify` (production).
5. Scripts finish with `Ok(result)` or propagate a `ProtocolError`, at which point
   helper functions mark the session `Completed` / `Aborted` in the ledger.

---

## 3. Core Runtime Components

### 3.1 Instruction Set (`Instruction`, `InstructionResult`)

| Instruction                      | Description                                                  |
|---------------------------------|--------------------------------------------------------------|
| `WriteToLedger(Event)`          | Append CRDT event (signing handled by context).              |
| `AwaitEvent { filter, timeout }`| Wait for one matching ledger event.                          |
| `AwaitThreshold { count, … }`   | Wait for M matching events (with timeout).                   |
| `GetLedgerState`                | Snapshot of account state (nonce, parent hash, etc.).        |
| `GetCurrentEpoch`               | Lamport clock as maintained by account ledger.               |
| `WaitEpochs(n)`                 | Cooperative sleep via `TimeSource`.                          |
| `RunSubProtocol { … }`          | Launch nested choreography.                                  |
| `CheckForEvent { filter }`      | Non-blocking event check.                                    |

Scripts yield instructions; the context returns `InstructionResult` variants
(`Acknowledged`, `CollectedEvents`, `LedgerState`, etc.).

### 3.2 TimeSource & Scheduler

- **Production** – `ProductionTimeSource` wraps `tokio::Notify` and wall-clock sleeps.
- **Simulation** – `SimulatedTimeSource` registers wake conditions with
  `SimulationScheduler`.  Scripts await on a oneshot receiver; simulator ticks advance
  the scheduler, which wakes contexts whose conditions are satisfied.
- Wake Conditions: `EpochReached`, `TimeoutAt`, `NewEvents`, `EventMatching`, `ThresholdEvents`.

### 3.3 Event Watching

An `EventWatcher` runs alongside the context, tailing the ledger and dispatching callbacks
for filters used in `AwaitEvent` / `AwaitThreshold`.  Filters support matching by session
ID, event type, author set, or custom predicates (e.g., “commitment from participant X”).

### 3.4 Instruction Projection & Error Handling

The context automatically:

- Signs events using the device certificate (`sign_event`).
- Tracks `last_event_hash` / nonce to maintain ledger consistency.
- Translates `ProtocolErrorType` (`Timeout`, `InvalidState`, `Other`) into surface errors.
- On error, helper routines append `Abort*` ledger events (e.g., `AbortDkdSession`).

---

## 4. Protocol Choreographies

### 4.1 DKD (Deterministic Key Derivation)

Module: `crates/coordination/src/choreography/dkd.rs`

Session Type:
```
Initiate . Commit* . Reveal* . Aggregate . Finalize
```

Flow:
1. **Initiate** – Device writes `InitiateDkdSession` event (threshold, participants, TTL).
2. **Commit** – Each device generates deterministic commitment, writes `RecordDkdCommitment`,
   waits for threshold commitments via `AwaitThreshold`.
3. **Reveal** – Broadcast point via `RevealDkdPoint`, await threshold reveals.
4. **Aggregate** – Local aggregation using `DkdParticipant`, produce derived key.
5. **Finalize** – Coordinator (current device) writes `FinalizeDkdSession`, others observe.

Timeouts (e.g., `TimeoutAt(epoch + 10)`) cause the context to abort and write
`AbortDkdSession`.  Threshold failures throw `ProtocolErrorType::Timeout`.

### 4.2 Resharing

Module: `choreography/resharing.rs`

Session Type (simplified):
```
Propose → DistributeSubShare* → Acknowledge* → Verify → Finalize
```

The choreography handles:
- Lock acquisition (`Instruction::RunSubProtocol` → locking choreography).
- Sub-share distribution (HPKE-encrypted payloads stored in ledger).
- Threshold acknowledgments and replay-proof counters.
- Finalization with `FinalizeResharing` event and new threshold metadata.

### 4.3 Recovery

Module: `choreography/recovery.rs`

Session Type:
```
Initiate → CollectGuardianApproval* → Cooldown → Reshare → Complete
```

Key features:
- Guardian approvals are `AwaitThreshold` over `CollectGuardianApproval` events.
- Cooldown uses `WaitEpochs` / `TimeoutAt`.
- Recovery reuses the resharing choreography via `RunSubProtocol`.
- Completion writes `CompleteRecovery`, bumps session epoch, invalidates old tickets.

### 4.4 Locking (Operation Lock)

Module: `choreography/locking.rs`

Implements `Request → Grant → Release`, used by resharing to serialize access to
account-wide mutable operations.  Session types ensure grants and releases are balanced.

---

## 5. Integration with Ledger & Transport

- **Event Signing** – All events are authenticated with device certificates via
  `ProtocolContext::sign_event`.  Threshold events use FROST when required.
- **Counters & Locks** – Ledger stores per-relationship counters, operation locks, and
  session metadata.  Choreographies call helper instructions that emit the necessary
  threshold-signed ledger events (`IncrementCounter`, `GrantOperationLock`, etc.).
- **Transport Abstraction** – Choreographies are control-plane only; data-plane
  connections (e.g., QUIC, WebRTC) are established after successful rendezvous.

---

## 6. Simulation Integration (`aura-simulator`)

The simulation harness injects:

- Deterministic `Effects` (seeded RNG, deterministic clock).
- `SimulatedTimeSource` tied to the `SimulationScheduler` (tick advancement).
- `SimulatedNetwork` implementing the `Transport` trait.

`ProtocolExecutor` polls protocol futures, and when all are pending it advances the
simulation by one tick.  Wake conditions are registered with the scheduler, ensuring that
protocol futures resume once the simulated time / event conditions are satisfied.

The simulation framework includes built-in deadlock detection and wake condition tracking to help catch protocol hangs.

---

## 7. Testing & Verification

- **Unit tests** – Each choreography has integration tests covering honest 3/5 party runs,
  byzantine drop/corrupt behaviors, resharing threshold changes, guardian recovery flows.
- **Simulation tests** – Tests in `crates/simulator/tests/` operate on `Simulation`, verifying ledger
  state, timeouts, and byzantine scenarios.
- **Tokio integration** – `tokio_choreographic` tests ensure the same scripts run under a
  real tokio executor without the simulation harness.
- **Determinism** – Tests set a fixed RNG seed; derived keys and event sequences are stable.

---

## 8. Minimal API Surface (Rust)

```rust
// Execute a choreography for N participants inside the simulation.
use std::sync::Arc;
use tokio::sync::RwLock;

let sim = Arc::new(RwLock::new(aura_simulator::Simulation::new(42)));
let (_account_id, devices) = {
    let mut sim_guard = sim.write().await;
    sim_guard.add_account_with_devices(&["alice", "bob", "carol"]).await
};

let participants: Vec<_> = {
    let sim_guard = sim.read().await;
    devices.iter()
        .map(|(pid, _)| sim_guard.get_participant(*pid).unwrap())
        .collect()
};

// Create protocol futures using choreography builders
let protocol_futures: Vec<_> = participants.into_iter()
    .map(|p| p.execute_dkd_protocol())
    .collect();

let executor = aura_simulator::runners::ProtocolExecutor::new(sim);
executor.run_many(protocol_futures).await;
```

```rust
// Production usage inside DeviceAgent (simplified)
let mut ctx = ProtocolContext::new(
    session_id,
    device_id,
    participants,
    Some(threshold),
    ledger.clone(),
    transport.clone(),
    effects.clone(),
    device_signing_key.clone(),
    Box::new(ProductionTimeSource::new()),
);

let derived_key = choreography::dkd::dkd_choreography(&mut ctx).await?;
```

---

## 9. Future Work

1. **Threshold DH Ceremony** – Replace per-device link DH with true threshold DH.
2. **Optimised Event Watchers** – Delta-based subscriptions to avoid scanning entire logs.
3. **Static Session Types** – Generate Rust session types from choreography macros to catch
   misordered `Instruction`s at compile time.
4. **Automated Key Rewrap** – Ledger-driven re-encryption of pairwise secrets for newly
   added devices (integration with SBB/Rendezvous work).
5. **Formal Verification** – Model-check choreographies to ensure deadlock freedom and
   progress guarantees.

---

## 10. References

- `crates/coordination/src/choreography/*.rs` – Protocol implementations.
- `crates/coordination/src/execution/context.rs` – `ProtocolContext` and instruction interpreter.
- `crates/coordination/src/execution/time.rs` – `TimeSource` implementations.
- `crates/simulator` – Deterministic simulation engine.
- `work/04_declarative_protocol_evolution.md` – Architectural roadmap.
