# Aura Sync (Layer 5) - Architecture and Invariants

## Purpose
Synchronization protocol providing fact exchange, merkle verification, anti-entropy
coordination, and writer fence semantics for distributed journal consistency.

## Inputs
- aura-core (effect traits, identifiers, session types).
- aura-journal (fact infrastructure, commitment trees).
- Lower-layer protocols for transport coordination.

## Outputs
- `SyncCore` types for synchronization state.
- `SyncProtocol`, `FactSyncProtocol`, `AuthorityJournalSync` for sync flows.
- `MerkleVerifier`, `MerkleComparison`, `VerificationResult` for integrity checks.
- `WriterFence`, `WriterFenceGuard` for write ordering.
- `MaintenanceService` for background sync operations.

## Invariants
- Sync operations must not bypass guard chain checks in runtime.
- Protocols should operate on explicit inputs (snapshot, budget, timestamp).
- Merkle verification ensures fact integrity across peers.

## Boundaries
- Fact storage lives in aura-journal.
- Transport effects live in aura-effects.
- Runtime sync manager lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
