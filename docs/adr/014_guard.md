# **ADR-014: Pure Guard Evaluation with Asynchronous Effect Interpretation**

**Status**: Proposed
**Date**: 2024-12-03
**Author**: Architecture Team

---

# 1. Context

Aura’s current guard system mixes synchronous trait calls with asynchronous effectful operations:

* Guards run on hot paths (protocol handlers, ratchets, validation) and are inherently **synchronous** at call sites.
* Effect operations (storage access, network I/O, capability evaluation, budget charging) are **asynchronous** and may require I/O.
* Simulation requires **deterministic**, **single-threaded**, **fully replayable** execution.
* WASM targets disallow blocking, making `block_on` and `blocking_recv` unusable.

This produces persistent friction:

1. **Sync guards calling async effects** → blocking → deadlocks in WASM or single-threaded runtimes.
2. **Simulation/production divergence** → cannot share guard logic.
3. **Effects hidden inside guards** → difficult to observe, record, replay, or test.

At the architectural level, this contradicts Aura’s theoretical model:
Effects should be *algebraic operations*, not imperative async calls.

---

# 2. Decision

**Convert the entire guard system to a *pure* synchronous evaluation model, where guards do not execute effects.**

Instead:

1. Guards take an immutable **GuardSnapshot**, representing all data they are allowed to inspect.

2. Guard evaluation is a **pure function**:

   ```
   (snapshot, request) → (decision, Vec<EffectCommand>)
   ```

3. An **EffectInterpreter** is responsible for executing the effect commands returned from guards:

   * In production: async, real I/O.
   * In simulation: deterministic, single-threaded, event-logged.

4. No guard directly performs I/O.

5. No blocking is needed anywhere.

6. Simulation and production share the same guard logic.

This aligns the system with algebraic-effect best practices and Aura’s own architecture model.

---

# 3. Architecture

## 3.1 Overview

```
           ┌─────────────────────────────────────┐
           │    Pure Guard Evaluation (sync)      │
           │   (Snapshot + Request → Outcome)     │
           └─────────────────────────────────────┘
                        │ produces
                        ▼
           ┌─────────────────────────────────────┐
           │         Effect Commands              │
           │ (charge budget, append journal, ... )│
           └─────────────────────────────────────┘
                        │ interpreted by
                        ▼
    ┌────────────────────────────┐     ┌────────────────────────────┐
    │ ProductionEffectInterpreter│     │ SimulationEffectInterpreter│
    │  - async I/O               │     │  - deterministic            │
    │  - storage, network, etc.  │     │  - event log                │
    └────────────────────────────┘     └────────────────────────────┘
```

All complexity moves to the interpreters.
Guards remain pure, simple, testable logic.

---

# 3.2 GuardSnapshot

Prepared by the async environment *before* entering the guard chain.

```rust
pub struct GuardSnapshot {
    pub now: TimeStamp,
    pub caps: Cap,                      // Derived capability set
    pub budgets: FlowBudgetView,        // Current headroom
    pub metadata: MetadataView,         // Key state for metadata rules
    pub rng_seed: [u8; 32],             // Pre-allocated randomness
}
```

This is a **read-only view**.
Guards cannot mutate global state.

---

# 3.3 Pure Guard Evaluation

The guard chain becomes:

```rust
pub struct GuardOutcome {
    pub decision: Decision,             // Authorized / Denied
    pub effects: Vec<EffectCommand>,    // Budget charges, journal writes, etc.
}
```

The guard chain implementation:

```rust
pub trait Guard {
    fn evaluate(
        &self,
        snapshot: &GuardSnapshot,
        request: &Request
    ) -> GuardOutcome;
}
```

Guards **never** perform effects directly.

They only describe what must happen *if* the guard manager accepts the request.

---

# 3.4 Effect Commands

These are the **primitive algebraic operations** of the Aura runtime.

```rust
pub enum EffectCommand {
    ChargeBudget { authority: AuthorityId, amount: u32 },
    AppendJournal { entry: JournalEntry },
    RecordLeakage { bits: u32 },
    StoreMetadata { key: String, value: String },
    SendEnvelope { to: Address, envelope: Vec<u8> },
    GenerateNonce { bytes: usize },
}
```

Key principle:
**Only minimal, domain-agnostic primitives go here.**
No high-level guards, no policy evaluation.

---

# 3.5 Effect Interpreters

### Production Interpreter (async)

```rust
#[async_trait]
pub trait EffectInterpreter {
    async fn exec(&self, cmd: EffectCommand) -> Result<EffectResult>;
}
```

* Uses `aura-effects`
* Performs real storage reads/writes
* Sends network envelopes
* Updates flow budgets
* Any batching/caching is local and transparent

### Simulation Interpreter (deterministic)

```rust
pub struct SimulationEffectInterpreter {
    pub time: TimeStamp,
    pub rng: StdRng,
    pub events: Vec<SimulationEvent>,
}
```

Effects append events:

```rust
self.events.push(SimulationEvent::BudgetCharged { ... });
self.events.push(SimulationEvent::MessageQueued { ... });
```

Everything is single-threaded, no locks, no non-determinism.

---

# 3.6 End-to-end Flow

```
async fn handle(request):
    snapshot = prepare_snapshot().await      // async
    outcome  = guards.evaluate(snapshot)     // pure, sync

    if outcome.decision == Denied:
        return Deny

    for cmd in outcome.effects:              // async
        interpreter.exec(cmd).await?
```

Guards remain fast and sync → Zero blocking risk.
Interpreter is async and free to take time → proper I/O boundaries.

---

# 4. Rationale

### Why purity?

* Perfect fit for algebraic effects
  (effects become data; interpreters do the work).
* Avoids mix of sync/async in guards.
* Works in WASM (no blocking).
* Enables deterministic simulation.

### Why snapshots?

* Guards cannot accidentally perform I/O.
* Snapshot preparation is async and can involve batching.
* Guards become a pure decision system.

### Why two interpreters?

* Production: real async I/O
* Simulation: reproducible deterministic behavior

Shared guard logic, separate execution semantics.

### Why not sync executor with channels?

* Any sync→async bridging requires blocking or thread-hopping.
* WASM and single-threaded runtimes cannot support this.
* Async effects become explicit and boundary-safe.

---

# 5. Consequences

### Positive

* Guards are pure → easy to test, simulate, reason about.
* No blocking & full WASM compatibility.
* Effects are observable and replayable.
* Deterministic simulation with full event logs.
* Clean separation of concerns.

### Negative

* Requires rewriting some existing guard logic to remove embedded I/O.
* Requires an async snapshot preparation stage.
* More explicit API (commands returned instead of implicit actions).

### Neutral

* Effect command vocabulary must stay minimal.
* Interpreters become the main place for complexity (good layering).

---

# 6. Migration Strategy

### Phase 1 — Introduce New Primitives (1–2 weeks)

* Add `EffectCommand`, `EffectResult`, and `EffectInterpreter`.
* Add `GuardSnapshot`.
* Add pure guard evaluation API.

### Phase 2 — Dual Path (1–2 weeks)

* Prepare snapshots in existing runtime.
* Migrate guards to pure evaluation, keep old system as fallback.

### Phase 3 — Interpreter Integration (1 week)

* Production: integrate async interpreter.
* Simulation: integrate deterministic interpreter.

### Phase 4 — Remove Old System (1 week)

* Delete `GuardEffectSystem`.
* Delete blocking sync/async bridges.
* Update documentation.
