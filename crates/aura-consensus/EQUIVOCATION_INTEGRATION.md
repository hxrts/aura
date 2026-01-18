# Equivocation Proof Integration Guide

## Status

**Foundation Complete ✓**
- Domain fact types created (`aura-consensus/src/facts.rs`)
- `EquivocationDetector` implemented with proof generation
- All unit tests passing
- Architecture validated (domain fact pattern per docs/102_journal.md §2.2)

## Remaining Integration Steps

### 1. Wire Detector into WitnessTracker

**File**: `crates/aura-consensus/src/witness.rs`

Add detector to struct:
```rust
pub struct WitnessTracker {
    // ... existing fields
    equivocation_detector: EquivocationDetector,
    equivocation_proofs: Vec<ConsensusFact>,
}
```

Modify `add_signature` to check for equivocation:
```rust
pub fn add_signature(
    &mut self,
    context_id: ContextId,
    witness: AuthorityId,
    signature: PartialSignature,
    consensus_id: ConsensusId,
    prestate_hash: Hash32,
    result_id: Hash32,
    timestamp_ms: u64,
) {
    // Check for equivocation
    if let Some(proof) = self.equivocation_detector.check_share(
        context_id,
        witness,
        consensus_id,
        prestate_hash,
        result_id,
        timestamp_ms,
    ) {
        // Store proof for later emission
        self.equivocation_proofs.push(proof);
        // Don't add the equivocating signature
        return;
    }

    // Add signature normally
    self.partial_signatures.insert(witness, signature);
}
```

### 2. Surface Proofs in ConsensusResult

**File**: `crates/aura-consensus/src/types.rs`

Add evidence field to `ConsensusResult`:
```rust
pub enum ConsensusResult {
    Committed(CommitFact) {
        equivocation_proofs: Vec<ConsensusFact>,
    },
    Conflicted(ConflictFact) {
        equivocation_proofs: Vec<ConsensusFact>,
    },
    Timeout {
        consensus_id: ConsensusId,
        elapsed_ms: u64,
        equivocation_proofs: Vec<ConsensusFact>,
    },
}
```

Or add a separate `evidence()` accessor:
```rust
impl ConsensusResult {
    pub fn equivocation_proofs(&self) -> &[ConsensusFact] {
        // Extract from instance tracker
    }
}
```

### 3. Register Fact Reducer

**File**: `crates/aura-agent/src/fact_registry.rs`

```rust
use aura_consensus::facts::{ConsensusFactReducer, CONSENSUS_FACT_TYPE_ID};

pub fn build_fact_registry() -> FactRegistry {
    let mut registry = FactRegistry::new();

    // ... existing registrations

    registry.register::<ConsensusFact>(
        CONSENSUS_FACT_TYPE_ID,
        Box::new(ConsensusFactReducer),
    );

    registry
}
```

### 4. Emit Facts to Journal

**Caller** (relational consensus or agent):
```rust
let result = run_consensus(...).await?;

// Emit equivocation proofs to journal
for proof in result.equivocation_proofs() {
    let fact = proof.to_generic();
    journal_effects.add_fact(context_id, fact).await?;
}
```

### 5. Integration Testing

**File**: `crates/aura-consensus/tests/equivocation_integration.rs`

Test flow:
1. Run consensus round with simulated equivocating witness
2. Verify proof is generated and included in result
3. Verify proof is emitted to journal
4. Verify proof propagates via P2P sync
5. Verify reducer correctly processes proof

## Design Decisions

**Why domain fact, not protocol fact?**
- Not reduction-coupled (accountability record, not core invariant)
- Not cross-domain (consensus-specific)
- Derivable via FactReducer + Generic
- Per docs/102_journal.md §2.2 criteria

**Why not emit facts directly from consensus?**
- Consensus is Layer 4 (orchestration), shouldn't depend on Layer 2 (journal)
- Caller has context about which journal to emit to
- Separation of concerns: consensus produces evidence, caller persists it

**Why ContextId parameter in check_share?**
- Equivocation proofs are relational facts scoped to contexts
- Consensus can run in different contexts (account, relational, etc.)
- Proof needs context_id for journal routing

## References

- Architecture docs: `docs/102_journal.md` (domain fact contract)
- Consensus spec: `docs/104_consensus.md`
- Relational contexts: `docs/103_relational_contexts.md`
- Domain fact example: `aura-chat/src/facts.rs`
