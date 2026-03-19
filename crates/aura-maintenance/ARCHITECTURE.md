# Aura Maintenance (Layer 2) - Architecture and Invariants

## Purpose
Define maintenance domain facts, release metadata, and reduction helpers for
snapshots, cache invalidation, OTA upgrades, and admin replacement.

## Inputs
- `aura-core`: Domain types, effect traits, fact encoding.

## Outputs
- Fact types: `MaintenanceFact` (snapshots, cache, upgrades, admin replacement).
- Release-domain types: release ids, provenance, signed manifests, deterministic build certificates.
- Reducer: `MaintenanceFactReducer` producing `MaintenanceFactDelta`.
- GC planning: `TranscriptGcPlan` for DKG transcript cleanup.
- Operation categories: `MaintenanceOperation`, `OperationCategory` (A/B/C).

## Key Modules
- `facts.rs`: `MaintenanceFact` enum with all maintenance fact variants.
- `gc.rs`: `plan_dkg_transcript_gc()` for transcript garbage collection.
- `release.rs`: Pure OTA release identity, provenance, artifact, manifest, and certificate types.
- `scope.rs`: Pure OTA activation and policy scope types, intentionally without any global-network scope.

## Invariants
- Facts are immutable and merge via join-semilattice semantics.
- Maintenance facts scoped to issuing authority's journal.
- Reduction is deterministic: no clocks, randomness, or external state.
- Category A: Low-risk (cache invalidation).
- Category B: Medium-risk (snapshot operations).
- Category C: High-risk (upgrades, admin replacement).

## Ownership Model

- `aura-maintenance` is primarily `Pure`.
- It defines maintenance facts, release identity, and policy scope rather than
  `ActorOwned` staging or activation services.
- Any exclusive cutover or replacement authority should be modeled as
  `MoveOwned` higher-layer contracts.
- Capability-gated publication of maintenance facts must remain explicit.
- `Observed` layers may display maintenance state but must not author lifecycle
  truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `facts.rs`, `reducer.rs` | `Pure` | Fact schemas and deterministic maintenance reduction. |
| `release.rs`, `scope.rs`, `gc.rs` | `Pure`, `MoveOwned` | Release/scope/GC planning surfaces are value-level contracts; any exclusive cutover authority stays explicit for higher layers. |
| Actor-owned runtime state | none | Staging, rollout, and activation services belong in higher layers. |
| Observed-only surfaces | none | Observation of maintenance state belongs in runtime/interface layers. |

### Capability-Gated Points

- maintenance fact publication and high-risk operation admission consumed by
  higher-layer guards and consensus paths

### Verification Hooks

- `cargo check -p aura-maintenance`
- `cargo test -p aura-maintenance -- --nocapture`

### Detailed Specifications

### InvariantMaintenanceReducerDeterminism
Maintenance reducers must remain deterministic and high-risk operations must preserve consensus evidence requirements.

Enforcement locus:
- src maintenance facts are reduced without external clock or randomness input.
- Operation categories gate high-impact transitions.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-maintenance

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines deterministic reduction.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines agreement constraints for high-risk operations.
## Testing

### Strategy

aura-maintenance defines upgrade, snapshot, and admin replacement facts.
The critical concern is reducer determinism — if replicas disagree on
maintenance state, some may run different software versions (split-brain).

### Running tests

```
cargo test -p aura-maintenance --test determinism  # reducer determinism
cargo test -p aura-maintenance --lib               # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Reducer non-deterministic / order-dependent | `tests/determinism/` | covered |
| Fact encoding roundtrip breaks | `tests/determinism/` | covered |
| Snapshot proposal+completion lifecycle | `tests/determinism/` | covered |
| Cache invalidation additive accounting | `tests/determinism/` | covered |
| Upgrade activations independently counted | `tests/determinism/` | covered |

## Boundaries
- No storage operations (use `StorageEffects`).
- No coordination logic (use `aura-protocol`).
- No runtime composition (use `aura-agent`).
- Uses Layer 2 fact pattern (no aura-journal dependency).
