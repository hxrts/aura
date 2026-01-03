# Verification Coverage Report

Generated: 2026-01-03 13:06:29 UTC

## Summary Metrics

| Metric | Count |
|--------|-------|
| Quint Specifications | 37 |
| Quint Invariants | 169 |
| Quint Temporal Properties | 11 |
| Quint Type Definitions | 304 |
| Rust Invariant Checks | 54 |
| Lean Theorems | 118 |
| Verified Specs (CI) | 11 |
| ITF Traces | 1 |
| Differential Tests | 146 |

## Verification Layers

### Layer 1: Quint Specifications

Formal protocol specifications in `verification/quint/`:

| Status | Count |
|--------|-------|
| Model-checked (CI verified) | 4 |
| Has invariants (not in CI) | 25 |
| No invariants (helpers/harness) | 8 |

### Layer 2: Rust Integration

Files with Quint type correspondence:

```
aura-consensus/src/core/mod.rs
aura-consensus/src/core/state.rs
aura-consensus/src/core/transitions.rs
aura-consensus/src/core/validation.rs
aura-consensus/src/core/verification/mod.rs
aura-consensus/src/core/verification/quint_mapping.rs
aura-core/src/effects/mod.rs
aura-core/src/effects/quint.rs
aura-simulator/src/liveness/mod.rs
aura-simulator/src/quint/aura_state_extractors.rs
aura-simulator/src/quint/mod.rs
aura-simulator/src/quint/state_mapper.rs
aura-testkit/src/consensus/reference.rs
```

### Layer 3: Lean Proofs

Lean 4 verification modules in `verification/lean/`:

| Module | Theorems |
|--------|----------|
| `Aura/Types/ByteArray32.lean` | 6 |
| `Aura/Types/OrderTime.lean` | 4 |
| `Aura/Proofs/Consensus/Validity.lean` | 7 |
| `Aura/Proofs/Consensus/Equivocation.lean` | 5 |
| `Aura/Proofs/Consensus/Liveness.lean` | 3 |
| `Aura/Proofs/Consensus/Evidence.lean` | 8 |
| `Aura/Proofs/Consensus/Adversary.lean` | 7 |
| `Aura/Proofs/Consensus/Agreement.lean` | 3 |
| `Aura/Proofs/Consensus/Frost.lean` | 12 |
| `Aura/Proofs/Journal.lean` | 14 |
| `Aura/Proofs/ContextIsolation.lean` | 16 |
| `Aura/Proofs/FlowBudget.lean` | 5 |
| `Aura/Proofs/GuardChain.lean` | 7 |
| `Aura/Proofs/KeyDerivation.lean` | 3 |
| `Aura/Proofs/TimeSystem.lean` | 8 |
| `Aura/Assumptions.lean` | 1 |
| `Aura/Domain/Journal/Operations.lean` | 1 |
| `Aura/Domain/Consensus/Types.lean` | 8 |

## Verified Invariants

Quint invariants with Apalache verification in CI:

| Invariant | Specification |
|-----------|---------------|
| `InvariantNonceUnique` | journal/core.qnt:417 |
| `InvariantFactsOrdered` | journal/core.qnt:426 |
| `InvariantFactsMatchNamespace` | journal/core.qnt:433 |
| `InvariantLifecycleCompletedImpliesStable` | journal/core.qnt:440 |
| `InvariantNonceMergeCommutative` | journal/core.qnt:450 |
| `InvariantLamportMonotonic` | journal/core.qnt:462 |
| `InvariantReduceDeterministic` | journal/core.qnt:469 |
| `InvariantPhaseRegistered` | journal/counter.qnt:396 |
| `InvariantCountersRegistered` | journal/counter.qnt:400 |
| `InvariantLifecycleStatusDefined` | journal/counter.qnt:404 |
| `InvariantOutcomeWhenCompleted` | journal/counter.qnt:409 |
| `InvariantFactsMonotonic` | journal/anti_entropy.qnt:260 |
| `InvariantFactsSubsetOfGlobal` | journal/anti_entropy.qnt:267 |
| `InvariantVectorClockConsistent` | journal/anti_entropy.qnt:274 |
| `InvariantEventualConvergence` | journal/anti_entropy.qnt:284 |
| `InvariantDeltasFromSource` | journal/anti_entropy.qnt:294 |
| `InvariantCompletedSessionsConverged` | journal/anti_entropy.qnt:301 |
| `InvariantProgressUnderSynchrony` | consensus/liveness.qnt:518 |
| `InvariantRetryBound` | consensus/liveness.qnt:524 |
| `InvariantCommitRequiresHonestParticipation` | consensus/liveness.qnt:544 |
| `InvariantQuorumPossible` | consensus/liveness.qnt:546 |
| `InvariantByzantineThreshold` | consensus/adversary.qnt:369 |
| `InvariantEquivocationDetected` | consensus/adversary.qnt:377 |
| `InvariantCompromisedNoncesExcluded` | consensus/adversary.qnt:388 |
| `InvariantHonestMajorityCanCommit` | consensus/adversary.qnt:398 |
| `InvariantUniqueCommitPerInstance` | consensus/core.qnt:742 |
| `InvariantCommitRequiresThreshold` | consensus/core.qnt:752 |
| `InvariantCommittedHasCommitFact` | consensus/core.qnt:764 |
| `InvariantEquivocatorsExcluded` | consensus/core.qnt:775 |
| `InvariantProposalsFromWitnesses` | consensus/core.qnt:786 |

_... and 139 more invariants_

## Coverage Recommendations

### High Priority

1. **Add CI verification for specs with invariants but no model checking:**
   - `journal/core.qnt` (7 invariants)
   - `journal/counter.qnt` (4 invariants)
   - `journal/anti_entropy.qnt` (6 invariants)
   - `consensus/liveness.qnt` (4 invariants)
   - `consensus/adversary.qnt` (4 invariants)

2. **Add QuintMappable implementations for core types used in ITF conformance:**
   - ConsensusId, ResultId, ThresholdSignature

3. **Expand Lean theorem coverage:**
   - Add proofs for liveness properties
   - Add proofs for cross-protocol safety

## Related Commands

```bash
just quint-verify-models        # Run Apalache model checking
just quint-check-types          # Check Quint-Rust type drift
just verify-conformance         # Run ITF conformance tests
just verify-lean                # Build and check Lean proofs
just verify-all                 # Run all verification
```
