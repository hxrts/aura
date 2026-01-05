# Aura Lean Verification

Formal verification modules for Aura's kernel components using Lean 4.

## Setup

The Lean toolchain is managed through Nix (from repo root):

```bash
nix develop
```

## Building

```bash
just lean-build
# or directly:
cd verification/lean && lake build
```

Check current proof status (sorries/TODOs):

```bash
just lean-status
# or:
rg -n "\bsorry\b" verification/lean
```

## Module Structure

```
Aura/
├── Assumptions.lean          # Cryptographic axioms (FROST, hash, PRF)
├── Types.lean                # Shared core type definitions
├── Types/                    # Shared type helpers
│   ├── AttestedOp.lean
│   ├── ByteArray32.lean
│   ├── FactContent.lean
│   ├── Identifiers.lean
│   ├── Namespace.lean
│   ├── OrderTime.lean
│   ├── ProtocolFacts.lean
│   ├── TimeStamp.lean
│   └── TreeOp.lean
├── Domain/                   # Domain types and operations (no proofs)
│   ├── Consensus/
│   │   ├── Types.lean        # Consensus data structures
│   │   └── Frost.lean        # FROST types and operations
│   ├── Journal/
│   │   ├── Types.lean        # Fact, Journal structures
│   │   └── Operations.lean   # merge, reduce, factsEquiv
│   ├── FlowBudget.lean       # Budget types and charge operation
│   ├── GuardChain.lean       # Guard types and evaluation
│   ├── TimeSystem.lean       # Timestamp types and comparison
│   └── KeyDerivation.lean    # Key derivation types
├── Proofs/                   # All proofs centralized
│   ├── Consensus/            # Consensus proofs
│   │   ├── Agreement.lean    # Agreement safety proofs
│   │   ├── Validity.lean     # Validity proofs
│   │   ├── Evidence.lean     # Evidence CRDT proofs
│   │   ├── Equivocation.lean # Equivocation detection
│   │   ├── Frost.lean        # FROST integration proofs
│   │   ├── Liveness.lean     # Liveness claims (axiomatized)
│   │   ├── Adversary.lean    # Byzantine model
│   │   └── Summary.lean      # Claims bundle aggregation
│   ├── Journal.lean          # CRDT semilattice proofs
│   ├── FlowBudget.lean       # Budget charging proofs
│   ├── GuardChain.lean       # Guard evaluation proofs
│   ├── TimeSystem.lean       # Timestamp ordering proofs
│   └── KeyDerivation.lean    # PRF isolation proofs
├── Proofs.lean               # Top-level entry point for reviewers
└── Runner.lean               # CLI for differential testing
```

### Import Discipline

Domain modules don't import from Proofs:
- `Domain/* ←── Proofs/*`
- Types flow upward, proofs import types

## Quint Correspondence

Lean proofs correspond to Quint specifications for verification coverage:

| Lean Module | Quint File | What It Proves |
|-------------|------------|----------------|
| `Proofs.Consensus.Agreement` | `consensus/core.qnt` | Agreement safety (unique commits) |
| `Proofs.Consensus.Evidence` | `consensus/core.qnt` | CRDT semilattice properties |
| `Proofs.Consensus.Frost` | `consensus/frost.qnt` | Threshold signature correctness |
| `Proofs.Consensus.Liveness` | `consensus/liveness.qnt` | Synchrony model axioms |
| `Proofs.Consensus.Adversary` | `consensus/adversary.qnt` | Byzantine tolerance bounds |
| `Proofs.Consensus.Equivocation` | `consensus/adversary.qnt` | Detection soundness/completeness |

See `verification/README.md` for the complete correspondence mapping.

## Proof Status

All proofs are complete with no `sorry` placeholders. Cryptographic assumptions are documented as axioms in `Aura/Assumptions.lean`.

Run `just lean-status` for the authoritative, per-module status.

## Key Properties (where proved or axiomatically assumed)

### Consensus Agreement
- Agreement: Valid commits for the same consensus instance have the same result
- Unique Commit: At most one valid CommitFact per ConsensusId
- Commit Determinism: Same threshold shares produce the same commit

### Consensus Evidence (CRDT)
- Commutativity: `merge e1 e2 ≃ merge e2 e1` (membership-wise)
- Associativity: `merge (merge e1 e2) e3 ≃ merge e1 (merge e2 e3)`
- Idempotence: `merge e e ≃ e`
- Monotonicity: Votes and equivocators only grow under merge

### Equivocation Detection
- Soundness: Detection only reports actual equivocation
- Completeness: All equivocations are detectable
- Honest Safety: Honest witnesses are never falsely accused

### FROST Integration
- Session Consistency: All shares in aggregation have same session
- Threshold Requirement: Aggregation requires at least k shares
- Share Binding: Shares are cryptographically bound to consensus data

### Journal CRDT
- Commutativity: `merge j1 j2 ≃ merge j2 j1`
- Associativity: `merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3)`
- Idempotence: `merge j j ≃ j`

### Flow Budget
- Monotonic Decrease: Charging never increases available budget
- Exact Charge: Charging exact amount results in zero budget

### TimeSystem
- Reflexivity: `compare policy t t = .eq`
- Transitivity: `compare policy a b = .lt -> compare policy b c = .lt -> compare policy a c = .lt`
- Privacy: Physical time hidden when `ignorePhysical = true`

## Claims Bundles

Each module exports a Claims bundle for reviewers. Start with `Aura.Proofs`:

```lean
import Aura.Proofs

-- Infrastructure claims
#check Aura.Proofs.journalClaims
#check Aura.Proofs.flowBudgetClaims
#check Aura.Proofs.guardChainClaims
#check Aura.Proofs.timeSystemClaims
#check Aura.Proofs.keyDerivationClaims

-- Consensus claims
#check Aura.Proofs.agreementClaims
#check Aura.Proofs.validityClaims
#check Aura.Proofs.evidenceClaims
#check Aura.Proofs.equivocationClaims
#check Aura.Proofs.frostClaims
#check Aura.Proofs.livenessClaims
#check Aura.Proofs.adversaryClaims
#check Aura.Proofs.consensusClaims  -- Main bundle
```

Axioms are documented in `Aura.Assumptions`.

## Justfile Commands

```bash
just lean-build        # Build and check proofs
just lean-check        # Build (same as lean-build)
just lean-clean        # Clean build artifacts
just lean-full         # Clean + build + check
just lean-status       # Per-module status (sorries)
just lean-oracle-build # Build aura_verifier (Lean oracle)
just test-differential # Rust vs Lean oracle tests
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
| `Domain.Consensus.Types` | `crates/aura-consensus/src/core/state.rs` | State structures |
| `Proofs.Consensus.Agreement` | `crates/aura-consensus/src/core/validation.rs` | Invariant checks |
| `Proofs.Consensus.Evidence` | `crates/aura-consensus/src/core/transitions.rs` | State transitions |

See the doc comments in individual proof modules for Rust correspondence notes.

### ITF Trace Conformance

The Rust pure core can be tested against Quint ITF traces:

```bash
# Generate traces with quint
quint run verification/quint/consensus/core.qnt \
  --out-itf traces/consensus/trace.itf.json --max-steps 20

# Run conformance tests
cargo test -p aura-testkit --test consensus_itf_conformance
```

ITF loader: `crates/aura-testkit/src/consensus/itf_loader.rs`
Conformance tests: `crates/aura-testkit/tests/consensus_itf_conformance.rs`

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

CI jobs in `.github/workflows/ci.yml` include:

1. `lean-proofs`: builds Lean modules and reports `sorry` usage (warning-only today)
2. `differential-testing`: runs the Lean oracle checks

Refer to the workflow file for exact triggers.

## Verification Coverage

See the full verification coverage report:

```bash
just verification-coverage      # Markdown report
just verification-coverage --json # JSON metrics
```

Current metrics include theorem counts per module and correspondence with Quint invariants.

See `docs/998_verification_coverage.md` for the generated report.
