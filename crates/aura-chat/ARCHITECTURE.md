# Aura Chat Architecture

## Facts
- `ChatFact` is the domain fact type stored in relational journals.
- `ChatFactReducer` maps facts to `RelationalBinding` for view derivation.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Message payloads are opaque bytes; decryption is a higher-layer concern.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
