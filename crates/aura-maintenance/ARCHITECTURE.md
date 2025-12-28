# aura-maintenance

Layer 2 domain crate defining maintenance facts and reduction helpers.

## Responsibilities
- Schema for maintenance facts (snapshots, cache invalidation, upgrades, admin replacement)
- Deterministic encoding + decoding via `DomainFact`
- Reducer that produces typed deltas for monitoring

## Invariants
- Facts are immutable and merge via join-semilattice semantics.
- Maintenance facts are stored in authority journals and scoped to the issuing authority.
- Reduction is deterministic: no clocks, randomness, or external state.

## Operation Categories
See `OPERATION_CATEGORIES` in `crates/aura-maintenance/src/lib.rs` for the current A/B/C table.
