# ADR: Async Simulator Host Boundary

## Status
Accepted (Phase 9, post-Telltale cutover)

## Context
Aura now runs choreography/session execution on Telltale backends. The simulator still uses mostly synchronous host wiring in key places, while Aura runtime integrations are async-first. We need an async host boundary for simulator integration without breaking determinism or replay guarantees.

Two options were considered:
- Simulator-only async bridge first (request/resume around current VM host interactions).
- VM-wide host trait async refactor immediately.

## Decision
Adopt **simulator-only async bridge first**.

Implement an async request/resume host boundary inside `aura-simulator`, keep VM core semantics deterministic, and preserve replay artifacts. Defer any VM-wide async host trait change until parity/regression evidence is strong.

## Determinism Constraints
- Host requests are sequenced with monotone IDs and processed FIFO.
- Replay compares normalized request/response transcript entries.
- No wall-clock time, thread interleaving, or nondeterministic RNG in host decision logic.
- Fault/scenario/network/property middleware effects must be encoded as deterministic envelope operations.

## Scope
- In scope now:
  - Async boundary API in `aura-simulator`.
  - Request/resume bridge for fault/network/scenario/property operations.
  - Sync-vs-async host parity tests on representative suites.
- Out of scope now:
  - Upstream `telltale-vm` host trait API change.
  - Native runtime behavior changes outside simulator boundary wiring.

## Consequences
- Lower migration risk: simulator gets async composition benefits without destabilizing VM core contracts.
- Clear upgrade path: if parity holds, propose upstream async host trait RFC as a separate step.
