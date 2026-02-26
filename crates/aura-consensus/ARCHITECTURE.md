# Aura Consensus (Layer 4) - Architecture and Invariants

## Purpose
Strong-agreement protocol for single-operation consensus using FROST threshold
signatures. Sole strong-agreement mechanism in Aura; all other coordination is CRDT.

## Inputs
- `ContextId` for isolation, `Prestate` for commitment binding.
- Witness set (authorities) and threshold parameters.
- FROST key packages and group public key.
- Effects: Crypto, time, transport, guards (dependency-injected).

## Outputs
- `CommitFact`: Threshold-signed agreement on proposal.
- `ConflictFact`: Equivocation evidence for accountability.
- Protocol messages (Execute, NonceCommit, SignRequest, SignShare, Result).

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
- `CommitFact` implies threshold agreement (≥ t signatures).
- No journal mutations inside protocol (charge-before-send at bridge).
- Progress requires honest threshold participation.
- Type-level guarantees: `ThresholdShareSet::combine()` only after threshold proven.

## Boundaries
- Pure core (`core/`) has no effects; orchestration (`protocol/`) has effects.
- Guard chain: CapGuard → FlowGuard → LeakageTracker → JournalCoupler.
- Journal coupling at runtime bridge, not protocol layer.
- Effects passed as parameters (dependency injection).
