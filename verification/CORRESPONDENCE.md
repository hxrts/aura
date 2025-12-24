# Verification Correspondence Map

This document maps Quint model invariants to Lean theorem proofs, establishing a verification correspondence for Aura consensus properties.

## Overview

| Layer | Purpose | Location |
|-------|---------|----------|
| **Quint** | State machine model checking | `verification/quint/` |
| **Lean 4** | Mathematical theorem proofs | `verification/lean/` |
| **Rust** | Implementation | `crates/aura-protocol/` |

## Consensus Verification Correspondence

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
| `InvariantCommitRequiresThreshold` | `Aura.Consensus.Validity.commit_has_threshold` | ✓ proven |
| `InvariantSignatureBindsToCommitFact` | `Aura.Consensus.Validity.validity` | ✓ proven |
| - | `Aura.Consensus.Validity.distinct_signers` | ✓ proven |
| - | `Aura.Consensus.Validity.prestate_binding_unique` | ✓ proven |
| - | `Aura.Consensus.Validity.honest_participation` | ✓ proven |
| - | `Aura.Consensus.Validity.threshold_unforgeability` | ✓ axiom |

**Claims Bundle**: `Aura.Consensus.Validity.validityClaims`

#### FROST Integration Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantSignatureThreshold` | `Aura.Consensus.Frost.aggregation_threshold` | ✓ proven |
| - | `Aura.Consensus.Frost.share_session_consistency` | ✓ proven |
| - | `Aura.Consensus.Frost.share_result_consistency` | ✓ proven |
| - | `Aura.Consensus.Frost.distinct_signers` | ✓ proven |
| - | `Aura.Consensus.Frost.share_binding` | ✓ proven |

**Claims Bundle**: `Aura.Consensus.Frost.frostClaims`

#### Evidence CRDT Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| - | `Aura.Consensus.Evidence.merge_comm_votes` | ✓ proven |
| - | `Aura.Consensus.Evidence.merge_assoc_votes` | ✓ proven |
| - | `Aura.Consensus.Evidence.merge_idem` | ✓ proven |
| - | `Aura.Consensus.Evidence.merge_preserves_commit` | ✓ proven |
| - | `Aura.Consensus.Evidence.commit_monotonic` | ✓ proven |

**Claims Bundle**: `Aura.Consensus.Evidence.evidenceClaims`

#### Equivocation Detection Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantEquivocationDetected` | `Aura.Consensus.Equivocation.detection_soundness` | ✓ proven |
| `InvariantEquivocationDetected` | `Aura.Consensus.Equivocation.detection_completeness` | ✓ proven |
| `InvariantEquivocatorsExcluded` | `Aura.Consensus.Equivocation.exclusion_correctness` | ✓ proven |
| `InvariantHonestMajorityCanCommit` | `Aura.Consensus.Equivocation.honest_never_detected` | ✓ proven |
| - | `Aura.Consensus.Equivocation.verified_proof_sound` | ✓ proven |

**Claims Bundle**: `Aura.Consensus.Equivocation.equivocationClaims`

#### Byzantine Tolerance (Adversary Module)

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantByzantineThreshold` | `Aura.Core.Assumptions.byzantine_threshold` | ✓ axiom |
| `InvariantCompromisedNoncesExcluded` | - | Quint only |

#### Liveness Properties

| Quint Property | Lean Support | Notes |
|----------------|--------------|-------|
| `InvariantProgressUnderSynchrony` | `honest_participation` | Safety support |
| `InvariantByzantineTolerance` | `byzantine_threshold` | Axiom |
| `InvariantRetryBound` | - | Quint model checking only |
| `FastPathProgressCheck` | - | Quint model checking only |
| `SlowPathProgressCheck` | - | Quint model checking only |
| `NoDeadlock` | - | Quint model checking only |

### Axioms (Assumptions.lean)

These are cryptographic assumptions taken as axioms:

| Axiom | Purpose | Used By |
|-------|---------|---------|
| `frost_threshold_unforgeability` | FROST k-of-n security | Validity, Frost |
| `frost_uniqueness` | Same shares → same signature | Agreement |
| `hash_collision_resistance` | Prestate binding | Validity |
| `byzantine_threshold` | k > f (threshold > Byzantine) | All safety properties |

## File Structure

```
verification/
├── CORRESPONDENCE.md          # This document
├── lean/
│   ├── STYLE.md               # Lean coding conventions
│   ├── lakefile.lean          # Build configuration
│   └── Aura/
│       ├── Core/
│       │   └── Assumptions.lean    # Cryptographic axioms
│       └── Consensus/
│           ├── Types.lean          # Domain types
│           ├── Agreement.lean      # Agreement proofs
│           ├── Validity.lean       # Validity proofs
│           ├── Evidence.lean       # Evidence CRDT proofs
│           ├── Equivocation.lean   # Equivocation proofs
│           ├── Frost.lean          # FROST integration proofs
│           ├── Liveness.lean       # Liveness claims (axioms for temporal)
│           ├── Adversary.lean      # Byzantine adversary model
│           └── Proofs.lean         # Claims bundle summary
└── quint/
    ├── STYLE.md                    # Quint coding conventions
    ├── README.md                   # Quint usage guide
    ├── protocol_consensus.qnt      # Core consensus model
    ├── protocol_consensus_adversary.qnt  # Byzantine models
    └── protocol_consensus_liveness.qnt   # Liveness properties
```

## Verification Status Summary

### Lean Proofs

**All proofs completed (no sorry)** as of 2024-12-24:
- Validity reflexive properties (commit_has_threshold, validity, distinct_signers)
- FROST aggregation properties (session/result consistency, threshold, share_binding)
- Equivocation exclusion correctness, detection soundness/completeness
- Validity honest participation
- Agreement properties (connected to FROST uniqueness axiom)
- Evidence CRDT properties (using pure Lean list dedup lemmas)
- All claims bundles fully instantiated

### Quint Model Checking

All invariants can be checked via:
```bash
quint run --invariant=InvariantName protocol_consensus.qnt
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

### Checking Quint Models

```bash
cd verification/quint
quint typecheck protocol_consensus.qnt
quint run --invariant=InvariantUniqueCommitPerInstance protocol_consensus.qnt
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
```

## Liveness Properties (Liveness.lean)

| Quint Property | Lean Type/Theorem | Status |
|----------------|-------------------|--------|
| `isSynchronous` | `Aura.Consensus.Liveness.isSynchronous` | ✓ |
| `canMakeProgress` | `Aura.Consensus.Liveness.canMakeProgress` | ✓ |
| `SynchronyState` | `Aura.Consensus.Liveness.SynchronyState` | ✓ |
| `ProgressCondition` | `Aura.Consensus.Liveness.ProgressCondition` | ✓ |
| `FastPathProgressCheck` | `Aura.Consensus.Liveness.livenessClaims.fastPathBound` | axiom |
| `SlowPathProgressCheck` | `Aura.Consensus.Liveness.livenessClaims.fallbackBound` | axiom |
| `InvariantProgressUnderSynchrony` | `Aura.Consensus.Liveness.livenessClaims.terminationUnderSynchrony` | axiom |
| `NoDeadlock` | `Aura.Consensus.Liveness.livenessClaims.noDeadlock` | axiom |

**Claims Bundle**: `Aura.Consensus.Liveness.livenessClaims`

## Adversary Properties (Adversary.lean)

| Quint Property | Lean Type/Theorem | Status |
|----------------|-------------------|--------|
| `isByzantine` | `Aura.Consensus.Adversary.isByzantine` | ✓ |
| `canEquivocate` | `Aura.Consensus.Adversary.hasEquivocated` | ✓ |
| `byzantineThresholdOk` | `Aura.Consensus.Adversary.byzantineThresholdOk` | ✓ |
| `ByzantineWitness` | `Aura.Consensus.Adversary.ByzantineWitness` | ✓ |
| `AdversaryState` | `Aura.Consensus.Adversary.AdversaryState` | ✓ |
| `EquivocationProof` | `Aura.Consensus.Adversary.EquivocationProof` | ✓ |
| `InvariantEquivocationDetected` | `Aura.Consensus.Adversary.adversaryClaims.equivocation_detectable` | ✓ claim |
| `InvariantHonestMajorityCanCommit` | `Aura.Consensus.Adversary.adversaryClaims.honest_majority_sufficient` | ✓ claim |
| `InvariantByzantineThreshold` | `Aura.Consensus.Adversary.adversaryClaims.byzantine_cannot_forge` | ✓ claim |
| `InvariantEquivocatorsExcluded` | `Aura.Consensus.Adversary.adversaryClaims.equivocators_excluded` | ✓ claim |

**Claims Bundle**: `Aura.Consensus.Adversary.adversaryClaims`

## Validation Checklist

Use this checklist to validate correspondence completeness:

- [x] Every Lean type has documented Quint/Rust equivalent (Types correspondence table)
- [x] Every Quint EXPOSE predicate has documented Lean/Rust equivalent
  - `protocol_consensus.qnt`: ValidCommit, WellFormedState, ValidShare, sharesConsistent, canCommit
  - `protocol_consensus_adversary.qnt`: isByzantine, canEquivocate, byzantineThresholdOk
  - `protocol_consensus_liveness.qnt`: isSynchronous, canMakeProgress, isTerminal, hasQuorumOnline
- [x] Every Lean theorem has documented Quint invariant equivalent (Invariant-Theorem tables)
- [x] Every cross-reference in Lean points to existing Quint location
  - Types.lean → protocol_consensus.qnt TYPES
  - Agreement.lean → protocol_consensus.qnt INVARIANTS
  - Validity.lean → protocol_consensus.qnt INVARIANTS
  - Evidence.lean → protocol_consensus.qnt INVARIANTS
  - Equivocation.lean → protocol_consensus_adversary.qnt
  - Frost.lean → protocol_consensus.qnt (signature properties)
  - Liveness.lean → protocol_consensus_liveness.qnt
  - Adversary.lean → protocol_consensus_adversary.qnt
- [x] Every cross-reference in Quint points to existing Lean location
  - protocol_consensus.qnt → Aura.Consensus.*
  - protocol_consensus_adversary.qnt → Aura.Consensus.Equivocation, Aura.Consensus.Adversary
  - protocol_consensus_liveness.qnt → Aura.Consensus.Liveness

## Maintaining Correspondence

When modifying verification:

1. **Adding Quint invariants**: Add corresponding Lean theorem stub to appropriate Claims bundle
2. **Completing Lean proofs**: Update status in this correspondence map
3. **Adding types**: Update both Quint and Lean, add to Types correspondence table
4. **Changing axioms**: Update Assumptions.lean and document impact
5. **Updating EXPOSE predicates**: Ensure Lean/Rust equivalents are documented
