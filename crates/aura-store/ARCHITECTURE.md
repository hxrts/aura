# Aura Store (Layer 2) - Architecture and Invariants

## Purpose
Define storage domain types and fact-based state management for content-addressed
storage with cryptographic chunk IDs.

## Inputs
- aura-core (domain types, effect traits, content addressing).

## Outputs
- Content addressing: `ChunkId`, `ContentId`, `ChunkManifest`.
- Chunk management: `compute_chunk_layout`, `ErasureConfig`.
- Fact types: `StorageFact` (for journal integration).
- CRDT storage state: `StorageState`, `StorageOpLog`.
- Search types: `SearchQuery`, `SearchResults`.

## Invariants
- Content-addressed with cryptographic chunk IDs.
- Storage changes recorded as `StorageFact` for journals.
- Authority model: operations attributed to `AuthorityId`.
- CRDT merge for distributed storage state.

## Ownership Model

- `aura-store` is primarily `Pure`.
- It defines storage-domain semantics and capability shapes rather than
  `ActorOwned` storage services.
- Any exclusive access or transfer semantics should be modeled as `MoveOwned`
  contracts in higher layers, not hidden mutable ownership here.
- Capability-gated storage operations should remain explicit and typed.
- `Observed` layers may inspect derived storage state but not mutate domain
  truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/chunking.rs`, `src/manifest.rs`, `src/search.rs` | `Pure` | Content addressing, manifests, and query/value semantics. |
| `src/crdt.rs`, `src/facts.rs`, `src/state.rs` | `Pure`, `MoveOwned` | Fact-backed storage transitions and explicit op-log/state transfer values. |
| Capability-gated storage admission types | `MoveOwned` | Higher layers consume these as explicit authority-bearing records rather than implicit mutable ownership. |
| Actor-owned runtime state | none | Storage services and caches belong in higher layers, not Layer 2. |
| Observed-only surfaces | none | Observation of derived storage state belongs in higher layers. |

### Capability-Gated Points

- typed storage operation admission and authority-attributed storage facts
- content/state transitions consumed by higher-layer journal and authorization
  gates

### Verification Hooks

- `cargo check -p aura-store`
- `cargo test -p aura-store -- --nocapture`

### Detailed Specifications

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
## Boundaries
- No actual storage I/O (use StorageEffects).
- Authorization is metadata only (use aura-authorization for Biscuit).
- Pure domain logic; I/O via effects.
