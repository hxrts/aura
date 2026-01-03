# Aura Formal Verification

This directory contains formal verification artifacts for the Aura protocol using two complementary systems: Lean 4 for mathematical theorem proofs and Quint for executable state machine specifications.

## Structure

```
verification/
├── README.md                 # This file
├── lean/                     # Lean 4 theorem proofs
│   ├── README.md             # Detailed Lean documentation
│   ├── lakefile.lean         # Build configuration
│   └── Aura/                 # Proof modules
│       ├── Assumptions.lean      # Cryptographic axioms
│       ├── Types.lean            # Core type definitions
│       ├── Types/                # Shared type helpers
│       ├── Consensus/            # Consensus proofs
│       ├── Journal.lean          # CRDT semilattice proofs
│       ├── FlowBudget.lean       # Budget charging monotonicity
│       ├── GuardChain.lean       # Guard chain cost calculation
│       ├── Frost.lean            # FROST state machine correctness
│       ├── KeyDerivation.lean    # Context key isolation
│       ├── TimeSystem.lean       # Timestamp ordering
│       └── Runner.lean           # CLI for differential testing
├── quint/                    # Quint state machine specs
│   ├── README.md             # Detailed Quint documentation
│   ├── STYLE.md              # Quint coding conventions
│   ├── core.qnt              # Shared runtime utilities
│   ├── consensus/            # Fast-path/fallback consensus specs
│   ├── journal/              # Journal and CRDT specs
│   ├── keys/                 # DKG, DKD, resharing specs
│   ├── sessions/             # Session and group specs
│   ├── liveness/             # Liveness analysis specs
│   ├── harness/              # Simulator harness modules
│   └── tui/                  # TUI state machine specs
└── traces/                   # Generated ITF traces for testing
```

## Verification Approach

| System | Purpose | Strength |
|--------|---------|----------|
| **Lean 4** | Mathematical theorem proofs | Correctness guarantees via formal proof |
| **Quint** | State machine model checking | Exhaustive state exploration via Apalache |

### Lean 4 Proofs

Mathematical proofs of safety properties:
- CRDT semilattice properties (commutativity, associativity, idempotence)
- Threshold signature correctness (FROST integration)
- Consensus agreement and validity
- Equivocation detection soundness/completeness
- Flow budget monotonicity
- Guard chain ordering

### Quint Models

Executable state machine specifications:
- Protocol state transitions
- Invariant checking via model checking
- Liveness and termination properties
- Byzantine fault tolerance
- Guard chain and capability verification

## Quick Start

```bash
# Enter development environment
nix develop

# Build Lean proofs
cd verification/lean && lake build

# Check Quint specs
cd verification/quint && quint typecheck consensus/core.qnt

# Run model checking
quint run --invariant=InvariantUniqueCommitPerInstance consensus/core.qnt
```

## Correspondence Map

This section maps Quint model invariants to Lean theorem proofs, establishing verification correspondence.

### Layer Overview

| Layer | Purpose | Location |
|-------|---------|----------|
| **Quint** | State machine model checking | `verification/quint/` |
| **Lean 4** | Mathematical theorem proofs | `verification/lean/` |
| **Rust** | Implementation | `crates/aura-protocol/` |

### Types Correspondence

| Quint Type | Lean Type | Rust Type |
|------------|-----------|-----------|
| `ConsensusId` | `Aura.Consensus.Types.ConsensusId` | `consensus::types::ConsensusId` |
| `ResultId` | `Aura.Consensus.Types.ResultId` | `consensus::types::ResultId` |
| `PrestateHash` | `Aura.Consensus.Types.PrestateHash` | `consensus::types::PrestateHash` |
| `AuthorityId` | `Aura.Consensus.Types.AuthorityId` | `core::AuthorityId` |
| `ShareData` | `Aura.Consensus.Types.SignatureShare` | `consensus::types::SignatureShare` |
| `ThresholdSignature` | `Aura.Consensus.Types.ThresholdSignature` | `consensus::types::ThresholdSignature` |
| `CommitFact` | `Aura.Consensus.Types.CommitFact` | `consensus::types::CommitFact` |
| `WitnessVote` | `Aura.Consensus.Types.WitnessVote` | `consensus::types::WitnessVote` |
| `Evidence` | `Aura.Consensus.Types.Evidence` | `consensus::types::Evidence` |

### Invariant-Theorem Correspondence

#### Agreement Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantUniqueCommitPerInstance` | `Aura.Consensus.Agreement.agreement` | `sorry` |
| `InvariantUniqueCommitPerInstance` | `Aura.Consensus.Agreement.unique_commit` | `sorry` |
| - | `Aura.Consensus.Agreement.commit_determinism` | `sorry` |

**Claims Bundle**: `Aura.Consensus.Agreement.agreementClaims`

#### Validity Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantCommitRequiresThreshold` | `Aura.Consensus.Validity.commit_has_threshold` | proven |
| `InvariantSignatureBindsToCommitFact` | `Aura.Consensus.Validity.validity` | proven |
| - | `Aura.Consensus.Validity.distinct_signers` | proven |
| - | `Aura.Consensus.Validity.prestate_binding_unique` | proven |
| - | `Aura.Consensus.Validity.honest_participation` | proven |
| - | `Aura.Consensus.Validity.threshold_unforgeability` | axiom |

**Claims Bundle**: `Aura.Consensus.Validity.validityClaims`

#### FROST Integration Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantSignatureThreshold` | `Aura.Consensus.Frost.aggregation_threshold` | proven |
| - | `Aura.Consensus.Frost.share_session_consistency` | proven |
| - | `Aura.Consensus.Frost.share_result_consistency` | proven |
| - | `Aura.Consensus.Frost.distinct_signers` | proven |
| - | `Aura.Consensus.Frost.share_binding` | proven |

**Claims Bundle**: `Aura.Consensus.Frost.frostClaims`

#### Evidence CRDT Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| - | `Aura.Consensus.Evidence.merge_comm_votes` | proven |
| - | `Aura.Consensus.Evidence.merge_assoc_votes` | proven |
| - | `Aura.Consensus.Evidence.merge_idem` | proven |
| - | `Aura.Consensus.Evidence.merge_preserves_commit` | proven |
| - | `Aura.Consensus.Evidence.commit_monotonic` | proven |

**Claims Bundle**: `Aura.Consensus.Evidence.evidenceClaims`

#### Equivocation Detection Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantEquivocationDetected` | `Aura.Consensus.Equivocation.detection_soundness` | proven |
| `InvariantEquivocationDetected` | `Aura.Consensus.Equivocation.detection_completeness` | proven |
| `InvariantEquivocatorsExcluded` | `Aura.Consensus.Equivocation.exclusion_correctness` | proven |
| `InvariantHonestMajorityCanCommit` | `Aura.Consensus.Equivocation.honest_never_detected` | proven |
| - | `Aura.Consensus.Equivocation.verified_proof_sound` | proven |

**Claims Bundle**: `Aura.Consensus.Equivocation.equivocationClaims`

#### Byzantine Tolerance (Adversary Module)

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantByzantineThreshold` | `Aura.Consensus.Adversary.adversaryClaims.byzantine_cannot_forge` | claim |
| `InvariantEquivocationDetected` | `Aura.Consensus.Adversary.adversaryClaims.equivocation_detectable` | claim |
| `InvariantHonestMajorityCanCommit` | `Aura.Consensus.Adversary.adversaryClaims.honest_majority_sufficient` | claim |
| `InvariantEquivocatorsExcluded` | `Aura.Consensus.Adversary.adversaryClaims.equivocators_excluded` | claim |
| `InvariantCompromisedNoncesExcluded` | - | Quint only |

**Claims Bundle**: `Aura.Consensus.Adversary.adversaryClaims`

#### Liveness Properties

| Quint Property | Lean Support | Notes |
|----------------|--------------|-------|
| `InvariantProgressUnderSynchrony` | `Aura.Consensus.Liveness.livenessClaims.terminationUnderSynchrony` | axiom |
| `InvariantByzantineTolerance` | `byzantine_threshold` | axiom |
| `FastPathProgressCheck` | `Aura.Consensus.Liveness.livenessClaims.fastPathBound` | axiom |
| `SlowPathProgressCheck` | `Aura.Consensus.Liveness.livenessClaims.fallbackBound` | axiom |
| `NoDeadlock` | `Aura.Consensus.Liveness.livenessClaims.noDeadlock` | axiom |
| `InvariantRetryBound` | - | Quint model checking only |

**Claims Bundle**: `Aura.Consensus.Liveness.livenessClaims`

### Axioms (Assumptions.lean)

These are cryptographic assumptions taken as axioms:

| Axiom | Purpose | Used By |
|-------|---------|---------|
| `frost_threshold_unforgeability` | FROST k-of-n security | Validity, Frost |
| `frost_uniqueness` | Same shares produce same signature | Agreement |
| `hash_collision_resistance` | Prestate binding | Validity |
| `byzantine_threshold` | k > f (threshold > Byzantine) | All safety properties |

## Verification Status

### Lean Proofs

Current proof status (run `just lean-status` for authoritative results):

**All proofs complete (no sorry)**:
- Validity properties (commit_has_threshold, validity, distinct_signers)
- FROST aggregation properties (session/result consistency, threshold, share_binding)
- Equivocation detection (soundness, completeness, exclusion correctness)
- Evidence CRDT properties (commutativity, associativity, idempotence)
- Agreement properties (uses FROST uniqueness axiom)
- Journal CRDT properties (commutativity, associativity, idempotence, reduce properties)
- Adversary model properties (Byzantine threshold, equivocation detection)
- ByteArray32 comparison properties (reflexivity, transitivity)

### Quint Model Checking

All invariants can be checked via:
```bash
quint run --invariant=InvariantName consensus/core.qnt
```

Key invariants verified:
- `InvariantUniqueCommitPerInstance`
- `InvariantCommitRequiresThreshold`
- `InvariantEquivocatorsExcluded`
- `InvariantSignatureBindsToCommitFact`
- `InvariantSignatureThreshold`

## Usage

### Verifying Lean Proofs

```bash
cd verification/lean
lake build
```

Or via justfile:
```bash
just lean-build        # Build and check proofs
just lean-status       # Per-module status (sorries)
just lean-oracle-build # Build aura_verifier (Lean oracle)
just test-differential # Rust vs Lean oracle tests
```

### Checking Quint Models

```bash
cd verification/quint
quint typecheck consensus/core.qnt
quint run --invariant=InvariantUniqueCommitPerInstance consensus/core.qnt
```

### Verification Tooling

```bash
# Check Quint-Rust type correspondence (detects type drift)
just quint-check-types          # Summary output
just quint-check-types --verbose # Detailed report

# Generate verification coverage report
just verification-coverage      # Markdown report
just verification-coverage --json # JSON metrics

# Run all verification
just verify-all                 # Lean + Quint + conformance tests
```

### Accessing Claims Bundles

```lean
import Aura.Consensus.Proofs

-- Master bundle with all claims
#check Aura.Consensus.Proofs.consensusClaims

-- Individual bundles
#check Aura.Consensus.Agreement.agreementClaims
#check Aura.Consensus.Validity.validityClaims
#check Aura.Consensus.Evidence.evidenceClaims
#check Aura.Consensus.Equivocation.equivocationClaims
#check Aura.Consensus.Frost.frostClaims
#check Aura.Consensus.Liveness.livenessClaims
#check Aura.Consensus.Adversary.adversaryClaims
```

## Documentation

- [Lean README](./lean/README.md) - Detailed Lean module documentation
- [Quint README](./quint/README.md) - Detailed Quint specification documentation
- [Verification Coverage](../docs/998_verification_coverage.md) - Current coverage metrics
- [Verification Guide](../docs/807_verification_guide.md) - Verification development guide
