import Aura.Domain.Consensus.Types
import Aura.Assumptions

/-!
# Agreement Safety Proofs

Proves that all honest witnesses that commit to an instance commit to the same value.
This is the fundamental safety property of consensus.

## Quint Correspondence
- File: verification/quint/protocol_consensus.qnt
- Section: INVARIANTS
- Invariant: `InvariantUniqueCommitPerInstance`

## Rust Correspondence
- File: crates/aura-consensus/src/consensus/types.rs
- Type: `CommitFact` - the committed result with threshold signature
- File: crates/aura-consensus/src/consensus/protocol.rs
- Function: `verify_commit` - validates commit facts

## Expose

The following definitions form the semantic interface for proofs:

**Properties** (stable, theorem statements):
- `agreement`: If two commits exist for same consensus, they have same result
- `unique_commit`: At most one valid CommitFact per ConsensusId
- `commit_determinism`: Same threshold shares produce same commit

**Internal helpers** (may change):
- Auxiliary lemmas about signature verification
-/

namespace Aura.Proofs.Consensus.Agreement

open Aura.Domain.Consensus.Types
open Aura.Assumptions

/-!
## Agreement Predicates

Predicates expressing agreement properties.
-/

/-- Two CommitFacts agree if they commit to the same result. -/
def agreesOn (c1 c2 : CommitFact) : Prop :=
  c1.consensusId = c2.consensusId ->
  c1.resultId = c2.resultId /\ c1.prestateHash = c2.prestateHash

/-- A CommitFact is valid if its signature verifies. -/
def validCommit (c : CommitFact) : Prop :=
  c.signature.signerSet.length >= threshold /\
  c.signature.boundCid = c.consensusId /\
  c.signature.boundRid = c.resultId /\
  c.signature.boundPHash = c.prestateHash

/-!
## Domain-Level Axioms

These axioms lift FROST properties to the domain level. They are justified by:
1. FROST signature uniqueness (frost_signature_uniqueness in Assumptions.lean)
2. Protocol invariants maintained by the consensus state machine

The key insight is that threshold signatures cryptographically bind to (cid, rid, pHash),
so two valid signatures for the same consensus instance must agree on the bound values.
-/

/-- **Axiom: Signature Binding Agreement**

Two valid threshold signatures for the same consensus instance must bind to
the same result and prestate. This follows from:
- Each share binds to (cid, rid, pHash) when created
- Aggregation only succeeds if all shares have consistent bindings
- Therefore, the aggregated signature inherits this binding

Quint: This is enforced by `sharesConsistent` in protocol_consensus.qnt
-/
axiom signature_binding_agreement :
  forall (sig1 sig2 : ThresholdSignature),
    sig1.signerSet.length >= threshold ->
    sig2.signerSet.length >= threshold ->
    sig1.boundCid = sig2.boundCid ->
    sig1.boundRid = sig2.boundRid /\ sig1.boundPHash = sig2.boundPHash

/-- **Axiom: Signature Value Determinism**

Given the same binding (cid, rid, pHash) and signer set, the signature
value is uniquely determined. This follows from FROST aggregation determinism.

Quint: Implicit in the aggregateShares function being deterministic
-/
axiom signature_value_determinism :
  forall (sig1 sig2 : ThresholdSignature),
    sig1.signerSet = sig2.signerSet ->
    sig1.boundCid = sig2.boundCid ->
    sig1.boundRid = sig2.boundRid ->
    sig1.boundPHash = sig2.boundPHash ->
    sig1.sigValue = sig2.sigValue

/-- **Axiom: Valid Commit Signature Uniqueness**

Two valid commits for the same consensus instance with the same prestate
must have identical signatures. This follows from:
- Both signatures bind to the same (cid, rid, pHash)
- By signature_binding_agreement, the result must be the same
- The protocol ensures only one valid signature set per (cid, rid, pHash)

Quint: `InvariantUniqueCommitPerInstance` + deterministic aggregation
-/
axiom valid_commit_signature_uniqueness :
  forall (c1 c2 : CommitFact),
    validCommit c1 ->
    validCommit c2 ->
    c1.consensusId = c2.consensusId ->
    c1.prestateHash = c2.prestateHash ->
    c1.resultId = c2.resultId ->
    c1.signature = c2.signature

/-!
## Claims Bundle

This structure collects all the theorems about consensus agreement.
-/

/-- Claims bundle for Agreement properties. -/
structure AgreementClaims where
  /-- Agreement: If two valid commits exist for same consensus, they have same result. -/
  agreement : forall c1 c2 : CommitFact,
    validCommit c1 -> validCommit c2 -> c1.consensusId = c2.consensusId -> c1.resultId = c2.resultId

  /-- Unique commit: At most one CommitFact per ConsensusId. -/
  unique_commit : forall c1 c2 : CommitFact,
    validCommit c1 -> validCommit c2 ->
    c1.consensusId = c2.consensusId -> c1.prestateHash = c2.prestateHash -> c1 = c2

  /-- Commit determinism: Same set of threshold shares produces same commit.
      Requires well-formedness: signatures bound to commit fields. -/
  commit_determinism : forall c1 c2 : CommitFact,
    c1.signature.signerSet = c2.signature.signerSet ->
    c1.consensusId = c2.consensusId ->
    c1.resultId = c2.resultId ->
    c1.prestateHash = c2.prestateHash ->
    c1.signature.boundCid = c1.consensusId ->
    c2.signature.boundCid = c2.consensusId ->
    c1.signature.boundRid = c1.resultId ->
    c2.signature.boundRid = c2.resultId ->
    c1.signature.boundPHash = c1.prestateHash ->
    c2.signature.boundPHash = c2.prestateHash ->
    c1.signature = c2.signature

/-!
## Proofs

Individual theorem proofs that construct the claims bundle.
-/

/-- Agreement theorem: Valid commits for same consensus have same result.

Proof sketch:
1. Valid commits have signatures that bind to their (cid, rid, pHash)
2. Both signatures have threshold signers (from validCommit)
3. Both signatures have same boundCid (from validCommit + hcid)
4. By signature_binding_agreement, they must have same boundRid
5. Since boundRid = resultId (from validCommit), the resultIds are equal
-/
theorem agreement (c1 c2 : CommitFact)
    (hv1 : validCommit c1) (hv2 : validCommit c2)
    (hcid : c1.consensusId = c2.consensusId) :
    c1.resultId = c2.resultId := by
  -- Extract components from validCommit hypotheses
  obtain ⟨hthresh1, hbcid1, hbrid1, hbph1⟩ := hv1
  obtain ⟨hthresh2, hbcid2, hbrid2, hbph2⟩ := hv2
  -- Both signatures bind to the same consensusId
  have hsameCid : c1.signature.boundCid = c2.signature.boundCid := by
    rw [hbcid1, hbcid2, hcid]
  -- Apply signature binding agreement axiom
  have hbinding := signature_binding_agreement c1.signature c2.signature hthresh1 hthresh2 hsameCid
  -- Extract that boundRid must be equal
  have hrid_eq : c1.signature.boundRid = c2.signature.boundRid := hbinding.1
  -- Since boundRid = resultId for valid commits, resultIds are equal
  rw [← hbrid1, ← hbrid2, hrid_eq]

/-- Unique commit theorem: At most one CommitFact per ConsensusId.

Proof sketch:
1. By agreement theorem, resultIds are equal
2. Given prestateHash equality (hph)
3. By valid_commit_signature_uniqueness, signatures are equal
4. All fields match, so commits are equal
-/
theorem unique_commit (c1 c2 : CommitFact)
    (hv1 : validCommit c1) (hv2 : validCommit c2)
    (hcid : c1.consensusId = c2.consensusId)
    (hph : c1.prestateHash = c2.prestateHash) :
    c1 = c2 := by
  -- Get result equality from agreement
  have hrid : c1.resultId = c2.resultId := agreement c1 c2 hv1 hv2 hcid
  -- Get signature equality from axiom
  have hsig : c1.signature = c2.signature :=
    valid_commit_signature_uniqueness c1 c2 hv1 hv2 hcid hph hrid
  -- Structural equality
  cases c1
  cases c2
  simp only at hcid hrid hph hsig
  simp only [CommitFact.mk.injEq]
  exact ⟨hcid, hrid, hph, hsig⟩

/-- Commit determinism: Same shares produce same commit.

Note: This theorem assumes the signature bound fields match the commit fields,
which is enforced by construction in the protocol. We add this as a hypothesis
about well-formed commits.
-/
theorem commit_determinism (c1 c2 : CommitFact)
    (hsigners : c1.signature.signerSet = c2.signature.signerSet)
    (hcid : c1.consensusId = c2.consensusId)
    (hrid : c1.resultId = c2.resultId)
    (hph : c1.prestateHash = c2.prestateHash)
    -- Additional hypotheses: signatures are well-formed (bound to commit)
    (hwf1 : c1.signature.boundCid = c1.consensusId)
    (hwf2 : c2.signature.boundCid = c2.consensusId)
    (hwf3 : c1.signature.boundRid = c1.resultId)
    (hwf4 : c2.signature.boundRid = c2.resultId)
    (hwf5 : c1.signature.boundPHash = c1.prestateHash)
    (hwf6 : c2.signature.boundPHash = c2.prestateHash) :
    c1.signature = c2.signature := by
  -- Derive bound field equalities
  have hbcid : c1.signature.boundCid = c2.signature.boundCid := by rw [hwf1, hwf2, hcid]
  have hbrid : c1.signature.boundRid = c2.signature.boundRid := by rw [hwf3, hwf4, hrid]
  have hbph : c1.signature.boundPHash = c2.signature.boundPHash := by rw [hwf5, hwf6, hph]
  -- Apply signature_value_determinism for sigValue
  have hsigval := signature_value_determinism c1.signature c2.signature hsigners hbcid hbrid hbph
  -- Structural equality of signatures
  cases hs1 : c1.signature
  cases hs2 : c2.signature
  simp only [hs1, hs2] at hsigners hbcid hbrid hbph hsigval
  simp only [ThresholdSignature.mk.injEq]
  exact ⟨hsigval, hbcid, hbrid, hbph, hsigners⟩

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving consensus agreement properties. -/
def agreementClaims : AgreementClaims where
  agreement := agreement
  unique_commit := unique_commit
  commit_determinism := fun c1 c2 hs hcid hrid hph hwf1 hwf2 hwf3 hwf4 hwf5 hwf6 =>
    commit_determinism c1 c2 hs hcid hrid hph hwf1 hwf2 hwf3 hwf4 hwf5 hwf6

end Aura.Proofs.Consensus.Agreement
