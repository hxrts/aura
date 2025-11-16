# Consensus Protocol Specification

## Overview

This document specifies Aura's consensus protocol as a capability-gated, fact-producing protocol within a semilattice system. Consensus in Aura is not a global state machine. Instead, consensus is an opt-in protocol whose output is a monotone commit fact merged into the journal.

Atomicity derives from agreement on which fact exists. Dissemination combines CRDT merge operations with session-typed gossip. All correctness properties follow from Aura's algebraic semantics where facts grow by join and capabilities shrink by meet.

The protocol uses FROST threshold signatures to prove agreement. Session types enforce message structure and evidence propagation. Nodes retain local sovereignty by choosing when to participate. Atomicity arises from global convergence to a single monotone fact.

## System Model

### Participation and Authorization

Each node maintains a local capability frontier `Caps(ctx)` and a policy capability `Policy`. Participation in consensus requires the guard predicate `need(Consensus(C)) ≤ Caps(ctx)` to hold. Nodes may refuse or accept participation independently based on local policy.

### Network Assumptions

The system operates under distributed asynchrony with unreliable transports including Bluetooth, WiFi, and Internet connections. The network may experience arbitrary partitions. No global clock or block time exists. The protocol requires no leader, coordinator, or epoch boundaries.

Consensus relies only on local timers. Recovery from partitions occurs through CRDT merge operations when connectivity resumes.

### Consistency Model

Most updates in Aura use conflict-free replicated data types for eventual consistency. A small subset of operations requires strong consistency. These operations include account recovery and membership changes. Consensus produces commit facts for these operations.

A node rejoining after partition simply merges the journal and regains all commits through CRDT convergence. No special recovery protocol exists.

## Consensus as Fact Emission

Consensus on command `C` produces a protocol that causes all honest participating nodes to merge `CommitFact(C)` into their journals if successful. A commit does not mutate state directly. It emits an append-only fact.

The commit fact has the following structure:

```
Fact::Commit {
   command_id: C,
   result_hash,
   threshold_sig,
   attesters: ORSet<NodeId>,
   vector_clock
}
```

This fact merges via CRDT join operation. The fact is monotone and append-only. It serves as the only authoritative representation of the commit. Atomicity equals convergence to a unique final fact per command.

## Threshold Signatures

### Key Configuration

Each consensus context maintains FROST key shares. The configuration includes a per-node secret key share `sk_i`, a group public key `pk_group`, and a threshold value `t` defining the quorum size.

Both fast path and fallback use the same FROST key configuration. Witnesses reuse their shares from the fast path when proposing in fallback if their result matches. New shares are generated only for different results.

### Witness Shares

The witness protocol completes in one round-trip time by piggybacking both FROST rounds. A witness share contains the command identifier, result hash, vector clock, node identifier, round 1 commitment, and round 2 share.

```
WitnessShare {
   command_id,
   result_hash,
   vector_clock,
   node_id,
   round1_commitment,
   round2_share
}
```

Signature shares validate only for the specific command `C`. This prevents replay attacks across different consensus instances.

## Evidence Tracking

### Evidence CRDT Structure

The protocol tracks commit evidence using a distributed CRDT. The evidence structure contains the threshold signature using validity-first merge, an OR-set of attester node identifiers, and a minimum vector clock for first observation time.

```
struct Evidence {
   threshold_sig: ValidityFirst<Option<ThresholdSig>>,
   attesters: ORSet<NodeId>,
   first_seen_clock: MinVC<VectorClock>
}
```

The `ValidityFirst` merge rule for threshold signatures prefers any valid signature over None. If both are valid (impossible for correct protocol), it keeps the first seen. It never replaces a valid signature with None or invalid.

The `MinVC` merge takes the earliest vector clock under happens-before partial order. If clocks are concurrent, it selects the one with the smallest lexicographic device identifier. This tie-breaker is purely for determinism and has no safety role.

All merge operations are monotone. Evidence for all commands stores in a map CRDT keyed by command identifier.

```
EvidenceCRDT : MapCRDT<CommandId, Evidence>
```

The evidence CRDT guarantees idempotence, conflict freedom, and safe recovery after partition.

### Session-Type Evidence Propagation

Messages referring to command `C` carry an evidence delta annotation `[▷ Δ = EvidenceDelta(C)]`. This is a journal delta injected by choreography projection.

Projection inserts `do merge(EvidenceDelta(C)); !Message;` on send and `?Message; do merge(EvidenceDelta(C));` on receive. Evidence propagation becomes strictly enforced. The protocol cannot forget or skip evidence updates. Context isolation prevents interference between concurrent consensus instances.

## Core Safety vs Dissemination

The protocol clearly separates core safety mechanisms from dissemination patterns:

### Core Safety (Direct Communication)
- **Fast path**: Initiator ↔ Witnesses directly, no intermediaries
- **Threshold collection**: Direct point-to-point for FROST shares
- **Safety guarantee**: t-of-n threshold signatures ensure authenticity

### Dissemination (Random Forwarding)
- **Fallback**: Random k-committee forwarding for aggregation
- **Evidence propagation**: Epidemic gossip with bounded fanout
- **Commit facts**: Priority-based dissemination after consensus

This separation ensures the core quorum construction remains simple and analyzable while leveraging epidemic algorithms for efficient propagation.

## Protocol Phases

### Initiation

A node initiates consensus only if the guard `need(Consensus(C)) ≤ Caps(ctx)` holds. The initiator prepares the command `C`, creates an execution plan, and initializes an empty evidence entry. The initiator may emit an optional `Fact::Prepare(C)` which is monotone.

### Execute Phase

The initiator broadcasts `Execute(C, ΔEvidence(C))` to all peers. Each peer executes `C` deterministically and returns a round 1 FROST commitment, vector clock, result hash, and attester identity. Peers update the evidence CRDT by adding themselves to the attester set.

This phase piggybacks FROST round 1 commitments onto the execute request. No separate FROST message exchange occurs.

### Witness Phase

Peers return `WitnessShare(..., ΔEvidence(C))` to the initiator. The initiator collects at least `t` matching shares where `t` is the configured threshold. The matching predicate is:

```
match(share_i, share_j) = (share_i.result_hash == share_j.result_hash)
```

Only the result hash determines matching. Vector clocks are included for causal tracking but do not affect the matching decision. This phase piggybacks FROST round 2 shares onto witness responses. The entire FROST protocol completes in one round-trip time.

### Commit Emission

The initiator combines collected shares using `threshold_sig = frost_combine(share[0..t])`. The initiator then writes a commit fact with the command identifier, result hash, threshold signature, accumulated attester OR-set, and vector clock.

The initiator broadcasts `Commit(C, threshold_sig, ΔEvidence(C))` to all participants. Each peer verifies the signature in constant time, merges the commit fact into its local journal, merges the evidence delta, and derives deterministic application effects.

### Conflict Resolution

If FROST witnesses produce inconsistent results, the protocol invokes the fallback choreography using concurrent multi-proposer consensus. Each witness proposes its result while accumulating threshold signatures through epidemic propagation.

Witnesses track multiple proposals concurrently in `proposals: BTreeMap<Hash, Vec<(DeviceId, FrostShare)>>`. Each `proposals[h]` must have at most one share per witness w, and additional shares from the same w for the same h are rejected. A witness must reject any FROST shares that are inconsistent with prior shares from the same witness for the same proposal. Malformed or equivocal shares are dropped and recorded as Byzantine evidence.

Each witness forwards to k randomly chosen committee members. When any witness accumulates threshold shares, it broadcasts the complete signature via CRDT delta. The first valid threshold signature to propagate through the CRDT system wins. Once a node has accepted a valid threshold_sig for C, later conflicting signatures are ignored by the ValidityFirst merge rule. CRDT convergence plus deterministic tie-breaking ensures all honest nodes eventually agree on the same threshold_sig. This ensures `Fact::Commit(C)` with the winning result. No rollbacks occur due to monotonicity and join semantics.

## Gossip Dissemination

Gossip operates as a separate effect independent of consensus logic. The gossip effect is `GossipSpread(C, ΔEvidence(C))`. Evidence and commit facts use k-fanout epidemic algorithm over all reachable peers, not full broadcast.

### Fanout Parameters

Each `GossipSpread` invocation uses a fanout `k` chosen per priority class. These are recommended defaults. Deployments may tune fanout per priority class subject to FlowBudget limits:

- **Consensus-critical**: k=4 (threshold signatures, commit facts)
- **CRDT updates**: k=2 (evidence deltas, journal merges)  
- **Background**: k=1 (anti-entropy, state sync)

This bounds FlowBudget use while retaining O(log n) expected convergence time. Here `n` is the estimated number of peers in the relevant gossip domain (committee members for consensus, all peers for general dissemination).

### Scheduling

Scheduling is local to each node with exponential backoff:
- Initial delay: 10ms * priority_multiplier
- Backoff factor: 2^attempt
- Max attempts: log₂(n) + 2

Session-type projection ensures all outgoing messages automatically include `do merge(EvidenceDelta(C))`. Convergence follows epidemic dissemination guarantees with high probability after O(log n) rounds.

## Recovery and Rejoin

A rejoining node performs journal synchronization to obtain all facts and evidence CRDT entries. The node merges all commit facts using CRDT join operations. The node replays deterministic effects based on merged facts.

No consensus-specific recovery exists. Monotonicity of facts and evidence suffices for recovery. The rejoining node sees the same final state as continuously connected nodes after CRDT convergence.

## Safety and Liveness Properties

### Safety

Threshold signatures ensure authenticity of commit evidence. Capability guards ensure only authorized nodes participate. Session types ensure message structure correctness. The evidence CRDT is monotone and idempotent. Commit facts are unique per command due to join-semilattice convergence.

Lemma (Deterministic Safety): Consensus safety depends on deterministic command execution. If all honest witnesses execute command C deterministically and produce the same result_hash, then at most one threshold signature can form for C. Byzantine witnesses cannot forge threshold signatures without t honest participants.

The result hash is computed as `result_hash = H(C, prestate)` where prestate is the join of all facts accepted up to executing C. All honest witnesses execute C against the same logical prestate. Given CRDT convergence and determinism, any differences in arrival order of monotone facts do not affect result_hash.

Byzantine Behavior Handling:
- Malformed FROST shares: Ignored during aggregation, logged as Byzantine evidence
- Equivocation (sending different shares): First valid share wins via CRDT merge
- Non-participation: Epidemic aggregation continues via other peers; non-responsive witnesses are skipped by random forwarding
- Invalid signatures: Rejected by ValidityFirst merge rule in evidence CRDT
- Multiple proposals from same witness: Deterministic selection by hash(proposal)

### Liveness

The fast path completes in one round-trip time under normal operation. Epidemic dissemination ensures eventual convergence under partition. The fallback path terminates if conflicting evidence exists. No global timing assumptions exist. Offline nodes catch up through merge-only recovery.

## Formal Choreography

This section presents the consensus choreographies in two complementary notations. The global type notation provides theoretical clarity and formal reasoning. The Aura DSL notation provides executable implementation with type safety and effect integration.

### Message Type Definitions

The consensus protocol uses structured message types carrying command data, execution results, and cryptographic proofs:

```rust
pub struct ExecuteMessage<C> {
    command: C,
    evidence_delta: EvidenceDelta,
    initiator_id: DeviceId,
}

pub struct WitnessShareMessage<C> {
    command_id: CommandId,
    result_hash: Hash,
    vector_clock: VectorClock,
    node_id: DeviceId,
    round1_commitment: FrostCommitment,
    round2_share: FrostShare,
    evidence_delta: EvidenceDelta,
}

pub struct CommitMessage<C> {
    command_id: CommandId,
    result_hash: Hash,
    threshold_sig: ThresholdSignature,
    attesters: BTreeSet<DeviceId>,
    vector_clock: VectorClock,
    commit_evidence_delta: EvidenceDelta,
}

pub struct ConflictReportMessage<C> {
    command_id: CommandId,
    conflicting_results: Vec<(Hash, Vec<DeviceId>)>,
    evidence_delta: EvidenceDelta,
    initiator_id: DeviceId,  // Identifies who detected the conflict
}

pub struct AggregateShareMessage<C> {
    command_id: CommandId,
    sender_id: DeviceId,
    proposals: BTreeMap<Hash, Vec<(DeviceId, FrostShare)>>,  // Multiple proposals being aggregated
    round_number: u32,  // Tracks epidemic round for debugging
    evidence_delta: EvidenceDelta,
}

pub struct ThresholdCompleteMessage<C> {
    command_id: CommandId,
    winning_proposal: Hash,
    threshold_signature: ThresholdSignature,
    contributing_witnesses: BTreeSet<DeviceId>,
    evidence_delta: EvidenceDelta,
}
```

### Main Consensus Choreography

The fast-path consensus choreography uses FROST threshold signatures for one round-trip time agreement.

#### Global Type Notation

The main consensus choreography in global type notation:

```
Consensus(C) ::= μ X.
  I -> * : Execute<C>
    [ guard: Γ_exec(I),
      ▷ Δ_exec,
      leak: L_exec ] .

  ( ∥_{w ∈ W}
      w -> I : WitnessShare<C>
        [ guard: Γ_witness(w),
          ▷ Δ_wit,
          leak: L_wit ]
  ) .

  I ▷ {
    enough_shares :
      I -> * : Commit<C>
        [ guard: Γ_commit(I),
          ▷ Δ_commit,
          leak: L_commit ] .
      end

  | conflict :
      FallbackConsensus(C)
  }
```

Guards enforce capability requirements. `Γ_exec(r)` checks `need(Execute(C)) ≤ caps_r(ctx)`. `Γ_witness(r)` checks `need(WitnessShare(C)) ≤ caps_r(ctx)`. `Γ_commit(r)` checks `need(Commit(C)) ≤ caps_r(ctx)`.

Journal deltas propagate evidence. `Δ_exec` applies `EvidenceDelta(C)` on execute messages. `Δ_wit` applies `EvidenceDelta(C)` on witness messages. `Δ_commit` applies `CommitEvidenceDelta(C)` including the commit fact.

The initiator `I` broadcasts execute to all witnesses `W`. Witnesses respond in parallel with witness shares. The initiator makes an internal choice based on collected shares. The fast path emits a commit with threshold signature. The conflict path invokes fallback consensus.

#### Aura DSL Implementation

The same choreography in executable Aura DSL:

```rust
choreography! {
    #[namespace = "consensus"]
    protocol Consensus {
        roles: Initiator, Witness[N];
        
        // Phase 1: Broadcast execute command to all witnesses
        Initiator[guard_capability = "consensus.execute",
                  journal_facts = "consensus_execute_sent"]
        -> Witness[*]: ExecuteMessage(ExecuteData);
        
        // Phase 2: Witnesses execute and return FROST shares in parallel
        Witness[i][guard_capability = "consensus.witness",
                   journal_facts = "witness_share_sent"]
        -> Initiator: WitnessShareMessage(WitnessData);
        
        // Phase 3: Initiator decides based on collected shares
        choice Initiator {
            enough_shares: {
                Initiator[guard_capability = "consensus.commit",
                          journal_facts = "consensus_committed"]
                -> Witness[*]: CommitMessage(CommitData);
            }
            
            conflict: {
                // Invoke fallback consensus protocol
                call FallbackConsensus;
            }
        }
    }
}
```

The initiator broadcasts the execute message to all witnesses. Each witness executes the command deterministically and sends back a witness share containing FROST round 1 and round 2 data. The initiator collects shares and makes an internal choice. If enough matching shares exist, the initiator combines them into a threshold signature and broadcasts commit. If shares conflict, the protocol invokes the fallback consensus choreography.

### Fallback Consensus Choreography

The fallback choreography is a threshold-signature race using random committee forwarding. This is not Paxos-like. Instead, witnesses accumulate threshold signatures through epidemic-style propagation over a sparse random overlay, with the first valid signature winning through CRDT convergence.

#### Global Type Notation

The fallback choreography uses random k-committee forwarding for efficient threshold accumulation:

```
FallbackConsensus(C) ::= μ X.
  I -> * : ConflictReport<C>
    [ guard: Γ_conflict(I),
      ▷ ΔEvidence(C),
      leak: L_conflict ] .

  // Aggregation phase
  ( ∥_{w ∈ W}
      w -> W_subset : AggregateShare<C>
        [ guard: Γ_aggregate(w),
          ▷ ΔEvidence(C),
          leak: L_aggregate ]
  ) .

  // Threshold detection and broadcast
  ( ∥_{w ∈ W}
      w ▷ {
        threshold_reached :
          w -> W : ThresholdComplete<C>
            [ guard: Γ_complete(w),
              ▷ ΔEvidence(C),
              leak: L_complete ] .
          end
        
      | continue :
          do merge(ΔEvidence(C)) .
          X
      }
  )
```

Note: `W_subset` denotes sending to k randomly selected witnesses. `W_subset` and `sample_k` describe the operational overlay; from the MPST perspective this is still a family of send actions to roles in W. The `loop` and `sample_k` notations describe operational strategy. The protocol may terminate early once `EvidenceCRDT[C].threshold_sig.is_some()` at all nodes.

All witnesses propose concurrently. Each witness forwards its aggregate state to k randomly chosen committee members (fanout parameter k=3 for consensus-critical). Witnesses accumulate FROST shares through epidemic propagation. When any witness accumulates threshold shares, it broadcasts the complete signature.

The protocol terminates early when `EvidenceCRDT[C].threshold_sig.is_some()`. Expected convergence in O(log n) rounds with high probability. Once a valid threshold signature exists in the evidence CRDT, all witnesses converge on the same decision through merge operations.

#### Aura DSL Implementation

The same choreography in executable Aura DSL using random committee forwarding:

```rust
use aura_macros::choreography;

choreography! {
    #[namespace = "consensus"]
    protocol FallbackConsensus {
        roles: Initiator, Witness[N];
        
        // Phase 1: Broadcast conflict to all witnesses
        Initiator[guard_capability = "consensus.report_conflict",
                  journal_facts = "conflict_reported"]
        -> Witness[*]: ConflictReportMessage(ConflictData);
        
        // Phase 2: Epidemic aggregation with random k-committee forwarding
        // Fanout k=3 for consensus-critical operations
        loop (rounds < log(N)) {
            Witness[i][guard_capability = "consensus.aggregate_shares",
                       flow_cost = 150,
                       journal_facts = "shares_aggregated",
                       journal_merge = true]  // Enable CRDT delta propagation
            -> Witness[sample_k(3)]: AggregateShareMessage(AccumulatedShares);
            
            // Each witness checks if threshold reached
            choice Witness[i] {
                threshold_reached: {
                    // If accumulated shares reach threshold, broadcast completion
                    Witness[i][guard_capability = "consensus.threshold_complete",
                               flow_cost = 200,
                               journal_facts = "threshold_reached",
                               journal_merge = true]
                    -> Witness[*]: ThresholdCompleteMessage(ThresholdSignature);
                }
                
                continue_aggregation: {
                    // Local effect: merge CRDT state, not a network message
                    do merge(ReceivedShares);
                }
            }
            
            // Early termination if threshold signature exists
            if (threshold_sig.is_some()) {
                break;
            }
        }
        
        // Local effect: converge on decision, not a network message
        do converge(ThresholdSignature);
    }
}
```

The initiator broadcasts the conflict to all witnesses. Each witness forwards its aggregate state to k=3 randomly chosen committee members per round. Witnesses accumulate FROST shares through epidemic propagation. When any witness accumulates threshold shares, it broadcasts the complete signature. All witnesses converge on the same decision through CRDT merge operations.

Message complexity: O(kn log n) = O(n log n) for constant fanout k. Expected convergence in O(log n) rounds with high probability.

## Integration with Aura Calculus

Consensus integrates with Aura's theoretical foundations. Capabilities govern participation through meet-semilattice refinement. Commits are facts with join-semilattice merge semantics. Evidence and results use CRDT composition. Session types enforce structural correctness and evidence propagation. The effect runtime executes all operations through merge, refine, send, and receive effects.

Local sovereignty and atomicity coexist through this design. A node may refuse consensus due to insufficient capabilities. Once a node accepts a commit fact, global atomicity follows from CRDT convergence. The system achieves distributed agreement while respecting local autonomy.
