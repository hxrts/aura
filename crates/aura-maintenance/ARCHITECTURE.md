# Aura Maintenance (Layer 2) - Architecture and Invariants

## Purpose
Define maintenance domain facts and reduction helpers for snapshots, cache
invalidation, OTA upgrades, and admin replacement.

## Inputs
- `aura-core`: Domain types, effect traits, fact encoding.

## Outputs
- Fact types: `MaintenanceFact` (snapshots, cache, upgrades, admin replacement).
- Reducer: `MaintenanceFactReducer` producing `MaintenanceFactDelta`.
- GC planning: `TranscriptGcPlan` for DKG transcript cleanup.
- Operation categories: `MaintenanceOperation`, `OperationCategory` (A/B/C).

## Key Modules
- `facts.rs`: `MaintenanceFact` enum with all maintenance fact variants.
- `gc.rs`: `plan_dkg_transcript_gc()` for transcript garbage collection.

## Invariants
- Facts are immutable and merge via join-semilattice semantics.
- Maintenance facts scoped to issuing authority's journal.
- Reduction is deterministic: no clocks, randomness, or external state.
- Category A: Low-risk (cache invalidation).
- Category B: Medium-risk (snapshot operations).
- Category C: High-risk (upgrades, admin replacement).

## Boundaries
- No storage operations (use `StorageEffects`).
- No coordination logic (use `aura-protocol`).
- No runtime composition (use `aura-agent`).
- Uses Layer 2 fact pattern (no aura-journal dependency).
