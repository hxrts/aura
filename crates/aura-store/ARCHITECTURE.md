# Aura Store (Layer 2)

## Purpose

Define storage domain types and fact-based state management for content-addressed storage with cryptographic chunk IDs.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Content addressing: `ChunkId`, `ContentId`, `ChunkManifest` | Actual storage I/O (use `StorageEffects`) |
| Chunk management: `compute_chunk_layout`, `ErasureConfig` | Authorization enforcement (use `aura-authorization` for Biscuit) |
| Fact types: `StorageFact` (for journal integration) | Runtime handler composition |
| CRDT storage state: `StorageState`, `StorageOpLog` | |
| Search types: `SearchQuery`, `SearchResults` | |
| Pure domain logic; I/O via effects | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Inbound | `aura-core` | Domain types, effect traits, content addressing |

## Invariants

- Content-addressed with cryptographic chunk IDs.
- Storage changes recorded as `StorageFact` for journals.
- Authority model: operations attributed to `AuthorityId`.
- CRDT merge for distributed storage state.

### InvariantStoreContentAddressIntegrity

Stored content identifiers must be deterministic and storage mutations must remain fact-backed.

Enforcement locus:
- src chunking and content identifier derivation compute stable addresses.
- Storage mutation paths emit storage facts for journal replay.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-store

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines content-addressed deterministic state.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines convergence and replay requirements.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-store` is primarily `Pure`. It defines storage-domain semantics and capability shapes rather than `ActorOwned` storage services. Exclusive access or transfer semantics are modeled as `MoveOwned` contracts in higher layers. `Observed` layers may inspect derived storage state but not mutate domain truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/chunking.rs`, `src/manifest.rs`, `src/search.rs` | `Pure` | Content addressing, manifests, and query/value semantics. |
| `src/crdt.rs`, `src/facts.rs`, `src/state.rs` | `Pure`, `MoveOwned` | Fact-backed storage transitions and explicit op-log/state transfer values. |
| Capability-gated storage admission types | `MoveOwned` | Higher layers consume these as explicit authority-bearing records rather than implicit mutable ownership. |
| Actor-owned runtime state | none | Storage services and caches belong in higher layers, not Layer 2. |
| Observed-only surfaces | none | Observation of derived storage state belongs in higher layers. |

### Capability-Gated Points

- Typed storage operation admission and authority-attributed storage facts
- Content/state transitions consumed by higher-layer journal and authorization gates

## Testing

### Strategy

aura-store defines content-addressed storage. If content addressing is non-deterministic, chunk IDs differ across replicas and Merkle trees diverge. If CRDT merge laws fail, storage state is order-dependent.

### Commands

```
cargo test -p aura-store --test laws       # CRDT algebraic properties
cargo test -p aura-store --test contracts  # storage invariants + encoding
cargo test -p aura-store --lib             # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| StorageState join not commutative | `tests/laws/storage_state_crdt.rs` | covered (proptest) |
| StorageState join not associative | `tests/laws/storage_state_crdt.rs` | covered (proptest) |
| StorageState join not idempotent | `tests/laws/storage_state_crdt.rs` | covered (proptest) |
| Quota used exceeds quota limit | `tests/contracts/storage_invariants.rs` | covered |
| Chunk layout metadata mismatch | `tests/contracts/storage_invariants.rs` | covered |
| Content address non-deterministic | `tests/contracts/storage_invariants.rs` | covered |
| ContentId hash unstable | `tests/contracts/storage_invariants.rs` | covered |
| StorageFact encoding roundtrip breaks | `tests/contracts/storage_invariants.rs` | covered |
| StorageFact encoding non-deterministic | `tests/contracts/storage_invariants.rs` | covered |
| Overlapping ContentId merge loses data | `tests/contracts/storage_invariants.rs` | covered |
| Overlapping ContentId merge non-commutative | `tests/contracts/storage_invariants.rs` | covered |

## References

- [Database & Queries](../../docs/107_database.md)
- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
