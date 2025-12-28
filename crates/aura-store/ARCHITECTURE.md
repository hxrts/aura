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

## Boundaries
- No actual storage I/O (use StorageEffects).
- Authorization is metadata only (use aura-authorization for Biscuit).
- Pure domain logic; I/O via effects.
