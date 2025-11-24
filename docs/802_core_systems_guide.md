# Core Systems Guide: Time Domain Selection

This guide gives developers a concise decision path for choosing and using Aura's unified time domains. The goal is to prevent accidental leakage and keep causal semantics correct.

## When to use each domain

- **PhysicalClock (`PhysicalTimeEffects`)**: Anything tied to wall time or user-facing clocks (cooldowns, receipt timestamps, liveness checks, maintenance uptime).
- **LogicalClock (`LogicalClockEffects`)**: Causal ordering for CRDTs, message happens-before, replay prevention within choreographies.
- **OrderClock (`OrderClockEffects`)**: Privacy-preserving total ordering when you need determinism but must avoid revealing timing or causality (e.g., shuffle ordering, batching).
- **Range**: Validity windows and policies that bound acceptable skew or dispute periods. Compose with PhysicalClock plus policy-derived uncertainty.
- **ProvenancedTime (`TimeAttestationEffects`)**: When a timestamp must be attested/consensus-backed (e.g., consensus commits, multi-party receipts).

## Rules of thumb

- Always request time through effect traits; never call `SystemTime::now()`/chrono from application code.
- Pick the narrowest domain that satisfies the requirement; avoid PhysicalClock when LogicalClock or OrderClock suffices.
- Compare timestamps with `TimeStamp::compare(policy)`; do not cast between domains unless policy explicitly allows it.
- Facts store `TimeStamp` directly. Do not introduce legacy IDs or raw `u64` timestamps in new fact schemas.
- Simulator and testkit already provide deterministic implementations for all domains—use them in tests to keep runs reproducible.

## Common patterns

- **Cooldowns/disputes**: PhysicalClock for start time + Range for validity window; apply policy-defined uncertainty.
- **CRDT merges**: LogicalClock for happens-before; fall back to deterministic tie-break only when policy allows.
- **Gossip batching/shuffle**: OrderClock tokens to order batches without leaking timing.
- **Consensus commits**: PhysicalClock + ProvenancedTime for attested timestamps on facts or receipts.

## Anti-patterns to avoid

- Mixing domains implicitly (e.g., sorting LogicalClock and PhysicalClock in the same list without policy).
- Using PhysicalClock for ordering when privacy matters; prefer OrderClock.
- Reintroducing legacy `FactId`/UUID ordering as a proxy for time.
- Embedding `SystemTime` or chrono types in public APIs; use `TimeStamp`.
