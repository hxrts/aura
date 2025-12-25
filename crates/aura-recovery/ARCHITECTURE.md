# Aura Recovery Architecture

## Facts
- `RecoveryFact` is the domain fact type stored in relational journals.
- `RecoveryFactReducer` maps facts to `RelationalBinding` using `RecoveryFact::binding_key()`.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Recovery and guardian membership transitions are consensus-gated (Category C).

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
