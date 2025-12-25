# Aura Social Architecture

## Facts
- `SocialFact` is the domain fact type stored in relational journals.
- `SocialFactReducer` maps facts to `RelationalBinding` for view derivation.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Membership and stewardship changes should follow approved workflows.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
