# Aura Authenticate Architecture

## Facts
- `AuthFact` is the domain fact type stored in relational journals via `RelationalFact::Generic`.
- `AuthFactReducer` maps facts to `RelationalBinding` using `AuthFact::binding_key()`.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Session and request identifiers are treated as stable binding keys.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
