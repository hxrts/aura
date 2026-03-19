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
- Terminal lifecycle state in protocol coordinators must carry typed failure
  causes rather than stringly `Failed(...)` payloads; epoch rotation is the
  current reference pattern.
- `Observed` consumers may inspect sync state but not author it.

### Ownership Inventory

- `Pure`
  - merkle verification, reconciliation, writer-fence checks, protocol facts,
    and reducers in `core/` and `protocols/`
- `MoveOwned`
  - sync-session identifiers, fence guards, and proposal/ceremony transitions
    that invalidate stale owners on handoff
- `ActorOwned`
  - long-lived runtime orchestration belongs in Layer 6; within this crate,
    `SyncService` and `MaintenanceService` keep service-local mutable state
    behind a single service-owned lock boundary rather than shared `Arc` state
- `Observed`
  - health, metrics, verification outputs, and status inspection surfaces

### Capability-Gated Points

- typed terminal protocol/service failure is required at async boundaries
- readiness and publication are expected to flow through owning runtime/service
  coordinators rather than ambient callers
- timeout and retry behavior must use the shared timeout/retry model rather than
  crate-local wall-clock ownership

### Verification Hooks

- `cargo check -p aura-sync`
- `just ci-timeout-policy`
- `just ci-timeout-backoff`
- targeted service/protocol tests in `cargo test -p aura-sync ...`

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
## Testing

### Strategy

Merkle verification and anti-entropy determinism are the primary concerns.
Tests are organized into three groups: `tests/integrity/` for data integrity
and digest stability, `tests/protocol/` for sync protocol integration, and
`tests/integration/` for multi-device and network partition scenarios.

### Running tests

```
cargo test -p aura-sync
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Digest non-deterministic | `tests/integrity/anti_entropy_digest_stability.rs` | Covered |
| Anti-entropy non-idempotent | `tests/integrity/anti_entropy_idempotence.rs` | Covered |
| Migration breaks existing data | `tests/integrity/migration_validation.rs` (23 tests) | Covered |
| Protocol creation/config invalid | `tests/protocol/protocol_integration.rs` (25 tests) | Covered |
| Epoch rotation state machine wrong | `tests/protocol/protocol_integration.rs` (6 epoch tests) | Covered |
| Journal sync loses facts or diverges | `tests/integration/journal_sync.rs` (9 tests) | Covered |
| Network partition causes split-brain | `tests/integration/network_partition.rs` (8 tests) | Covered |
| OTA ceremony insufficient approvals | `tests/integration/ota_coordination.rs` (9 tests) | Covered |
| Multi-device coordination fails | `tests/integration/multi_device_scenarios.rs` (5 tests) | Covered |
| Anti-entropy under packet loss | `tests/integration/anti_entropy.rs` (8 tests) | Covered |

## Boundaries
- Fact storage lives in aura-journal.
- Transport effects live in aura-effects.
- Runtime sync manager lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
