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

## Ownership Model

- `aura-sync` combines `Pure` verification/reconciliation logic with explicit
  `MoveOwned` sync-session authority where exclusivity matters.
- Long-lived background sync ownership belongs in explicit `ActorOwned` runtime
  managers, not hidden in sync helpers.
- Sync mutation and publication must remain capability-gated and typed.
- Retry and completion semantics should terminate explicitly rather than through
  ambiguous background state.
- `Observed` consumers may inspect sync state but not author it.

### Detailed Specifications

### InvariantSyncMerkleVerification
Synchronization must reject unverifiable merkle evidence and preserve guard-aware transport constraints.

Enforcement locus:
- src protocols validate merkle proofs and fact integrity.
- Sync paths operate on explicit snapshot, budget, and timestamp inputs.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-sync

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines deterministic replication semantics.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines anti-entropy and integrity guarantees.
## Boundaries
- Fact storage lives in aura-journal.
- Transport effects live in aura-effects.
- Runtime sync manager lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
