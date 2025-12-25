# Aura Relational Architecture

## Facts
- Relational domain facts are stored via `RelationalFact::Generic`.
- Reducers map facts to `RelationalBinding` for journal reduction.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Cross-authority relationships are established through explicit consensus flows.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
