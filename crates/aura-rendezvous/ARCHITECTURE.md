# Aura Rendezvous Architecture

## Facts
- `RendezvousFact` is the domain fact type stored in relational journals.
- `RendezvousFactReducer` maps facts to `RelationalBinding` using `RendezvousFact::binding_key()`.

## Invariants
- Descriptor facts must reduce under their matching `ContextId`.
- Channel establishment requires valid, non-expired descriptors.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
