# Aura Sync (Layer 5)

## Purpose

Synchronization protocol providing fact exchange, merkle verification, anti-entropy coordination, and writer fence semantics for distributed journal consistency.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Sync core types and protocol definitions | Fact storage (aura-journal) |
| Merkle verification and integrity checks | Transport effects (aura-effects) |
| Writer fence semantics | Runtime sync manager (aura-agent) |
| Pure peer-discovery views over runtime-owned rendezvous descriptor snapshots | Runtime descriptor cache ownership |
| Maintenance service for background sync | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers, session types |
| Incoming | aura-journal | Fact infrastructure, commitment trees |
| Incoming | lower-layer protocols | Transport coordination |
| Outgoing | — | `SyncCore` types for synchronization state |
| Outgoing | — | `SyncProtocol`, `FactSyncProtocol`, `AuthorityJournalSync` for sync flows |
| Outgoing | — | `MerkleVerifier`, `MerkleComparison`, `VerificationResult` for integrity checks |
| Outgoing | — | `WriterFence`, `WriterFenceGuard` for write ordering |
| Outgoing | — | `MaintenanceService` for background sync operations |

## Invariants

- Sync operations must not bypass guard chain checks in runtime.
- Protocols should operate on explicit inputs (snapshot, budget, timestamp).
- Merkle verification ensures fact integrity across peers.

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

### InvariantSyncTheoremPackAdmission

The OTA activation and device-epoch-rotation choreographies must remain
explicitly theorem-pack-gated through `AuraTransitionSafety`.

Enforcement locus:
- `src/protocols/ota_activation.tell` and
  `src/protocols/device_epoch_rotation.tell` declare the theorem pack in source.
- generated manifest metadata carries the required theorem pack and capability
  set.
- runtime launch in `aura-agent` fails closed when the admitted runtime does
  not expose the transition-safety capability surface.

Failure mode:
- OTA or device-epoch ceremonies can start on a runtime that lacks the
  transition / receipt / bridge guarantees those flows assume.

Verification hooks:
- `cargo test -p aura-sync theorem_pack_protocols -- --nocapture`
- `cargo test -p aura-agent theorem -- --nocapture`

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-sync` combines `Pure` verification/reconciliation logic with explicit `MoveOwned` sync-session authority where exclusivity matters.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| merkle verification, reconciliation, writer-fence checks, protocol facts, and reducers | `Pure` | Deterministic verification and reconciliation logic. |
| sync-session identifiers, fence guards, and proposal/ceremony transitions | `MoveOwned` | Invalidate stale owners on handoff. |
| `SyncService` and `MaintenanceService` local mutable state | `ActorOwned` | Service-local mutable state behind a single service-owned lock boundary. |
| health, metrics, verification outputs, and status inspection | `Observed` | Downstream inspection surfaces. |
| rendezvous adapter peer views | `Pure` | Derives `LinkEndpoint` and `ServiceDescriptor` views from runtime snapshots without turning descriptor compatibility data into routing policy. Includes sync-blended `Hold` retrieval batching over selector-based requests and bounded reply windows. |

Service implementation note:
- `src/services/sync.rs` and `src/services/maintenance.rs` keep orchestration in the main file and push health, builder/tests, and narrow bookkeeping into private submodules. This keeps the public `services` surface concrete while still making `ActorOwned` state easier to audit.

### Capability-Gated Points

- typed terminal protocol/service failure is required at async boundaries
- readiness and publication are expected to flow through owning runtime/service coordinators rather than ambient callers
- timeout and retry behavior must use the shared timeout/retry model rather than crate-local wall-clock ownership

## Testing

### Strategy

Merkle verification and anti-entropy determinism are the primary concerns. Tests are organized into three groups: `tests/integrity/` for data integrity and digest stability, `tests/protocol/` for sync protocol integration, and `tests/integration/` for multi-device and network partition scenarios. Shared deterministic fixture/time/device helpers live under `tests/support/`, while `tests/integration/test_utils.rs` owns only the mechanical multi-device topology builders used across scenario tests.

### Commands

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

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Operation Categories](../../docs/109_operation_categories.md)
