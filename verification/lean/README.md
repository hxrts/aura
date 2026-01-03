Aura Development Environment
============================

Rust version: rustc 1.90.0 (1159e78c4 2025-09-14)
Cargo version: cargo 1.90.0 (840b83a10 2025-07-30)
Quint version: 0.25.1
Apalache version: 0.45.4
TLA+ tools: available
Node.js version: v20.19.5
Lean version: Lean (version 4.23.0, arm64-apple-darwin, commit v4.23.0, Release)
Aeneas version: available

Available commands:
  just --list          Show all available tasks
  just build           Build all crates
  just test            Run all tests
  just check           Check workspace (cargo check)
  just quint-parse     Parse Quint files to JSON
  just serve-console   Serve console with hot reload (crates/console)
  quint --help         Formal verification with Quint
  apalache-mc --help   Model checking with Apalache
  lean --help          Kernel verification with Lean 4
  aeneas --help        Rust-to-Lean translation
  crate2nix --help     Generate hermetic Nix builds

Hermetic builds:
  nix build            Build with crate2nix (hermetic)
  nix build .#aura-terminal Build specific package
  nix run              Run aura CLI hermetically
  nix flake check      Run hermetic tests

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
├── Types/                    # Shared type helpers (ByteArray32, OrderTime, etc.)
│   ├── AttestedOp.lean
│   ├── ByteArray32.lean
│   ├── FactContent.lean
│   ├── Identifiers.lean
│   ├── Namespace.lean
│   ├── OrderTime.lean
│   ├── ProtocolFacts.lean
│   ├── TimeStamp.lean
│   └── TreeOp.lean
├── Consensus/
│   ├── Types.lean            # Core consensus data structures
│   ├── Agreement.lean        # Agreement safety proofs
│   ├── Validity.lean         # Validity proofs (threshold, prestate)
│   ├── Evidence.lean         # Evidence CRDT semilattice proofs
│   ├── Equivocation.lean     # Equivocation detection correctness
│   ├── Frost.lean            # FROST threshold signature integration
│   ├── Liveness.lean         # Liveness claims (synchrony model)
│   ├── Adversary.lean        # Byzantine model and tolerance
│   ├── Proofs.lean           # Claims bundle aggregation
│   ├── RustCorrespondence.lean # Mapping notes for Rust core
│   └── TestVectors.lean      # Test fixtures and examples
├── Journal.lean              # CRDT semilattice proofs
├── KeyDerivation.lean        # Contextual key derivation isolation
├── GuardChain.lean           # Guard chain cost calculation
├── FlowBudget.lean           # Budget charging monotonicity
├── Frost.lean                # FROST state machine correctness
├── TimeSystem.lean           # Timestamp ordering and privacy
└── Runner.lean               # CLI for differential testing (aura_verifier)
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
| `Consensus.Types` | `crates/aura-consensus/src/core/state.rs` | State structures |
| `Consensus.Agreement` | `crates/aura-consensus/src/core/validation.rs` | Invariant checks |
| `Consensus.Evidence` | `crates/aura-consensus/src/core/transitions.rs` | State transitions |

See `Aura/Consensus/RustCorrespondence.lean` for additional mapping notes.

### ITF Trace Conformance

The Rust pure core can be tested against Quint ITF traces:

```bash
# Generate traces
./scripts/generate-itf-traces.sh 50

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
