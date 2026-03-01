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

