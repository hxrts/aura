# Aura Anti-Entropy (Layer 4)

## Purpose

Provide digest-based reconciliation and broadcast coordination for OpLog sync,
with explicit guard chain enforcement on network operations.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Pure reconciliation logic (`sync/pure.rs`) | Guardless network sends |
| Guarded network operations (digest requests, op requests, announcements) | Long-lived background reconciliation ownership (higher-layer runtime) |
| Persistent OpLog caching via shared commitment tree storage keys | Application-specific protocol logic |
| Vector-clock metadata and causal consistency | Runtime composition or lifecycle management |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions, domain types |
| Down | `aura-journal` | Commitment tree storage keys |
| In | BloomDigest values | Reconciliation input |
| In | GuardChainEffects + TransportEffects | Effectful sync paths |
| In | StorageEffects | Persistent OpLog caching |
| Out | Merged OpLog updates | Pure set union semantics |
| Out | Guarded network operations | Digest requests, op requests, announcements |

## Invariants

- Reconciliation logic is pure (see `sync/pure.rs`).
- Network-visible operations must be guard-chain approved.
- Persistent storage uses shared commitment tree storage keys.
- Vector-clock metadata remains causally consistent (`InvariantVectorClockConsistent`).

### InvariantAntiEntropyReconciliationPurity

Anti-entropy reconciliation remains pure and deterministic for identical inputs.

Enforcement locus:
- sync pure logic computes reconciliation decisions without side effects.
- Network execution applies outputs only after guard-chain approval.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just test-crate aura-anti-entropy`

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

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-anti-entropy` keeps reconciliation logic `Pure`. Exclusive sync-session
or reconciliation ownership remains `MoveOwned` in higher-layer coordination.
Long-lived background reconciliation belongs in explicit `ActorOwned` runtime
services, not hidden here.

See [System Internals Guide](../../docs/807_system_internals_guide.md) §Core + Orchestrator Rule.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `pure.rs` and digest/reconciliation helpers | `Pure` | Deterministic anti-entropy decisions and digest math. |
| sync-session and request/response orchestration | `MoveOwned` | Reconciliation/session authority remains explicit and value-oriented. |
| `anti_entropy.rs`, `broadcast.rs`, `persistent.rs` | orchestration with typed errors | Effectful sync/broadcast orchestration stays explicit and does not become a hidden long-lived owner. |
| long-lived background reconciliation | none local | Ongoing reconciliation ownership belongs in higher-layer runtime services. |
| Observed-only surfaces | none | Observation of reconciliation results belongs downstream. |

### Capability-Gated Points

- Guard-approved digest/op request publication.
- Typed sync/broadcast outcomes consumed by higher-layer runtime and testing
  lanes.

## Testing

### Strategy

Reconciliation purity and digest determinism are the primary concerns.
Integration tests in `tests/reconciliation/` validate digest computation,
serialization roundtrips, and config defaults. Inline tests cover pure
reconciliation helpers and broadcast rate limiting.

### Commands

```
cargo test -p aura-anti-entropy
```

### Coverage matrix

| What breaks if wrong | Invariant | Status |
|---------------------|-----------|--------|
| Reconciliation non-deterministic | InvariantAntiEntropyReconciliationPurity | Covered (`src/pure.rs` `reconciliation_is_deterministic`) |
| Push/pull asymmetric (peers diverge) | InvariantAntiEntropyReconciliationPurity | Covered (`src/pure.rs` `reconciliation_is_symmetric`) |
| Vector clock order violated | InvariantVectorClockConsistent | Covered (Quint) |
| Digest order-dependent | — | Covered (`tests/reconciliation/`) |
| Digest non-deterministic | — | Covered (`tests/reconciliation/`) |
| Broadcast rate limiting broken | — | Covered (`src/broadcast.rs` inline) |
| Back pressure threshold ignored | — | Covered (`src/broadcast.rs` inline) |
| Sync error codes collide | — | Covered (`tests/reconciliation/`) |

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [System Internals Guide](../../docs/807_system_internals_guide.md)
