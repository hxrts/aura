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
| - | `Aura.Consensus.Frost.share_binding` | `sorry` |

**Claims Bundle**: `Aura.Consensus.Frost.frostClaims`

#### Evidence CRDT Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| - | `Aura.Consensus.Evidence.merge_comm_votes` | `sorry` |
| - | `Aura.Consensus.Evidence.merge_assoc_votes` | `sorry` |
| - | `Aura.Consensus.Evidence.merge_idem` | `sorry` |
| - | `Aura.Consensus.Evidence.merge_preserves_commit` | `sorry` |
| - | `Aura.Consensus.Evidence.commit_monotonic` | `sorry` |

**Claims Bundle**: `Aura.Consensus.Evidence.evidenceClaims`

#### Equivocation Detection Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantEquivocationDetected` | `Aura.Consensus.Equivocation.detection_soundness` | `sorry` |
| `InvariantEquivocationDetected` | `Aura.Consensus.Equivocation.detection_completeness` | `sorry` |
| `InvariantEquivocatorsExcluded` | `Aura.Consensus.Equivocation.exclusion_correctness` | ✓ proven |
| `InvariantHonestMajorityCanCommit` | `Aura.Consensus.Equivocation.honest_never_detected` | `sorry` |
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

**Completed (no sorry)**:
- Validity reflexive properties (commit_has_threshold, validity, distinct_signers)
- FROST aggregation properties (session/result consistency, threshold)
- Equivocation exclusion correctness
- Validity honest participation

**Placeholder (sorry)**:
- Agreement properties (require FROST uniqueness axiom connection)
- Evidence CRDT properties (require list dedup lemmas)
- Equivocation detection soundness/completeness
- FROST share binding

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

## Maintaining Correspondence

When modifying verification:

1. **Adding Quint invariants**: Add corresponding Lean theorem stub to appropriate Claims bundle
2. **Completing Lean proofs**: Update status in this correspondence map
3. **Adding types**: Update both Quint and Lean, add to Types correspondence table
4. **Changing axioms**: Update Assumptions.lean and document impact
