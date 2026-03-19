# Aura AMP (Layer 4) - Architecture and Invariants

## Purpose
Orchestrate AMP channel lifecycle and message transport coordination on top of
relational journal facts.

## Inputs
- JournalEffects + OrderClockEffects for canonical fact storage and ordering.
- Guard chain effects from callers for send authorization and budgeting.
- Transport effects for envelope delivery (via higher-level protocols).

## Outputs
- Relational facts (channel checkpoints, epoch bumps, policies).
- Deterministic channel state via reduction.
- Non-canonical evidence cache for consensus provenance (explicitly scoped).

## Invariants
- AMP facts are stored in the context journal using OrderClock ordering.
- Channel epochs are monotone; committed bumps supersede proposals.
- Evidence is optional and does not affect channel state reconstruction.

## Ownership Model

- `aura-amp` combines `Pure` channel-state reduction with `MoveOwned`
  coordination where channel/session authority must be exclusive.
- It should avoid implicit shared ownership of channel lifecycle.
- Long-lived coordination should be `ActorOwned` only when explicitly
  supervised by higher layers.
- Capability-gated message and checkpoint publication must remain explicit.
- `Observed` consumers may render AMP state but not author it.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| channel/fact reducers and state derivation | `Pure` | Deterministic AMP state reconstruction from relational facts. |
| channel/session coordination and bootstrap authority | `MoveOwned` | Exclusive channel/session authority remains explicit at coordination boundaries. |
| long-lived AMP coordination | selective `ActorOwned` | Only where higher layers explicitly supervise ongoing coordination. |
| evidence cache surfaces | isolated support state | Evidence is explicitly non-canonical and must not become hidden semantic ownership. |
| Observed-only surfaces | none | Observation of AMP state belongs in higher layers. |

### Capability-Gated Points

- message publication and checkpoint publication boundaries
- channel-policy and epoch-advancement operations consumed by higher-layer
  guards/runtime

### Verification Hooks

- `cargo check -p aura-amp`
- `cargo test -p aura-amp -- --nocapture`

### Detailed Specifications

### InvariantAmpEpochMonotonic
AMP channel epochs must advance monotonically and committed bumps must supersede proposals.

Enforcement locus:
- src channel and fact reducers apply epoch transitions.
- Operation category gates protect high-risk transitions.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-amp

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines monotone transition laws.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines epoch validity expectations.
## Testing

### Strategy

Wire format stability and epoch monotonicity are the primary concerns.
Integration tests in `tests/wire/` validate serialization roundtrips with
property-based testing. Inline tests verify pure helper determinism.

### Running tests

```
cargo test -p aura-amp
```

### Coverage matrix

| What breaks if wrong | Invariant | Status |
|---------------------|-----------|--------|
| AMP epoch not monotonic | InvariantAmpEpochMonotonic | aura-journal reduction (cross-crate) |
| AMP message serialization breaks | — | Covered (`tests/wire/amp_wire.rs`, 11 tests) |
| Serialization non-deterministic | — | Covered (proptest in `tests/wire/`) |
| Different epochs produce same bytes | — | Covered (`test_different_headers_produce_different_bytes`) |
| Nonce derivation non-deterministic | — | Covered (`src/core.rs` inline) |
| Ratchet state conversion lossy | — | Covered (`src/core.rs` inline) |

## Boundaries
- No direct StorageEffects for channel facts (journal only).
- Evidence storage is isolated behind AmpEvidenceEffects.
- Pure state is derived via `aura-journal` reduction.

## Core + Orchestrator Rule
- Pure helpers live under `amp/core`.
- Orchestrators must depend on effects explicitly.
