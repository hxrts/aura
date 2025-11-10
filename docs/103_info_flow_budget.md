# 300 · Information Flow Budget

This document defines the unified **information-flow budget** system that simultaneously enforces privacy leakage limits and prevents spam. The goal is to express “who may talk, how often, and with how much metadata” using the same semilattice primitives as the rest of Aura so the feature is simple, monotone, and deterministic.

Scope for 1.0:
- No automatic cover traffic (handlers simply block when a budget is exhausted).
- Budgets are per-context (`RID`, `GID`, or DKD namespace) and per-peer.
- Enforcement happens locally before any transport side effects.

Later versions can layer padding/cover traffic on top of the same ledger without changing the core math.

---

## 1. Data Model

Each `(context, peer)` pair stores a **flow budget fact** in the journal:

```rust
struct FlowBudget {
    spent: u64,   // join-semilattice: merge = max
    limit: u64,   // meet-semilattice: merge = min
    epoch: Epoch, // ties the fact to the active key epoch
}
```

- `spent` counts the cumulative cost of messages we have emitted in the current epoch.
- `limit` is the maximum cost allowed before throttling.
- Budgets live inside the same CRDT namespace as `JournalCaps`, so they inherit monotonicity. Joins can only increase `spent`; meets can only decrease `limit`.
- Because entries are scoped to contexts, replicas outside that context (or lacking the relevant capability) never see the counters, preventing leakage through the ledger itself.

**Flow cost**: every send/receive primitive carries `flow_cost` metadata derived from the choreography. For 1.0 the defaults are:
- `DirectSend`: 1 unit
- `RelayForward`: 1 unit (charged to both hops)
- `LargePayload` (e.g., snapshot chunk): proportional to size, `ceil(bytes / 1 KiB)`
- `QueryRequest/Reply`: 2 units (to account for metadata exposure)

---

## 2. Charging & Receipts

### 2.1 Charging algorithm

```
fn charge(ctx: ContextId, peer: DeviceId, cost: u64) -> Result<(), FlowError> {
    let FlowBudget { spent, limit, epoch } = ledger.lookup(ctx, peer);
    if spent.saturating_add(cost) > limit {
        return Err(FlowError::BudgetExhausted { ctx, peer, epoch });
    }
    ledger.update(ctx, peer, FlowBudget {
        spent: spent + cost,
        limit,
        epoch,
    });
    Ok(())
}
```

Charging happens **before** the corresponding `TransportEffects::send` call. If the charge fails, the choreography emits an error branch and no network observable is produced.

### 2.2 Multi-hop receipts

Relays require proof that the upstream hop already charged its budget:

```
struct FlowReceipt {
    ctx: ContextId,
    from: DeviceId,
    to: DeviceId,
    remaining: u64,     // limit - spent after the charge
    epoch: Epoch,
    sig: Signature,     // signed by sender using context key
}
```

Protocol:
1. Sender charges its `(ctx, receiver)` budget.
2. Sender emits the payload plus `FlowReceipt`.
3. Relay validates the signature and checks `remaining > 0`.
4. Relay charges its own `(ctx, next_hop)` budget before forwarding.
5. If any step fails, the message is dropped locally.

Receipts never leave the context (they are encrypted/MACed with the same relationship keys), so they do not leak budget values globally.

---

## 3. Replenishment

Budgets replenish when epochs rotate or when an explicit `BudgetUpdate` fact is merged. Updates are **deterministic functions of journal data**, so every replica derives the same limit.

### 3.1 Input signals

| Symbol | Source | Meaning |
|--------|--------|---------|
| `w` | Web-of-trust edge weight ∈ [0, 1] | Baseline trust |
| `recip` | `min(outbound_msgs, inbound_msgs)` | Reciprocity bonus |
| `abuse` | Count of abuse flags for the peer/context | Penalty |
| `tier` | Manual override (guardian, relay, etc.) | Multiplier |

All inputs are facts already present in the journal (`Guardianship`, `AbuseReport`, `EdgeMetadata`).

### 3.2 Deterministic formula

```
base = BASE_LIMIT * tier
trust_boost = base * w
recip_boost = min(recip / RECIP_WINDOW, RECIP_CAP) * RECIP_UNIT
penalty = abuse * PENALTY_UNIT

new_limit = max(MIN_LIMIT, trust_boost + recip_boost - penalty)
```

Constants (`BASE_LIMIT`, `RECIP_UNIT`, etc.) live in the capability policy configuration so all devices share them.

### 3.3 Update choreography

During epoch rotation:
1. Each participant computes `new_limit`.
2. Participants gossip `BudgetProposal{ctx, peer, new_limit, epoch}` facts.
3. Merge rule takes the **minimum** proposed limit to remain conservative.
4. `spent` resets to 0 with the new epoch fact.

Because limits only move via facts, forks cannot accidentally grant extra budget.

---

## 4. Enforcement Pipeline

1. **Choreography annotations** – every `send` node adds both `leakage` and `flow_cost`.
2. **Runtime shim** – generated role code calls `charge(ctx, peer, flow_cost)` using the current `AuraContext`. Failure routes to an error branch (e.g., retry later, notify user).
3. **Transport** – once the charge succeeds, the actual network packet is sent. For multi-hop sends, the receipt is attached automatically.
4. **Ledger merge** – `FlowBudget` updates enter the journal as CRDT ops so all devices converge on the same spent/limit values.
5. **Observation** – high-level analytics can read bucketed per-epoch `spent` values, but raw counters remain scoped to their contexts.

Error surface area is intentionally tiny: only the handler performing the send can fail, and the failure is deterministic (“budget exhausted”).

### 4.1 FlowGuard Interface

Every choreography must invoke a `FlowGuard` before calling any transport effect. The guard enforces both flow budgets and leakage annotations:

```rust
pub struct FlowGuard<'a> {
    ctx: &'a ContextId,
    peer: &'a DeviceId,
    flow_cost: u32,
    leakage: LeakageTuple,
}

impl<'a> FlowGuard<'a> {
    pub async fn authorize(&self, effects: &impl FlowBudgetEffects) -> Result<(), FlowError> {
        effects.charge_flow(self.ctx, self.peer, self.flow_cost).await?;
        effects.record_leakage(self.ctx, self.leakage).await?;
        Ok(())
    }
}
```

**Requirements**:
- All transport calls in choreographies (`Send`, `Broadcast`, `Forward`) must pass through `FlowGuard::authorize`.
- Manual protocols must call the same guard until the session-type projection emits the code automatically.
- FlowGuard implementations live in `aura-protocol/src/guards/privacy.rs` and are reused by rendezvous, search, recovery, storage sync, and any future protocol that emits network traffic.

This makes FlowBudget enforcement a mandatory part of every protocol rather than an optional add-on.

---

## 5. Component Responsibilities

| Component | Responsibility |
|-----------|----------------|
| **Journal CRDT** | Store `FlowBudget` facts; merge via max/min. |
| **Protocol compiler** | Emit `flow_cost` metadata for each send node. |
| **Transport handler** | Enforce `charge` + attach `FlowReceipt`; drop packets without valid receipts. |
| **Effect guards** | Expose `FlowError` up to the choreography so sessions can retry/defer. |
| **Simulator** | Provide deterministic knobs for budget limits to test spam scenarios and privacy leakage counters together. |

---

## 6. Interaction with Leakage Budgets

- `LeakageEffects` still tracks observer-class exposure (external, neighbor, in-group).
- Flow budgets sit underneath as a hard rate limit: if the ledger blocks the send, no leakage accounting occurs.
- Because both artifacts are monotone CRDTs, we can mux them in the same handler (`PrivacyGuard`) without risking divergence.

---

## 7. Out-of-Scope / Future Work

1. **Cover traffic** – deferred to >1.0. When implemented, dummy packets will call `charge` exactly like real packets so they cannot be distinguished.
2. **Adaptive flow cost** – future heuristics may raise `flow_cost` for unusually large payloads or sensitive choreographies.
3. **Global budgets** – today everything is per-context. A later design could add per-device global caps by storing an additional `FlowBudget` keyed by `(device_id, "*")`.

---

## 8. Summary

By encoding spam limits as yet another semilattice fact, we keep the system:
- **Simple** – charging is a single function call before an effect.
- **Robust** – merges can only tighten limits or remember higher spend.
- **Consistent** – the same web-of-trust math that governs capabilities now governs throughput and metadata exposure.

This spec should give every implementation team (protocols, transport, simulator, product) a precise contract for building privacy-aware, spam-resistant flows in Aura 1.0.

---

## Implementation Notes

### Current Implementation Status

**✅ Flow Budget Infrastructure**:
- **Budget tracking**: [`crates/aura-mpst/src/leakage.rs`](../crates/aura-mpst/src/leakage.rs) - MPST budget annotations and enforcement
- **Privacy guards**: [`crates/aura-protocol/src/guards/privacy.rs`](../crates/aura-protocol/src/guards/privacy.rs) - Runtime budget checking
- **Budget semilattice**: Flow budget facts stored in journal CRDT with proper join/meet semantics

**✅ Message Cost Annotation**:
- **Session types**: MPST extensions support `(S --[ msg | ℓ_ext, ℓ_ngh, ℓ_grp ]--> S')` leakage annotations
- **Guard checking**: Runtime enforcement of `L(τ,o) ≤ Budget(o)` constraints before message sends
- **Context isolation**: Budget tracking respects context boundaries (RID, GID, DKD namespaces)

**⚠️ Partial Implementation**:
- **Budget replenishment**: Infrastructure for epoch-based budget refresh exists but needs policy implementation
- **Trust weight calculation**: Web-of-trust based budget allocation under development
- **Receipt verification**: Multi-hop relay budget tracking partially implemented

**❌ Future Implementation**:
- **Cover traffic generation**: Automatic padding and dummy messages
- **Adaptive flow costs**: Dynamic cost adjustment based on payload size and sensitivity
- **Global budget caps**: Per-device aggregate limits across all contexts

### API Verification

The described APIs in this document correspond to real implementation:

**Working Today**:
- `FlowBudget { spent, limit, epoch }` - CRDT facts in journal
- `charge(flow_cost)` - Budget charging before message emission
- `PrivacyGuard` - Combined leakage and budget enforcement
- Context-scoped budget tracking

**Under Development**:
- Budget replenishment policies and epoch rotation
- Trust-weighted budget allocation algorithms
- Integration with transport layer for comprehensive enforcement

**Planned**:
- Cover traffic generation and automatic padding
- Advanced budget management and monitoring
- Performance optimization for high-throughput scenarios

### Testing Status

**✅ Property-Based Tests**:
- Budget semilattice laws verification (monotonicity, convergence)
- Guard enforcement correctness
- Context isolation validation

**⚠️ Integration Tests**:
- End-to-end budget enforcement across protocols
- Multi-hop relay budget tracking
- Performance impact measurement

For current flow budget usage patterns, see the guard implementations in [`crates/aura-protocol/src/guards/privacy.rs`](../crates/aura-protocol/src/guards/privacy.rs) and MPST integration in [`crates/aura-mpst/src/leakage.rs`](../crates/aura-mpst/src/leakage.rs).
