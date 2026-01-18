# Equivocation Detection Architecture

## Overview

This document explains the architectural design and implementation of the equivocation detection system in aura-consensus. Equivocation occurs when a witness submits conflicting votes (different result IDs) for the same consensus instance, which is a protocol violation that must be detected and proven cryptographically.

## Core Components

### 1. Domain Fact System (`facts.rs`)

**Design Decision**: Equivocation proofs are implemented as **domain facts** rather than protocol facts.

**Rationale** (per `docs/102_journal.md` §2.2):
- **Not reduction-coupled**: Equivocation proofs are accountability records, not core state invariants
- **Not cross-domain**: Consensus-specific evidence, doesn't span multiple domains
- **Derivable**: Can be reduced via `FactReducer` pattern without special journal handling

**Implementation**:
```rust
pub enum ConsensusFact {
    EquivocationProof(EquivocationProof),
}

pub struct EquivocationProof {
    pub context_id: ContextId,        // Journal routing
    pub witness: AuthorityId,         // Equivocating authority
    pub consensus_id: Hash32,         // Which consensus instance
    pub prestate_hash: Hash32,        // State binding
    pub first_result_id: Hash32,      // First vote
    pub second_result_id: Hash32,     // Conflicting vote
    pub timestamp: PhysicalTime,      // When detected
}
```

**Benefits**:
- Clean separation of concerns (consensus generates evidence, Layer 6 emits to journals)
- Follows extensibility pattern for domain-specific facts
- No protocol-level fact complexity
- Explicit context_id for journal routing

### 2. Equivocation Detector (`core/validation.rs`)

**Design Decision**: Stateful detector that tracks share history per (witness, consensus_id, prestate_hash).

**Implementation**:
```rust
pub struct EquivocationDetector {
    share_history: HashMap<(AuthorityId, ConsensusId, Hash32), (Hash32, u64)>,
}

impl EquivocationDetector {
    pub fn check_share(...) -> Option<ConsensusFact> {
        let key = (witness, consensus_id, prestate_hash);

        match self.share_history.get(&key) {
            None => {
                // First share - record and accept
                self.share_history.insert(key, (result_id, timestamp_ms));
                None
            }
            Some((existing_rid, _)) => {
                if *existing_rid == result_id {
                    None  // Duplicate, not equivocation
                } else {
                    Some(ConsensusFact::EquivocationProof(...))  // Conflict!
                }
            }
        }
    }
}
```

**Key Properties**:
- **Stateful**: Remembers first vote to detect conflicts
- **Per-instance isolation**: Different consensus instances tracked independently
- **Duplicate vs Conflict**: Distinguishes between harmless duplicates and actual equivocation
- **Cryptographic proof**: Captures both conflicting result IDs

**Why not stateless?**
- Equivocation is inherently stateful (need to remember prior votes)
- Alternative would require passing history externally (less ergonomic)
- Detector lifecycle matches consensus round (acceptable state scope)

### 3. WitnessTracker Integration (`witness.rs`)

**Design Decision**: Opt-in equivocation detection via separate method.

**Implementation**:
```rust
pub struct WitnessTracker {
    partial_signatures: HashMap<AuthorityId, PartialSignature>,
    equivocation_detector: EquivocationDetector,
    equivocation_proofs: Vec<ConsensusFact>,
}

impl WitnessTracker {
    // Existing API (no detection)
    pub fn add_signature(&mut self, witness, signature) { ... }

    // Enhanced API (with detection) - opt-in
    pub fn record_signature_with_detection(
        &mut self,
        context_id: ContextId,
        witness: AuthorityId,
        signature: PartialSignature,
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        result_id: Hash32,
        timestamp_ms: u64,
    ) {
        if let Some(proof) = self.equivocation_detector.check_share(...) {
            self.equivocation_proofs.push(proof);
            // Reject equivocating signature
            return;
        }
        self.partial_signatures.insert(witness, signature);
    }

    pub fn get_equivocation_proofs(&self) -> &[ConsensusFact] {
        &self.equivocation_proofs
    }

    pub fn clear_equivocation_proofs(&mut self) {
        self.equivocation_proofs.clear();
    }
}
```

**Benefits**:
- **Backward compatible**: Existing `add_signature()` unchanged
- **Opt-in**: Callers choose when to enable detection (when they have full context)
- **Accumulation**: Proofs stored for batch emission
- **Clear lifecycle**: Explicit clear prevents duplicate emissions

**Why two methods?**
- Protocol layer doesn't always have `context_id` (lower abstraction level)
- Callers at Layer 5/6 have full context and control journal emission
- Separation of concerns: protocol detects, caller persists

### 4. ConsensusResult Propagation (`types.rs`)

**Design Decision**: Include equivocation proofs in all result variants.

**Implementation**:
```rust
pub enum ConsensusResult {
    Committed {
        commit: CommitFact,
        equivocation_proofs: Vec<ConsensusFact>,
    },
    Conflicted {
        conflict: ConflictFact,
        equivocation_proofs: Vec<ConsensusFact>,
    },
    Timeout {
        consensus_id: ConsensusId,
        elapsed_ms: u64,
        equivocation_proofs: Vec<ConsensusFact>,
    },
}

impl ConsensusResult {
    pub fn equivocation_proofs(&self) -> &[ConsensusFact] {
        match self {
            ConsensusResult::Committed { equivocation_proofs, .. } => equivocation_proofs,
            ConsensusResult::Conflicted { equivocation_proofs, .. } => equivocation_proofs,
            ConsensusResult::Timeout { equivocation_proofs, .. } => equivocation_proofs,
        }
    }
}
```

**Benefits**:
- **Completeness**: Equivocation can occur regardless of consensus outcome
- **Unified API**: Single method to extract proofs from any variant
- **No data loss**: Proofs preserved even if consensus fails/times out

**Why all variants?**
- Equivocation detection runs independently of consensus success
- Timeout doesn't mean equivocation didn't happen
- Conflicts might coexist with equivocation (different issues)

### 5. Fact Registry Integration (`aura-agent`)

**Implementation**:
```rust
// aura-agent/src/fact_registry.rs
pub fn build_fact_registry() -> FactRegistry {
    let mut registry = FactRegistry::new();
    registry.register::<ConsensusFact>(
        CONSENSUS_FACT_TYPE_ID,
        Box::new(ConsensusFactReducer)
    );
    // ...
}

// aura-agent/src/fact_types.rs
pub static FACT_TYPE_IDS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    vec![
        CONSENSUS_FACT_TYPE_ID,  // "consensus"
        // ...
    ]
});
```

**Benefits**:
- Central registration prevents type ID collisions
- Validation ensures all registered types are in central list
- Clear ownership: consensus crate owns fact type, agent crate registers it

## Evidence Propagation Architecture

### Journal-Based Propagation (Current Design)

**Decision**: Use existing journal CRDT propagation instead of message-level evidence deltas.

**Original Plan** (rejected):
```rust
// Would add evidence_delta to every message
pub enum ConsensusMessage {
    Execute {
        consensus_id: ConsensusId,
        evidence_delta: EvidenceDelta,  // ❌ Rejected
        // ...
    },
}
```

**Actual Implementation** (accepted):
```rust
// Callers emit proofs to journals
let result = run_consensus(...).await?;
for proof in result.equivocation_proofs() {
    let fact = proof.to_generic();
    journal_effects.add_fact(context_id, fact).await?;
}
// Journal handles CRDT propagation via anti-entropy
```

**Rationale**:
- **Separation**: P2P propagation layer already exists (journal anti-entropy)
- **Retention**: Journals provide durable storage layer
- **CRDT**: Facts merge via set union (no conflict resolution needed)
- **No duplication**: Don't build parallel propagation system
- **Clean architecture**: Consensus produces evidence, journals distribute it

**User Feedback** (pivotal moment):
> "wait, I want you to step back and think about this... why we are adding another message type and not having evidence flow through the journal system. In our journal we already have a separation between the P2P layer which propagates data, and what data is retained by the journal it seems like we should be using that same principle"

This feedback led to the journal-based approach, which is architecturally cleaner and reuses existing infrastructure.

## Caller Integration Patterns

Three patterns documented in `tests/equivocation_caller_example.rs`:

### Pattern 1: Direct Tracker Integration
**Use case**: Custom consensus flows with full control

```rust
let mut tracker = WitnessTracker::new();
tracker.record_signature_with_detection(
    context_id, witness, signature, consensus_id,
    prestate_hash, result_id, timestamp_ms
);

for proof in tracker.get_equivocation_proofs() {
    journal_effects.add_fact(context_id, proof.to_generic()).await?;
}
tracker.clear_equivocation_proofs();
```

### Pattern 2: Result Extraction
**Use case**: Standard consensus protocol integration

```rust
let result = run_consensus(...).await?;
for proof in result.equivocation_proofs() {
    journal_effects.add_fact(context_id, proof.to_generic()).await?;
}
```

### Pattern 3: Standalone Detector
**Use case**: Custom validation workflows

```rust
let mut detector = EquivocationDetector::new();
if let Some(proof) = detector.check_share(...) {
    journal_effects.add_fact(context_id, proof.to_generic()).await?;
}
```

## Type Safety Features

### Linear Share Collection (`shares.rs`)

**Status**: Implemented but not yet integrated into protocol.

**Design**: Sealed/unsealed type pattern for compile-time threshold proofs.

```rust
pub struct ShareCollector {
    threshold: usize,
    shares_by_rid: BTreeMap<ResultId, LinearShareSet>,
}

pub struct LinearShareSet {  // Unsealed - accepts shares
    shares: BTreeMap<AuthorityId, PartialSignature>,
    sealed: bool,
}

pub struct ThresholdShareSet {  // Sealed - proven at type level
    shares: BTreeMap<AuthorityId, PartialSignature>,
}

// ONLY ThresholdShareSet can call combine()
impl ThresholdShareSet {
    pub fn combine(self) -> Result<ThresholdSignature> { ... }
}
```

**Benefits**:
- **Compile-time proof**: Cannot call `combine()` before threshold met
- **Linearity**: Sealed sets prevent further insertions
- **Type-level invariant**: Rust's type system enforces threshold constraint

**Integration Challenges**:
- **Protocol model mismatch**: ShareCollector tracks multiple result IDs, but current consensus protocol assumes single result ID
- **Requires refactoring**: Would need protocol changes to handle competing result IDs
- **Lower priority**: Equivocation detection works without this

**Future Work**:
- Could adapt ShareCollector to single-result-ID case
- Or refactor protocol to handle result ID conflicts explicitly
- Currently documented but not enforced in runtime

## Testing Strategy

### Unit Tests (17 tests)
- `core/validation.rs`: 7 tests for EquivocationDetector
- `facts.rs`: 3 tests for ConsensusFact serialization
- `aura-agent/fact_registry.rs`: 3 tests for registration
- `shares.rs`: 4 tests for linear share collection (not yet integrated)

### Integration Tests (10 tests)
- `tests/equivocation_detection.rs`: 6 tests for end-to-end detection flow
  - Basic equivocation detection
  - Duplicate vs conflicting signatures
  - Multiple witnesses
  - Multiple consensus instances
  - Proof serialization
  - Proof cleanup

- `tests/equivocation_caller_example.rs`: 4 tests for caller patterns
  - Direct tracker integration
  - Multi-round accumulation
  - Standalone detector usage
  - ConsensusResult integration

### Coverage
- **Detection logic**: ✅ Comprehensive
- **Proof generation**: ✅ Verified
- **Serialization**: ✅ Round-trip tested
- **Journal emission**: ⚠️ Documented patterns, not runtime-tested (requires Layer 6)
- **P2P propagation**: ⚠️ Future work (requires full anti-entropy testing)

## Design Principles

1. **Separation of Concerns**
   - Consensus (Layer 4) generates evidence
   - Callers (Layer 5/6) emit to journals
   - Journals (Layer 2) handle propagation

2. **Type Safety**
   - Domain fact pattern for extensibility
   - Sealed types for threshold proofs (implemented, not integrated)
   - Explicit context routing

3. **Backward Compatibility**
   - Opt-in detection via new method
   - Existing APIs unchanged
   - Non-breaking additions to ConsensusResult

4. **Evidence Completeness**
   - Both conflicting result IDs captured
   - Timestamp for temporal ordering
   - Context binding for journal routing
   - No information loss

5. **Architectural Alignment**
   - Reuse journal propagation (no parallel system)
   - Follow domain fact pattern (no special journal handling)
   - Respect layer boundaries (consensus doesn't know about journals)

## References

- **Fact system**: `docs/102_journal.md` (domain fact contract)
- **Consensus spec**: `docs/104_consensus.md`
- **Relational contexts**: `docs/103_relational_contexts.md`
- **Integration guide**: `EQUIVOCATION_INTEGRATION.md`
- **Example code**: `tests/equivocation_caller_example.rs`

## Future Enhancements

1. **ShareCollector Integration**
   - Adapt to single-result-ID model
   - Or refactor protocol for multi-result-ID handling
   - Provide compile-time threshold guarantees

2. **Runtime Testing**
   - End-to-end journal emission
   - Anti-entropy propagation verification
   - Multi-hop evidence distribution

3. **Monitoring**
   - Metrics for equivocation detection rate
   - Alerting on equivocation events
   - Dashboard for consensus health

4. **Guard Chain Integration**
   - Enforce flow costs from choreography annotations
   - Track leakage budgets from protocol metadata
   - Automatic journal coupling for facts

## Changelog

- `0252672b` - Domain fact refactoring
- `7de32a70` - Integration guide
- `945e4b8f` - Runtime integration
- `d4c350c3` - Documentation update
- `e21d671f` - Caller examples and patterns
- TBD - Architecture documentation
