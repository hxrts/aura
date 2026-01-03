# Verification Coverage Matrix

This document provides a comprehensive view of what is formally verified in Aura and how.

## Overview

Aura uses complementary verification approaches:
- **Lean 4**: Mathematical theorem proving for correctness properties
- **Quint**: State machine model checking for protocol behavior
- **Property Tests**: Runtime verification for implementation conformance

## Security-Critical Properties

### Consensus Safety

| Property | Description | Verified By | Status |
|----------|-------------|-------------|--------|
| **Agreement** | All honest witnesses commit to same value | Lean: `Agreement.agreement` | ✓ proven |
| **Validity** | Committed values were honestly proposed | Lean: `Validity.validity` | ✓ proven |
| **Unique Commit** | At most one commit per instance | Quint: `InvariantUniqueCommitPerInstance` | ✓ model checked |
| **Threshold Enforcement** | Commits require k-of-n signatures | Lean: `Validity.commit_has_threshold` | ✓ proven |
| **Prestate Binding** | Commits bound to specific prestate | Lean: `Validity.prestate_binding_unique` | ✓ proven |

### Byzantine Tolerance

| Property | Description | Verified By | Status |
|----------|-------------|-------------|--------|
| **Equivocation Detection** | Conflicting signatures are detectable | Lean: `Equivocation.detection_soundness` | ✓ proven |
| **Equivocator Exclusion** | Detected equivocators excluded from attestation | Quint: `InvariantEquivocatorsExcluded` | ✓ model checked |
| **Honest Majority** | k - f honest witnesses can commit | Lean: `Adversary.honest_majority_sufficient` | ✓ claim |
| **Byzantine Threshold** | f < k Byzantine cannot forge | Lean: `Adversary.byzantine_cannot_forge` | ✓ claim (axiom) |

### FROST Threshold Signatures

| Property | Description | Verified By | Status |
|----------|-------------|-------------|--------|
| **Aggregation Threshold** | Need k shares to aggregate | Lean: `Frost.aggregation_threshold` | ✓ proven |
| **Share Binding** | Shares bound to session/result | Lean: `Frost.share_binding` | ✓ proven |
| **Commitment Before Signing** | Round 1 before Round 2 | Quint: `InvariantCommitmentBeforeSigning` | ✓ model checked |
| **Nonce Uniqueness** | Nonces never reused | Rust: affine state machine + session types | ✓ enforced (see note) |
| **Threshold Unforgeability** | < k cannot forge signature | Lean: `Assumptions.frost_threshold_unforgeability` | axiom |

**Nonce Uniqueness Note**: Nonce reuse prevention is enforced through three mechanisms. Session types in the `AuraConsensus` choreography enforce that `NonceCommit` precedes `SignRequest`, the witness state machine uses `Option::take()` to consume `NonceToken` exactly once, and epoch changes invalidate cached nonces. The Quint model (`InvariantNonceUniquenessNote`) documents this requirement but cannot model-check nonce values due to deterministic test generation.

### Evidence CRDT

| Property | Description | Verified By | Status |
|----------|-------------|-------------|--------|
| **Merge Commutativity** | merge(a,b) = merge(b,a) | Lean: `Evidence.merge_comm_votes` | ✓ proven |
| **Merge Associativity** | merge(a,merge(b,c)) = merge(merge(a,b),c) | Lean: `Evidence.merge_assoc_votes` | ✓ proven |
| **Merge Idempotence** | merge(a,a) = a | Lean: `Evidence.merge_idem` | ✓ proven |
| **Commit Monotonicity** | Commits never removed by merge | Lean: `Evidence.commit_monotonic` | ✓ proven |

### DKG Protocol

| Property | Description | Verified By | Status |
|----------|-------------|-------------|--------|
| **Threshold Bounds** | 1 ≤ t ≤ n always holds | Quint: `InvariantThresholdBounds` | ✓ model checked |
| **Phase Consistency** | Phase transitions follow protocol | Quint: `InvariantPhaseCommitmentCounts` | ✓ model checked |
| **Shares After Verification** | No shares before verification | Quint: `InvariantSharesOnlyAfterVerification` | ✓ model checked |
| **Share Consistency** | Shares from configured participants | Quint: `InvariantShareConsistency` | ✓ model checked |

### Guardian Recovery

| Property | Description | Verified By | Status |
|----------|-------------|-------------|--------|
| **Threshold Bounds** | 1 ≤ t ≤ guardians.size() | Quint: `InvariantThresholdWithinBounds` | ✓ model checked |
| **Approvals Subset** | Only guardians can approve | Quint: `InvariantApprovalsSubsetGuardians` | ✓ model checked |
| **Cooldown Non-Negative** | Timer never goes negative | Quint: `InvariantCooldownNonNegative` | ✓ model checked |
| **Shares From Approvers** | Only approving guardians share | Quint: `InvariantSharesFromApprovers` | ✓ model checked |

## Cryptographic Axioms

These properties are assumed based on cryptographic security proofs:

| Axiom | Cryptographic Justification | Used By |
|-------|----------------------------|---------|
| `frost_threshold_unforgeability` | FROST security proof (Komlo & Goldberg, 2020) | Validity, Frost |
| `frost_uniqueness` | Schnorr signature determinism | Agreement |
| `hash_collision_resistance` | SHA-256/Blake3 security | Validity (prestate binding) |
| `byzantine_threshold` | BFT assumption: k > f | All safety properties |

## Rust Implementation Coverage

### Consensus Module (`crates/aura-consensus/src/consensus/`)

| File | Key Functions | Verified Spec | Coverage |
|------|---------------|---------------|----------|
| `types.rs` | `ConsensusId`, `CommitFact`, `WitnessVote` | Quint: `consensus/core.qnt` TYPES | ✓ types match |
| `protocol.rs` | `run_consensus`, `participate_as_witness` | Quint: `submitWitnessShare`, `commitViaFastPath` | partial |
| `witness.rs` | `WitnessSet`, `WitnessTracker` | Quint: `WitnessState` | partial |
| `frost.rs` | `sign_with_nonce`, `aggregate_signatures` | Quint: `consensus/frost.qnt` | ✓ types match |

### Core Crypto (`crates/aura-core/src/crypto/`)

| File | Key Functions | Verified Spec | Coverage |
|------|---------------|---------------|----------|
| `tree_signing.rs` | FROST primitives | Lean: `Frost.lean` | axiom-based |

## Coverage Gaps

### Requiring Future Work

| Gap | Description | Priority | Proposed Approach |
|-----|-------------|----------|-------------------|
| **Pure Core Extraction** | Consensus protocol mixes effects | High | Extract effect-free state machine |
| **ITF Conformance** | No trace-based testing | High | Generate ITF traces, replay in Rust |
| **Liveness Proofs** | Temporal properties as axioms | Medium | Strengthen Quint model checking |
| **Recovery Edge Cases** | Guardian unavailability scenarios | Medium | Add more Quint scenarios |
| **Network Partitions** | Before-GST behavior | Low | Adversarial Quint scenarios |

### Partially Covered

| Area | Current Coverage | Gap |
|------|------------------|-----|
| Agreement | Lean proof complete | Needs ITF trace validation |
| Validity | Lean proof complete | Needs pure core extraction |
| FROST | Axiom-based | Needs integration testing |
| DKG | Model checked | Needs Lean proofs for crypto |
| Recovery | Model checked | Needs liveness properties |

## Verification Commands

### Lean Proofs

```bash
cd verification/lean
lake build           # Build and verify all proofs
lake check           # Type check only
```

### Quint Model Checking

```bash
cd verification/quint

# Type check all specs
for f in *.qnt; do quint typecheck "$f"; done

# Check specific invariants
quint run --invariant=InvariantUniqueCommitPerInstance consensus/core.qnt
quint run --invariant=WellFormedState consensus/core.qnt
quint run --invariant=WellFormedDkgState keys/dkg.qnt
quint run --invariant=WellFormedRecoveryState recovery.qnt
quint run --invariant=WellFormedFrostState consensus/frost.qnt
```

### Property Tests

```bash
cargo test --workspace -- --test-threads=1  # Run all tests
cargo test consensus                         # Consensus-specific tests
```

## Related Documentation

- [verification/README.md](../verification/README.md) - Verification overview and Quint-Lean correspondence map
- [verification/lean/README.md](../verification/lean/README.md) - Lean module documentation
- [verification/quint/README.md](../verification/quint/README.md) - Quint specification documentation
- [verification/quint/STYLE.md](../verification/quint/STYLE.md) - Quint coding conventions
- [docs/004_distributed_systems_contract.md](004_distributed_systems_contract.md) - System guarantees
- [docs/104_consensus.md](104_consensus.md) - Consensus protocol design
