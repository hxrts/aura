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

## Key Modules

- `src/channel.rs`: AMP channel lifecycle effect implementation.
- `src/core.rs`: pure ratchet derivation and data-plane policy helpers.
- `src/choreography.rs` / `src/choreography.tell`: AMP data/ack choreography.
- `src/epoch_transition_choreography.rs` / `src/epoch_transition.tell`: AMP
  epoch-transition choreography payloads for proposal, witness collection, A2
  certificate publication, and A3 finalization handoff.
- `src/protocol/`: high-level send/receive orchestration and telemetry.

## Invariants

- AMP facts are stored in the context journal using OrderClock ordering.
- Channel epochs are monotone; A3 committed bumps supersede proposals.
- A2-certified AMP channel epoch transitions may become live before A3
  durability only when deterministic reduction exposes exactly one
  unsuppressed successor for the parent epoch.
- AMP data-plane acceptance is subordinate to reducer-derived epoch and
  membership state; the ratchet does not invent membership truth or choose
  among competing successors.
- Emergency channel transitions are channel-scoped. They do not automatically
  mutate authority-root membership, recovery eligibility, or governance rights.
- Evidence is optional for diagnostics only; when evidence affects live
  channel state it must be represented as explicit reducer-visible facts.

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

### InvariantAmpSingleLiveSuccessor

For any `(context_id, channel_id, parent_epoch, parent_commitment)`, AMP
reduction exposes at most one live successor.

Enforcement locus:
- `aura-journal` context reduction groups AMP transition facts by parent and
  validates proposal, A2 certificate, A3 commit, abort, conflict, and
  supersession evidence.
- `aura-amp` only prepares or consumes transition facts that bind the canonical
  `transition_id`.

Failure mode:
- Two replicas with the same facts could accept different live epochs or allow
  removed members to continue sending under conflicting successor state.

Verification hooks:
- `just test-crate aura-amp`
- reducer replay tests in `aura-journal` and `aura-testkit`

Contract alignment:
- [Journal](../../docs/105_journal.md) defines fact-only deterministic
  reduction.
- [Operation Categories](../../docs/109_operation_categories.md) defines
  `A2Live` as AMP-specific operational authority, not A3 durability.

### InvariantAmpRatchetSubordination

The AMP ratchet accepts messages only for the stable epoch or the single
reducer-exposed `A2Live` successor, and only for senders present in that
epoch's membership commitment.

Enforcement locus:
- AMP send/receive validation consumes reduced channel state.
- Transition policy controls whether bounded dual-epoch receive overlap is
  allowed.

Failure mode:
- Ratchet-local fallback logic could fabricate membership truth or continue
  accepting old-epoch traffic after a subtractive or emergency transition.

Verification hooks:
- `just test-crate aura-amp`
- emergency and subtractive-transition data-plane tests

Contract alignment:
- [Transport and Information Flow](../../docs/111_transport_and_information_flow.md)
  defines AMP data-plane acceptance and emergency policy behavior.

### InvariantAmpEmergencyScope

AMP emergency quarantine and cryptoshred transitions are channel-scoped
control-plane actions. They may exclude a suspect from a channel successor
epoch and tighten data-plane/retention policy, but they do not automatically
alter authority-root membership or recovery/governance eligibility.

Enforcement locus:
- AMP emergency facts bind channel transition identity and policy class.
- Authority-scoped suspension or structural removal must use separate
  governance facts and thresholds.

Failure mode:
- A malicious or mistaken channel accusation could become an unintended
  authority-governance mutation.

Verification hooks:
- `just test-crate aura-amp`
- governance separation tests in higher layers if suspension facts are added

Contract alignment:
- [Authority and Identity](../../docs/102_authority_and_identity.md) owns
  authority-root membership.
- [Ownership Model](../../docs/122_ownership_model.md) requires scoped
  capability-gated mutation.

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
