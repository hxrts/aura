# Aura Consensus (Layer 4)

## Purpose

Strong-agreement protocol for single-operation consensus using FROST threshold signatures. Sole strong-agreement mechanism in Aura; all other coordination is CRDT.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Pure consensus state machine and transitions | Production effect implementations |
| FROST aggregation and pipelined commitments | Journal coupling (happens at runtime bridge) |
| Equivocation proof and evidence propagation | Runtime composition or lifecycle management |
| Type-safe share collection with threshold proof | Application-specific protocol logic |
| Distributed key generation coordination | |
| Cross-authority consensus adapter | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions, domain types |
| In | `ContextId` | Isolation boundary |
| In | `Prestate` | Commitment binding |
| In | Witness set, threshold parameters | Protocol configuration |
| In | FROST key packages, group public key | Crypto material |
| In | Crypto, time, transport, guard effects | Dependency-injected effects |
| Out | `CommitFact` | Threshold-signed agreement on proposal |
| Out | `ConflictFact` | Equivocation evidence for accountability |
| Out | Protocol messages | Execute, NonceCommit, SignRequest, SignShare, Result |

## Key Modules

- `core/`: Pure state machine, effect-free transitions.
- `protocol/`: Coordinator and witness orchestration.
- `frost/`: FROST aggregation and pipelined commitments.
- `evidence.rs`: Equivocation proof and evidence propagation.
- `shares.rs`: Type-safe share collection with threshold proof.
- `relational/`: Cross-authority consensus adapter.
- `dkg/`: Distributed key generation coordination.

## Invariants

- Single-shot: one proposal bound to one prestate.
- `CommitFact` implies threshold agreement (>= t signatures).
- No journal mutations inside protocol (charge-before-send at bridge).
- Progress requires honest threshold participation.
- Type-level guarantees: `ThresholdShareSet::combine()` only after threshold proven.

### InvariantUniqueCommitPerInstance

Consensus must produce at most one commit for each consensus id and prestate hash pair.

Enforcement locus:
- src consensus modules validate prestate binding and threshold admission.
- Evidence paths exclude equivocators from threshold calculations.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just test-crate aura-consensus`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantUniqueCommitPerInstance`.
- [Consensus](../../docs/108_consensus.md) defines protocol details.

### InvariantCommitRequiresThreshold

Every accepted commit must include a valid threshold attestation set for the configured witness policy.

Enforcement locus:
- `src/core/validation.rs`: threshold membership and attestation checks.
- `src/core/transitions.rs`: commit transitions require validated threshold evidence.
- `src/shares.rs`: type-level threshold witness collection before combine.

Failure mode:
- Commit accepted with insufficient or malformed attester set.
- Safety violation in quorum assumptions.

Verification hooks:
- `cargo test -p aura-consensus threshold`
- `quint run --invariant=InvariantCommitRequiresThreshold verification/quint/consensus/core.qnt`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantCommitRequiresThreshold`.

### InvariantEquivocatorsExcluded

Witnesses with equivocation evidence must be excluded from threshold admission and final commit calculation.

Enforcement locus:
- `src/evidence.rs`: equivocation proof ingestion and status tracking.
- `src/core/validation.rs`: threshold computation excludes flagged witnesses.
- `src/core/transitions.rs`: commit path checks evidence-aware witness eligibility.

Failure mode:
- Byzantine witnesses counted toward threshold.
- Divergent commits accepted under conflicting attestations.

Verification hooks:
- `cargo test -p aura-consensus equivocation`
- `quint run --invariant=InvariantEquivocatorsExcluded verification/quint/consensus/core.qnt`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantEquivocatorsExcluded`.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-consensus` uses `MoveOwned` for proposal, transcript, and ceremony authority that must remain exclusive. Long-lived consensus coordination may be `ActorOwned` where supervision and lifecycle matter, but that ownership stays explicit. Consensus operations require typed terminal success, failure, or abort paths.

See [System Internals Guide](../../docs/807_system_internals_guide.md) §Core + Orchestrator Rule.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `core/` | `Pure` | Deterministic consensus state machine and validation logic. |
| proposal/share/transcript/evidence types | `MoveOwned` | Exclusive proposal, share, and transcript authority remains explicit and value-based. |
| `protocol/`, `frost/`, witness/round coordinators | `ActorOwned` where long-lived | Coordinator ownership is explicit only where lifecycle/supervision matters; not the default for all logic. |
| `relational/`, `dkg/` orchestration adapters | `MoveOwned`, selective `ActorOwned` | Cross-authority coordination and DKG orchestration remain explicit about owner boundaries. |
| Observed-only surfaces | none | Projection/diagnostics stay downstream of consensus truth. |

### Capability-Gated Points

- Agreement and publication boundaries that emit consensus results/evidence.
- Guard-mediated send and runtime-bridge publication paths that consume consensus outputs.

## Testing

### Strategy

Consensus safety invariants are the highest-consequence tests in the system. `tests/safety/` validates equivocation detection, guard enforcement, and protocol coherence. `tests/contracts/` validates wire format stability and DKG transcript correctness. Inline tests cover the pure state machine.

### Commands

```
cargo test -p aura-consensus
```

### Coverage matrix

| What breaks if wrong | Invariant | Status |
|---------------------|-----------|--------|
| Two commits for same prestate | InvariantUniqueCommitPerInstance | Covered (`test_apply_share_after_commit_rejected` + Quint) |
| Commit without threshold attestation | InvariantCommitRequiresThreshold | Covered (`test_check_invariants_insufficient_witnesses` + Quint) |
| Late share alters commit | InvariantUniqueCommitPerInstance | Covered (`test_apply_share_after_commit_rejected`) |
| Equivocating witness admitted | InvariantEquivocatorsExcluded | Covered (`tests/safety/equivocation_detection.rs`) |
| Wire format breaks between versions | — | Covered (`tests/contracts/wire_compatibility.rs`) |
| DKG produces invalid threshold keys | — | Covered (`tests/contracts/dkg_transcript.rs`) |
| Guard enforcement bypassed | — | Covered (`tests/safety/guard_enforcement.rs`) |
| Orphan protocol messages accepted | — | Covered (`tests/safety/protocol_orphan_free.rs`) |

## References

- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Consensus](../../docs/108_consensus.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [System Internals Guide](../../docs/807_system_internals_guide.md)
