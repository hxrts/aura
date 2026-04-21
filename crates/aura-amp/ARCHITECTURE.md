# Aura AMP (Layer 4)

## Purpose

Orchestrate AMP channel lifecycle and message transport coordination on top of relational journal facts.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Relational facts (channel checkpoints, epoch bumps, policies) | Direct StorageEffects for channel facts (journal only) |
| Deterministic channel state via reduction | Pure state derived outside `aura-journal` reduction |
| Non-canonical evidence cache for consensus provenance (explicitly scoped) | Hidden semantic ownership of evidence |
| AMP message serialization and wire format | Runtime composition or lifecycle management |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions, domain types |
| Down | `aura-journal` | Fact storage and reduction |
| Down | `aura-macros` | Domain fact and choreography macros |
| In | JournalEffects + OrderClockEffects | Canonical fact storage and ordering |
| In | Guard chain effects | Send authorization and budgeting (from callers) |
| In | Transport effects | Envelope delivery (via higher-level protocols) |
| Out | Relational facts | Channel checkpoints, epoch bumps, policies |
| Out | Deterministic channel state | Via reduction |

## Invariants

- AMP facts are stored in the context journal using OrderClock ordering.
- Channel epochs are monotone; committed bumps supersede proposals.
- Evidence is optional and does not affect channel state reconstruction.

### InvariantAmpEpochMonotonic

AMP channel epochs must advance monotonically and committed bumps must supersede proposals.

Enforcement locus:
- src channel and fact reducers apply epoch transitions.
- Operation category gates protect high-risk transitions.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just test-crate aura-amp`

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines monotone transition laws.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines epoch validity expectations.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-amp` combines `Pure` channel-state reduction with `MoveOwned` coordination where channel/session authority must be exclusive. Long-lived coordination is `ActorOwned` only when explicitly supervised by higher layers.

See [System Internals Guide](../../docs/807_system_internals_guide.md) §Core + Orchestrator Rule.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| channel/fact reducers and state derivation | `Pure` | Deterministic AMP state reconstruction from relational facts. |
| channel/session coordination and bootstrap authority | `MoveOwned` | Exclusive channel/session authority remains explicit at coordination boundaries. |
| long-lived AMP coordination | selective `ActorOwned` | Only where higher layers explicitly supervise ongoing coordination. |
| evidence cache surfaces | isolated support state | Evidence is explicitly non-canonical and must not become hidden semantic ownership. |
| Observed-only surfaces | none | Observation of AMP state belongs in higher layers. |

### Capability-Gated Points

- Message publication and checkpoint publication boundaries.
- Channel-policy and epoch-advancement operations consumed by higher-layer guards/runtime.

## Testing

### Strategy

Wire format stability and epoch monotonicity are the primary concerns. Integration tests in `tests/wire/` validate serialization roundtrips with property-based testing. Inline tests verify pure helper determinism.

### Commands

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

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [System Internals Guide](../../docs/807_system_internals_guide.md)
