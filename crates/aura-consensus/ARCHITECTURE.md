# Aura Consensus (Layer 4) - Architecture and Invariants

## Purpose
Provide the strong-agreement protocol for single-operation consensus using FROST threshold
signatures. This is the only strong-agreement mechanism in Aura; all other coordination
is CRDT/monotone.

## Architecture

### Protocol Roles
- **Coordinator**: Aggregates nonce commitments, requests signatures, finalizes consensus
- **Witness**: Generates nonce commitments, creates signature shares, validates results

### Core Components

1. **Pure State Machine** (`core/`)
   - Effect-free state transitions
   - Invariant validation via Quint correspondence
   - No direct I/O or journal mutations

2. **Protocol Orchestration** (`protocol/`)
   - `coordinator.rs`: Coordinator role with FROST aggregation
   - `witness.rs`: Witness role with signature share generation
   - `logic.rs`: ConsensusProtocol coordination struct
   - `guards.rs`: Guard chain helpers for all message types

3. **Type-Safe Share Collection** (`shares.rs`)
   - `LinearShareSet`: Unsealed set accepting shares
   - `ThresholdShareSet`: Sealed set with type-level threshold proof
   - FROST aggregation via `combine()` (requires message, commitments, pubkey)

4. **Evidence Propagation** (`evidence.rs`)
   - `EquivocationProof`: Cryptographic proof of conflicting votes
   - `EvidenceDelta`: Incremental evidence propagation via messages
   - `EvidenceTracker`: Maintains known equivocation proofs

5. **Message Protocol** (`messages.rs`)
   - All messages carry `evidence_delta` for accountability
   - Execute, NonceCommit, SignRequest, SignShare, ConsensusResult
   - DAG-CBOR serialization (wire format)

## Inputs
- **Context**: `ContextId` for isolation and guard scoping
- **Prestate**: Commitment and proposal payload
- **Witness Set**: Authorities and threshold parameters
- **FROST Keys**: Key packages and group public key
- **Effects**: Crypto, time, transport, guards (dependency-injected)

## Outputs
- **CommitFact**: Threshold-signed agreement on proposal
- **Evidence**: Equivocation proofs for accountability
- **Journal Facts**: Via runtime bridge (not protocol layer)

## Invariants

### Safety
- Single-shot consensus: one proposal bound to one prestate
- CommitFact implies threshold agreement (>= t signatures)
- Result ID determinism: honest witnesses agree on operation_hash
- No journal mutations inside protocol (charge-before-send at bridge)

### Liveness
- Progress requires honest threshold participation
- Equivocation detection doesn't block honest path
- Evidence propagation is best-effort (doesn't block consensus)

### Type-Level Guarantees
- `ThresholdShareSet::combine()` only callable after threshold proven
- Linear share sets prevent duplicate insertions
- Sealed sets reject new shares after threshold

## Boundaries

### Pure/Effect Separation
- **Pure Core** (`core/`): State machine, validation, transitions
- **Effectful Protocol** (`protocol/`): Coordinator/witness orchestration
- Effects passed as parameters (dependency injection pattern)

### Guard Chain Integration
- Protocol evaluates guards before message sends
- Guard chain: CapGuard → FlowGuard → LeakageTracker → JournalCoupler
- Journal coupling at **runtime bridge**, not protocol layer

### Context Isolation
- Each consensus scoped to `ContextId` (derived from authority for ceremonies)
- Guards evaluated per-context for capability/flow budget isolation

## FROST Integration

### Aggregation
- Coordinator calls `frost_aggregate()` with partial signatures
- Requires nonce commitments from all signers
- Deterministic ordering via BTreeMap (canonical)

### Pipelining
- Fast path (cached commitments): Disabled pending interpreter integration
- Slow path (generate per round): Currently used
- Requires capability token handoff for fast path

## Evidence & Accountability

### Equivocation Detection
- Tracked per (witness, consensus_id, prestate_hash)
- Generates cryptographic proof with both conflicting result IDs
- Evidence propagates via message deltas (timestamp-based sync)

### Evidence Delta Protocol
- Attached to all consensus messages
- Coordinator/witnesses merge incoming deltas
- Only new proofs included (timestamp watermark)

## Testing

### Unit Tests
- Pure state machine: 59 tests
- Guard enforcement: 8 tests
- Wire compatibility: 8 tests (DAG-CBOR)
- Equivocation detection: 6 tests

### Integration Tests
- Full consensus rounds with evidence propagation
- FROST multi-participant setup (work in progress)
- Guard chain enforcement

## Design Patterns

### Dependency Injection
```rust
pub async fn process_coordinator_message<E>(
    &self,
    message: ConsensusMessage,
    sender: AuthorityId,
    effects: &E,  // Injected, not stored
) -> Result<Option<ConsensusMessage>>
where
    E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
```

### Journal Coupling
```rust
// Protocol creates fact, runtime commits it
let commit_fact = coordinator.finalize_consensus(cid, effects).await?;
runtime.commit_relational_facts(vec![commit_fact]).await?;
runtime.broadcast(ConsensusResult { commit_fact, ... }).await?;
```

### Type-Safe Threshold
```rust
match collector.try_insert(rid, witness, share)? {
    InsertResult::ThresholdReached(threshold_set) => {
        // Type system proves threshold met
        let sig = threshold_set.combine(msg, commitments, pubkey)?;
    }
    InsertResult::Inserted { count } => { /* wait for more */ }
}
```

## Future Work

- **ShareCollector Integration**: Currently tested but not used in protocol
- **Pipelined Commitments**: Enable fast path when interpreter supports token handoff
- **FROST Multi-Participant Tests**: Fix identifier coordination for full integration tests
- **Session Type Integration**: MPSTv2 Phase 2 (session-typed channels)
