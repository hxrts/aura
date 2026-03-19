# Aura Anti-Entropy (Layer 4) - Architecture and Invariants

## Purpose
Provide digest-based reconciliation and broadcast coordination for OpLog sync,
with explicit guard chain enforcement on network operations.

## Inputs
- BloomDigest values for reconciliation.
- GuardChainEffects + TransportEffects for effectful sync paths.
- StorageEffects for persistent OpLog caching.

## Outputs
- Merged OpLog updates (pure set union semantics).
- Guarded network operations (digest requests, op requests, announcements).

## Invariants
- Reconciliation logic is pure (see `sync/pure.rs`).
- Network-visible operations must be guard-chain approved.
- Persistent storage uses shared commitment tree storage keys.
- Vector-clock metadata remains causally consistent (`InvariantVectorClockConsistent`).

## Ownership Model

- `aura-anti-entropy` keeps reconciliation logic `Pure`.
- Any exclusive sync-session or reconciliation ownership should remain
  `MoveOwned` in higher-layer coordination surfaces.
- Long-lived background reconciliation ownership belongs in explicit
  `ActorOwned` runtime services, not hidden here.
- Guarded sync publication must remain capability-aware and typed.
- `Observed` tooling may inspect reconciliation outcomes but not define them.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `pure.rs` and digest/reconciliation helpers | `Pure` | Deterministic anti-entropy decisions and digest math. |
| sync-session and request/response orchestration | `MoveOwned` | Reconciliation/session authority remains explicit and value-oriented. |
| `anti_entropy.rs`, `broadcast.rs`, `persistent.rs` | orchestration with typed errors | Effectful sync/broadcast orchestration stays explicit and does not become a hidden long-lived owner. |
| long-lived background reconciliation | none local | Ongoing reconciliation ownership belongs in higher-layer runtime services. |
| Observed-only surfaces | none | Observation of reconciliation results belongs downstream. |

### Capability-Gated Points

- guard-approved digest/op request publication
- typed sync/broadcast outcomes consumed by higher-layer runtime and testing
  lanes

### Verification Hooks

- `cargo check -p aura-anti-entropy`
- `cargo test -p aura-anti-entropy -- --nocapture`

### Detailed Specifications

### InvariantAntiEntropyReconciliationPurity
Anti-entropy reconciliation remains pure and deterministic for identical inputs.

Enforcement locus:
- sync pure logic computes reconciliation decisions without side effects.
- Network execution applies outputs only after guard-chain approval.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-anti-entropy

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines deterministic reduction.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines eventual convergence and vector-clock consistency.

### InvariantVectorClockConsistent
Anti-entropy reconciliation must preserve causal ordering metadata so merged views never violate vector-clock partial order.

Enforcement locus:
- sync digest exchange carries causal metadata for reconciliation decisions.
- merge path applies operations in a causally admissible order.
- guard-approved transport path preserves per-context sequencing assumptions.

Failure mode:
- Causal anomalies during replay/reconciliation.
- Divergent merged states after partition healing.

Verification hooks:
- `cargo test -p aura-anti-entropy`
- `quint run --invariant=InvariantVectorClockConsistent verification/quint/journal/anti_entropy.qnt`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantVectorClockConsistent`.
## Testing

### Strategy

Reconciliation purity and digest determinism are the primary concerns.
Integration tests in `tests/reconciliation/` validate digest computation,
serialization roundtrips, and config defaults. Inline tests cover pure
reconciliation helpers and broadcast rate limiting.

### Running tests

```
cargo test -p aura-anti-entropy
```

### Coverage matrix

| What breaks if wrong | Invariant | Status |
|---------------------|-----------|--------|
| Reconciliation non-deterministic | InvariantAntiEntropyReconciliationPurity | Covered (`src/pure.rs` inline) |
| Vector clock order violated | InvariantVectorClockConsistent | Covered (Quint) |
| Digest order-dependent | — | Covered (`tests/reconciliation/`) |
| Digest non-deterministic | — | Covered (`tests/reconciliation/`) |
| Broadcast rate limiting broken | — | Covered (`src/broadcast.rs` inline) |
| Back pressure threshold ignored | — | Covered (`src/broadcast.rs` inline) |
| Sync error codes collide | — | Covered (`tests/reconciliation/`) |

## Boundaries
- No guardless network sends.
- Storage helpers are shared via `aura_journal::commitment_tree::storage`.

## Core + Orchestrator Rule
- Pure reconciliation lives in `sync/pure.rs`.
- Effectful orchestration must accept explicit effect traits.
