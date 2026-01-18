# Equivocation Proof Integration Guide

## Status

**Runtime Integration Complete ✓**
- Domain fact types created (`aura-consensus/src/facts.rs`)
- `EquivocationDetector` implemented with proof generation
- Detector wired into `WitnessTracker` with accumulator
- `ConsensusResult` updated with equivocation_proofs field
- `ConsensusFactReducer` registered in fact registry
- Caller integration patterns documented with examples
- All tests passing (54 unit + 2 DKG + 6 detection + 4 caller = 66 consensus tests)
- Architecture validated (domain fact pattern per docs/102_journal.md §2.2)

**Commits:**
1. `0252672b` - Domain fact refactoring
2. `7de32a70` - Integration guide
3. `945e4b8f` - Runtime integration
4. `d4c350c3` - Documentation update
5. TBD - Caller examples and final documentation

## Usage Guide (For Callers)

### Using Equivocation Detection

The runtime integration provides two methods for tracking signatures:

**Option 1: With equivocation detection (recommended for production)**
```rust
// When you have full context available
tracker.record_signature_with_detection(
    context_id,           // ContextId for this consensus
    witness,             // AuthorityId of the witness
    signature,           // PartialSignature from witness
    consensus_id,        // ConsensusId for this round
    prestate_hash,       // Hash32 of prestate
    result_id,           // Hash32 of result being voted for
    timestamp_ms,        // u64 timestamp
);
```

**Option 2: Without detection (existing API, backward compatible)**
```rust
// Simple signature tracking without equivocation checks
tracker.add_signature(witness, signature);
```

### Retrieving Equivocation Proofs

After consensus completes, retrieve accumulated proofs:

```rust
let result = run_consensus(...).await?;

// Access proofs from result
for proof in result.equivocation_proofs() {
    // Emit to journal
    let fact = proof.to_generic();
    journal_effects.add_fact(context_id, fact).await?;
}
```

Or access directly from tracker:

```rust
let proofs = tracker.get_equivocation_proofs();
for proof in proofs {
    // Process equivocation proof
}
tracker.clear_equivocation_proofs(); // Prevent duplicate emission
```

## Implementation Details

### 1. WitnessTracker Enhancement (✓ COMPLETED)

Implemented in `crates/aura-consensus/src/witness.rs`:
- Added `EquivocationDetector` field to track share history
- Added `equivocation_proofs` accumulator
- Implemented `record_signature_with_detection()` method
- Implemented `get_equivocation_proofs()` accessor
- Implemented `clear_equivocation_proofs()` for cleanup

### 2. ConsensusResult Updates (✓ COMPLETED)

Implemented in `crates/aura-consensus/src/types.rs`:
- Added `equivocation_proofs` field to all result variants
- Implemented `equivocation_proofs()` accessor method
- Backward compatible - existing code unaffected

### 3. Fact Registry Integration (✓ COMPLETED)

Implemented in `crates/aura-agent/src/fact_registry.rs` and `fact_types.rs`:
- Registered `ConsensusFactReducer` in build_fact_registry()
- Added `CONSENSUS_FACT_TYPE_ID` to central fact types list
- Added test coverage for consensus fact registration
- All 3 fact registry tests passing

### 4. Emit Facts to Journal (✓ DOCUMENTED - Caller Responsibility)

**Complete example**: `tests/equivocation_caller_example.rs`

This test file demonstrates the full integration pattern for callers (relational consensus, agent runtime):

**Pattern 1: Direct tracker integration** (recommended for custom flows):
```rust
// Caller has context information
let context_id = ContextId::new_from_entropy([1u8; 32]);
let mut tracker = WitnessTracker::new();

// Record signatures with detection enabled
tracker.record_signature_with_detection(
    context_id,
    witness,
    signature,
    consensus_id,
    prestate_hash,
    result_id,
    timestamp_ms,
);

// Extract accumulated proofs
let proofs = tracker.get_equivocation_proofs();

// Emit to journal
for proof in proofs {
    let fact = proof.to_generic();
    journal_effects.add_fact(context_id, fact).await?;
}

// Clear to prevent duplicate emission
tracker.clear_equivocation_proofs();
```

**Pattern 2: Result extraction** (for consensus protocol integration):
```rust
let result = run_consensus(...).await?;

// Extract proofs from any result variant
for proof in result.equivocation_proofs() {
    let fact = proof.to_generic();
    journal_effects.add_fact(context_id, fact).await?;
}
```

**Pattern 3: Standalone detector** (for custom validation flows):
```rust
let mut detector = EquivocationDetector::new();

if let Some(proof) = detector.check_share(
    context_id,
    witness,
    consensus_id,
    prestate_hash,
    result_id,
    timestamp_ms,
) {
    // Process equivocation immediately
    let fact = proof.to_generic();
    journal_effects.add_fact(context_id, fact).await?;
}
```

See `tests/equivocation_caller_example.rs` for complete working examples of all three patterns.

### 5. Integration Testing (TODO - Future Work)

Recommended test coverage:

**Unit Tests (✓ COMPLETED)**
- EquivocationDetector proof generation (7 tests in validation.rs)
- ConsensusFact envelope roundtrip (3 tests in facts.rs)
- Fact registry registration (3 tests in fact_registry.rs)

**Integration Tests (✓ COMPLETED)**
- `tests/equivocation_detection.rs` - End-to-end detection flow (6 tests)
- `tests/equivocation_caller_example.rs` - Caller integration patterns (4 tests)

Coverage:
1. ✓ Detection with simulated equivocating witness
2. ✓ Proof generation and accumulation
3. ✓ Multiple consensus instances (independence)
4. ✓ Duplicate vs conflicting signatures
5. ✓ Proof serialization/deserialization
6. ✓ Caller patterns for journal emission
7. ✓ Consensus result with proofs
8. TODO: End-to-end with journal propagation (requires Layer 6 runtime)

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
