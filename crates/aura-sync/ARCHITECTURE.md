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
| Incoming | aura-macros | Test harness and choreography macros |
| Outgoing | — | `SyncCore` types for synchronization state |
| Outgoing | — | `SyncProtocol`, `FactSyncProtocol`, `AuthorityJournalSync` for sync flows |
| Outgoing | — | `MerkleVerifier`, `MerkleComparison`, `VerificationResult` for integrity checks |
| Outgoing | — | `WriterFence`, `WriterFenceGuard` for write ordering |
| Outgoing | — | `MaintenanceService` for background sync operations |

## Invariants

- Sync operations must not bypass guard chain checks in runtime.
- Protocols should operate on explicit inputs (snapshot, budget, timestamp).
- Merkle verification ensures fact integrity across peers.
- Peer Biscuit validation must use a configured trust root and concrete sync
  authority scope. Missing roots or scopes fail closed; deterministic roots are
  test fixtures only.

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

### Tracing Convention

Structured tracing is required for sync service and protocol logs.

- Include `authority_id` when the effect/context provider exposes it.
- Include `peer_id` for peer-scoped sync work.
- Include `operation_id` for protocol or service phases such as `journal_sync` and `anti_entropy`.
- Include `context_id` only when a real typed context is already in scope; do not fabricate one from weaker identifiers or log-only strings.
- Prefer typed fields over interpolated message text so distributed-node log correlation stays queryable.

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

Merkle verification and anti-entropy determinism are the primary concerns. Tests are organized into three groups: `tests/integrity/` for data integrity and digest stability, `tests/protocol/` for sync protocol integration, and `tests/integration/` for multi-device and network partition scenarios. Shared deterministic fixture/time/device helpers live in [`tests/support.rs`](tests/support.rs), while [`tests/integration/test_utils.rs`](tests/integration/test_utils.rs) owns only the mechanical multi-device topology builders, bidirectional network helpers, and session-finish helpers used across scenario tests.

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

### Enrollment epoch commit wire

Enrollment peer commits use a distinct v2 signature transcript binding the
canonical proposal and both the AddLeaf and original-parent-key RotateEpoch
operation hashes. Missing legacy fences cannot authorize enrollment activation.
Rotation/removal retain the original signature transcript byte for byte. Runtime
commit ingress and retained records are bounded to one MiB. These wire proofs
do not confer activation authority: peer acceptance and activation require the
actual current participant signer and held generation/tree owners.

### Existing participant response wire

Enrollment peer acceptance v2 binds the exact active signing epoch, signing
mode, participant index and original group-package digest to the ceremony,
physical acceptor, proposal and acceptance timestamp. The individual signature
is checked under the corresponding authenticated participant verifying share;
a group signature does not identify that participant. These fields are optional
only for historical wire decoding. Actual prior-schema binary fixtures preserve
legacy bytes and verify that missing fields cannot authorize a v2 proof. Typed
missing-context failures retain their native cause. A valid wire proof still
requires runtime-owned original registration and current generation/tree custody
before durable recording or activation. Durable response recovery and genuine
multi-party quorum signing remain separate integration obligations.

## Native sync failure provenance

Required codec, transport, and journal/anti-entropy producer failures retain the original standard error source. A newly composed `SyncDiagnostic` contains only a source-free category and message; it cannot be constructed from an existing `AuraError` or terminal error. `sync_error_with_cause` attaches the original concrete error directly, preserving nested chains and Clone behavior. Diagnostic text does not authorize retries, peer trust, or terminal progress.

The lifecycle gate requires the real codec and native-category regressions plus compile-fail guards through exact source inventory, discovery, and execution. Required requested-peer completeness and session/actor lifetime remain separate contracts from source retention.

### Required requested-peer session custody

The required requested-peer path admits every distinct requested peer or returns
a typed fault. It retains exact sessions issued by the local manager across the
protocol await. Initializing and terminating sessions consume admission
capacity. Session activation cannot extend the original admitted deadline.
Required session creation checks original physical observation and checked
endpoint arithmetic before mutation. Failure or cancellation removes only the
owned local session entries; this guarantee does not imply asynchronous remote
transport closure. Legacy aggregate background discovery is a separate API.

Required local session-custody tests observe actual retained allocation IDs,
including initializing sessions. `SessionManagerStatistics::total_sessions`
counts terminal outcomes and is not allocation or retirement evidence. Exact
issued-owner Drop and cancellation must remove original records while retaining
unrelated session records; partial failed admission must leave no issued subset.
