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
## Boundaries
- No storage operations (use `StorageEffects`).
- No coordination logic (use `aura-protocol`).
- No runtime composition (use `aura-agent`).
- Uses Layer 2 fact pattern (no aura-journal dependency).
