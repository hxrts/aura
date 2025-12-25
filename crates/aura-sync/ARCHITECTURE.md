# Aura Sync Architecture

## Facts
- Sync protocols exchange facts from journals; they do not define new domain facts.

## Invariants
- Sync operations must not bypass guard chain checks in runtime.
- Protocols should operate on explicit inputs (snapshot, budget, timestamp).

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
