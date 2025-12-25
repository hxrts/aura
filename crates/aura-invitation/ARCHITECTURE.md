# Aura Invitation Architecture

## Facts
- `InvitationFact` is the domain fact type stored in relational journals.
- `InvitationFactReducer` maps facts to `RelationalBinding` using `InvitationFact::binding_key()`.

## Invariants
- Facts with known context must reduce under their matching `ContextId`.
- Invitation identifiers are treated as stable binding keys.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
