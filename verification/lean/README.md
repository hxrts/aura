# Aura Lean Verification

Formal verification modules for Aura's kernel components using Lean 4.

## Setup

The Lean toolchain is managed through Nix:

```bash
nix develop
```

## Building

```bash
just lean-build
# or directly:
cd verification/lean && lake build
```

## Module Structure

```
Aura/
├── Assumptions.lean          # Cryptographic axioms (FROST, hash, PRF)
├── Consensus/
│   ├── Types.lean            # Core consensus data structures
│   ├── Agreement.lean        # Agreement safety proofs
│   ├── Validity.lean         # Validity proofs (threshold, prestate)
│   ├── Evidence.lean         # Evidence CRDT semilattice proofs
│   ├── Equivocation.lean     # Equivocation detection correctness
│   ├── Frost.lean            # FROST threshold signature integration
│   ├── Liveness.lean         # Liveness claims (synchrony model)
│   ├── Adversary.lean        # Byzantine model and tolerance
│   └── Proofs.lean           # Claims bundle aggregation
├── Journal.lean              # CRDT semilattice proofs
├── KeyDerivation.lean        # Contextual key derivation isolation
├── GuardChain.lean           # Guard chain cost calculation
├── FlowBudget.lean           # Budget charging monotonicity
├── Frost.lean                # FROST state machine correctness
├── TimeSystem.lean           # Timestamp ordering & privacy
└── Runner.lean               # CLI for differential testing
```

## Quint Correspondence

Lean proofs correspond to Quint specifications for verification coverage:

| Lean Module | Quint File | What It Proves |
|-------------|------------|----------------|
| `Consensus.Agreement` | `protocol_consensus.qnt` | Agreement safety (unique commits) |
| `Consensus.Evidence` | `protocol_consensus.qnt` | CRDT semilattice properties |
| `Consensus.Frost` | `protocol_consensus.qnt` | Threshold signature correctness |
| `Consensus.Liveness` | `protocol_consensus_liveness.qnt` | Synchrony model axioms |
| `Consensus.Adversary` | `protocol_consensus_adversary.qnt` | Byzantine tolerance bounds |
| `Consensus.Equivocation` | `protocol_consensus_adversary.qnt` | Detection soundness/completeness |

See `verification/CORRESPONDENCE.md` for complete mapping.

## Proof Status

All proofs are complete (no `sorry` placeholders):

| Module | Status | Key Theorems |
|--------|--------|--------------|
| `Assumptions` | ● Complete | Cryptographic axioms for FROST, hash, PRF |
| `Consensus.Agreement` | ● Complete | `agreement`, `unique_commit`, `commit_determinism` |
| `Consensus.Validity` | ● Complete | `threshold_reflexivity`, `prestate_binding` |
| `Consensus.Evidence` | ● Complete | `merge_comm`, `merge_assoc`, `merge_idem` |
| `Consensus.Equivocation` | ● Complete | `detection_soundness`, `detection_completeness`, `honest_never_detected` |
| `Consensus.Frost` | ● Complete | `share_session_consistency`, `aggregatable_implies_valid_commit` |
| `Consensus.Liveness` | ● Complete | `termination_under_synchrony`, `fast_path_bound` (axiomatic) |
| `Consensus.Adversary` | ● Complete | `byzantine_tolerance`, `honest_majority_sufficient` |
| `Journal` | ● Complete | `merge_comm`, `merge_assoc`, `merge_idem` |
| `KeyDerivation` | ● Complete | `derive_unique` (axiomatic) |
| `GuardChain` | ● Complete | `cost_sum` |
| `FlowBudget` | ● Complete | `charge_decreases`, `charge_exact` |
| `Frost` | ● Complete | `aggregate_same_session_round` |
| `TimeSystem` | ● Complete | `compare_refl`, `compare_trans`, `physical_hidden` |

## Key Properties Proven

### Consensus Agreement
- **Agreement**: Valid commits for the same consensus instance have the same result
- **Unique Commit**: At most one valid CommitFact per ConsensusId
- **Commit Determinism**: Same threshold shares produce the same commit

### Consensus Evidence (CRDT)
- **Commutativity**: `merge e1 e2 ≃ merge e2 e1` (membership-wise)
- **Associativity**: `merge (merge e1 e2) e3 ≃ merge e1 (merge e2 e3)`
- **Idempotence**: `merge e e ≃ e`
- **Monotonicity**: Votes and equivocators only grow under merge

### Equivocation Detection
- **Soundness**: Detection only reports actual equivocation
- **Completeness**: All equivocations are detectable
- **Honest Safety**: Honest witnesses are never falsely accused

### FROST Integration
- **Session Consistency**: All shares in aggregation have same session
- **Threshold Requirement**: Aggregation requires ≥k shares
- **Share Binding**: Shares are cryptographically bound to consensus data

### Journal CRDT
- **Commutativity**: `merge j1 j2 ≃ merge j2 j1`
- **Associativity**: `merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3)`
- **Idempotence**: `merge j j ≃ j`

### Flow Budget
- **Monotonic Decrease**: Charging never increases available budget
- **Exact Charge**: Charging exact amount results in zero budget

### TimeSystem
- **Reflexivity**: `compare policy t t = .eq`
- **Transitivity**: `compare policy a b = .lt → compare policy b c = .lt → compare policy a c = .lt`
- **Privacy**: Physical time hidden when `ignorePhysical = true`

## Claims Bundles

Each module exports a Claims bundle for reviewers:

```lean
import Aura.Consensus.Proofs

#check Aura.Consensus.Agreement.agreementClaims
#check Aura.Consensus.Validity.validityClaims
#check Aura.Consensus.Evidence.evidenceClaims
#check Aura.Consensus.Equivocation.equivocationClaims
#check Aura.Consensus.Frost.frostClaims
```

Axioms are documented in `Aura.Assumptions`.

## Justfile Commands

```bash
just lean-build    # Build and check proofs
just lean-full     # Full workflow (clean + build + verify)
just lean-clean    # Clean build artifacts
```

## Implementation Notes

### Bool Conditional Reduction

Proofs involving `if (bne x y) then ... else ...` require specific simp lemmas:

```lean
have hne : (e1.consensusId != e2.consensusId) = false := by
  rw [bne_eq_false_iff_eq, h]
simp only [hne, Bool.false_eq_true, ite_false]
```

The pattern `Bool.false_eq_true, ite_false` reduces `if false = true then a else b` to `b`.

### Manual BEq Instances

FROST module uses manually defined `BEq` instances for proof reducibility:

```lean
instance : BEq SessionId where
  beq a b := a.id == b.id
```

### Membership-Based Set Equivalence

Journal and Evidence equivalence use membership rather than structural equality:

```lean
def Journal.equiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1 ↔ f ∈ j2
```

## Rust Pure Core Correspondence

The Lean proofs correspond to a pure, effect-free Rust implementation for direct testing:

| Lean Module | Rust Module | Correspondence |
|-------------|-------------|----------------|
| `Consensus.Types` | `aura-protocol/consensus/core/state.rs` | State structures |
| `Consensus.Agreement` | `aura-protocol/consensus/core/validation.rs` | Invariant checks |
| `Consensus.Evidence` | `aura-protocol/consensus/core/transitions.rs` | State transitions |

### ITF Trace Conformance

The Rust pure core can be tested against Quint ITF traces:

```bash
# Generate traces
cd verification/quint
quint run --out-itf=consensus_trace.itf.json protocol_consensus.qnt

# Run conformance tests
cargo test -p aura-protocol --test consensus_itf_conformance
```

ITF loader: `crates/aura-consensus/src/consensus/core/itf_loader.rs`
Conformance tests: `aura-protocol/tests/consensus_itf_conformance.rs`

## Differential Testing

The Lean oracle supports differential testing against Rust:

```bash
just lean-oracle-build
just test-differential
```

Oracle commands (JSON stdin/stdout):
- `aura_verifier journal-merge`
- `aura_verifier journal-reduce`
- `aura_verifier flow-charge`
- `aura_verifier timestamp-compare`

## CI Integration

Two CI jobs verify formal proofs:

1. **lean-proofs**: Builds all Lean modules
2. **differential-testing**: Runs Rust vs Lean oracle tests

Jobs trigger on changes to:
- `verification/lean/**`
- `crates/aura-journal/**`
- `crates/aura-core/src/time/**`
- `crates/aura-testkit/**`
