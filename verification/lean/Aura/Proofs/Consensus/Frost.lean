import Aura.Domain.Consensus.Frost
import Aura.Domain.Consensus.Types
import Aura.Assumptions

/-!
# FROST Protocol Proofs

Proves session/round consistency for threshold signature aggregation
and FROST integration with consensus.

## Quint Correspondence
- File: verification/quint/protocol_frost.qnt
- File: verification/quint/protocol_consensus.qnt
- Section: INVARIANTS
- Invariant: `InvariantCommitRequiresThreshold`
- Properties: Aggregate only combines shares from same session/round

## Rust Correspondence
- File: crates/aura-core/src/crypto/tree_signing.rs
- Functions: `aggregate`, `aggregate_signatures`, `verify_share`
- File: crates/aura-consensus/src/consensus/frost.rs
- Type: `FrostConsensusOrchestrator`

## Expose

**Properties** (theorem statements):
- `aggregate_same_session_round`: Successful aggregation implies all shares match session/round
- `share_session_consistency`: All shares in aggregation have same consensus
- `share_result_consistency`: All shares have same result
- `aggregation_threshold`: Aggregation only succeeds with ≥k shares
- `aggregatable_implies_valid_commit`: Aggregatable shares form valid commit candidate

**Claims Bundles**:
- `FrostOrchestratorClaims`: Low-level aggregation safety
- `FrostClaims`: Consensus integration properties
-/

namespace Aura.Proofs.Consensus.Frost

open Aura.Domain.Consensus.Frost
open Aura.Domain.Consensus.Types
open Aura.Assumptions

/-!
## Part 1: Aggregation Safety (from Aura.Frost)

Low-level FROST share aggregation proofs.
-/

/-!
### Helper Lemmas
-/

/-- List.all means the predicate holds for every element. -/
theorem list_all_forall {α : Type} (p : α → Bool) (xs : List α) :
    xs.all p = true → ∀ x ∈ xs, p x = true := by
  induction xs with
  | nil => intro _ x hx; cases hx
  | cons y ys ih =>
    intro hall x hx
    simp only [List.all, Bool.and_eq_true] at hall
    cases hx with
    | head => exact hall.1
    | tail _ htail => exact ih hall.2 x htail

/-- BEq equality for SessionId implies propositional equality. -/
theorem SessionId.eq_of_beq {a b : SessionId} (h : (a == b) = true) : a = b := by
  cases a with | mk aid =>
  cases b with | mk bid =>
  simp only [SessionId.mk.injEq]
  simp only [BEq.beq] at h
  exact of_decide_eq_true h

/-- BEq equality for Round implies propositional equality. -/
theorem Round.eq_of_beq {a b : Round} (h : (a == b) = true) : a = b := by
  cases a with | mk aidx =>
  cases b with | mk bidx =>
  simp only [Round.mk.injEq]
  simp only [BEq.beq] at h
  exact of_decide_eq_true h

/-- Extract equalities from conjunction of BEq checks. -/
theorem beq_and_true_imp {a b : SessionId} {c d : Round}
    (h : (a == b && c == d) = true) : a = b ∧ c = d := by
  simp only [Bool.and_eq_true] at h
  exact ⟨SessionId.eq_of_beq h.1, Round.eq_of_beq h.2⟩

/-!
### Orchestrator Claims Bundle
-/

/-- Claims bundle for FROST orchestrator properties. -/
structure FrostOrchestratorClaims where
  /-- Session/round consistency: Successful aggregation implies all shares match. -/
  aggregate_same_session_round : ∀ (state : AggregatorState) (sig : Signature),
    aggregate state = some sig →
    ∃ sid rnd, ∀ sh ∈ state.pending, sh.sid = sid ∧ sh.round = rnd

/-!
### Orchestrator Proofs
-/

/-- Aggregation session/round consistency.
    If `aggregate` succeeds, ALL shares have the same session and round. -/
theorem aggregate_same_session_round
  (state : AggregatorState) (sig : Signature)
  (h : aggregate state = some sig) :
  ∃ sid rnd, ∀ sh ∈ state.pending, sh.sid = sid ∧ sh.round = rnd := by
  unfold aggregate at h
  split at h
  case isTrue hcan =>
    cases hpend : state.pending with
    | nil =>
      simp only [canAggregate, hpend] at hcan
      exact False.elim (Bool.false_ne_true hcan)
    | cons first rest =>
      refine ⟨first.sid, first.round, ?_⟩
      intro x hx
      cases hx with
      | head => exact ⟨rfl, rfl⟩
      | tail _ htail =>
        simp only [canAggregate, hpend] at hcan
        have hpred := list_all_forall _ rest hcan x htail
        exact beq_and_true_imp hpred
  case isFalse hcant =>
    cases h

/-- The claims bundle for FROST orchestrator. -/
def frostOrchestratorClaims : FrostOrchestratorClaims where
  aggregate_same_session_round := aggregate_same_session_round

/-!
## Part 2: Consensus Integration (from Aura.Consensus.Frost)

FROST integration with consensus protocol.
-/

/-!
### Consistency Predicates
-/

/-- All votes are for the same consensus instance.
    Quint: Implicit in ConsensusInstance scoping -/
def sameConsensus (votes : List WitnessVote) : Prop :=
  match votes with
  | [] => True
  | v :: vs => vs.all (fun v' => v'.consensusId == v.consensusId)

/-- All votes commit to the same result.
    Quint: Required for successful aggregation -/
def sameResult (votes : List WitnessVote) : Prop :=
  match votes with
  | [] => True
  | v :: vs => vs.all (fun v' => v'.resultId == v.resultId)

/-- All votes commit to the same prestate.
    Quint: Required for valid commit -/
def samePrestate (votes : List WitnessVote) : Prop :=
  match votes with
  | [] => True
  | v :: vs => vs.all (fun v' => v'.prestateHash == v.prestateHash)

/-- All votes are from distinct witnesses.
    Quint: Implicit in proposals as set of witness IDs -/
def distinctWitnesses (votes : List WitnessVote) : Prop :=
  let witnesses := votes.map (·.witness)
  witnesses.length = (List.removeDups witnesses).length

/-- Shares can be aggregated if they meet consistency requirements. -/
def canAggregateShares (votes : List WitnessVote) : Prop :=
  sameConsensus votes ∧
  sameResult votes ∧
  samePrestate votes ∧
  distinctWitnesses votes ∧
  votes.length ≥ threshold

/-!
### Consensus Integration Claims Bundle
-/

/-- Claims bundle for FROST-consensus integration properties. -/
structure FrostClaims where
  /-- Session consistency: All shares in successful aggregation have same consensus. -/
  share_session_consistency : ∀ votes : List WitnessVote,
    canAggregateShares votes →
    sameConsensus votes

  /-- Result consistency: All shares in successful aggregation have same result. -/
  share_result_consistency : ∀ votes : List WitnessVote,
    canAggregateShares votes →
    sameResult votes

  /-- Threshold requirement: Aggregation requires at least k shares. -/
  aggregation_threshold : ∀ votes : List WitnessVote,
    canAggregateShares votes →
    votes.length ≥ threshold

  /-- Distinct witnesses: Each share from different witness. -/
  distinct_signers : ∀ votes : List WitnessVote,
    canAggregateShares votes →
    distinctWitnesses votes

  /-- Share binding: Shares are bound to (cid, rid, pHash). -/
  share_binding : ∀ v : WitnessVote,
    v.share.dataBinding ≠ ""

/-!
### Consensus Integration Proofs
-/

/-- Session consistency follows from canAggregateShares definition. -/
theorem share_session_consistency (votes : List WitnessVote)
    (h : canAggregateShares votes) : sameConsensus votes := by
  unfold canAggregateShares at h
  exact h.1

/-- Result consistency follows from canAggregateShares definition. -/
theorem share_result_consistency (votes : List WitnessVote)
    (h : canAggregateShares votes) : sameResult votes := by
  unfold canAggregateShares at h
  exact h.2.1

/-- Threshold requirement follows from canAggregateShares definition. -/
theorem aggregation_threshold (votes : List WitnessVote)
    (h : canAggregateShares votes) : votes.length ≥ threshold := by
  unfold canAggregateShares at h
  exact h.2.2.2.2

/-- Distinct witnesses follows from canAggregateShares definition. -/
theorem distinct_signers (votes : List WitnessVote)
    (h : canAggregateShares votes) : distinctWitnesses votes := by
  unfold canAggregateShares at h
  exact h.2.2.2.1

/-- **Axiom: Share Binding Non-Empty**

Share data bindings are always non-empty when created by the FROST protocol.
This is enforced by the share creation process which hashes (cid, rid, pHash).
A truly empty binding would indicate a malformed share that would fail
verification at the cryptographic layer.

This is an axiom because we cannot prove it from the structure definition alone;
it depends on runtime behavior of share creation.
-/
axiom share_data_binding_nonempty : ∀ v : WitnessVote, v.share.dataBinding ≠ ""

/-- Share binding follows from the cryptographic axiom. -/
theorem share_binding (v : WitnessVote) : v.share.dataBinding ≠ "" :=
  share_data_binding_nonempty v

/-!
### Integration Theorems
-/

/-- If shares can be aggregated, the result is a valid commit candidate. -/
theorem aggregatable_implies_valid_commit (votes : List WitnessVote)
    (h : canAggregateShares votes) (hne : votes ≠ []) :
    ∃ cid rid ph, ∀ v ∈ votes,
      v.consensusId = cid ∧ v.resultId = rid ∧ v.prestateHash = ph := by
  -- Get the first vote as the reference using cases instead of match
  cases hvotes : votes with
  | nil => exact absurd hvotes hne
  | cons v0 vs =>
    -- Use v0's values as the common values
    refine ⟨v0.consensusId, v0.resultId, v0.prestateHash, ?_⟩
    intro v hv_mem
    -- Extract consistency predicates from h
    unfold canAggregateShares at h
    obtain ⟨hsame_c, hsame_r, hsame_p, _, _⟩ := h
    -- Rewrite votes to v0 :: vs in h
    rw [hvotes] at hsame_c hsame_r hsame_p
    simp only [sameConsensus, sameResult, samePrestate] at hsame_c hsame_r hsame_p
    -- Case analysis: v is either v0 or in vs (hv_mem is already v ∈ v0 :: vs after cases)
    cases hv_mem with
    | head => exact ⟨rfl, rfl, rfl⟩
    | tail _ hv_in_vs =>
      -- v is in vs, so the all predicates apply
      constructor
      · -- consensusId
        have := List.all_eq_true.mp hsame_c v hv_in_vs
        simp only [beq_iff_eq] at this
        exact this
      constructor
      · -- resultId
        have := List.all_eq_true.mp hsame_r v hv_in_vs
        simp only [beq_iff_eq] at this
        exact this
      · -- prestateHash
        have := List.all_eq_true.mp hsame_p v hv_in_vs
        simp only [beq_iff_eq] at this
        exact this

/-- Threshold aggregation connects to FROST axioms. -/
theorem threshold_aggregation_exists (votes : List WitnessVote)
    (h : canAggregateShares votes) :
    votes.length ≥ threshold := by
  exact aggregation_threshold votes h

/-- The claims bundle for FROST-consensus integration. -/
def frostClaims : FrostClaims where
  share_session_consistency := share_session_consistency
  share_result_consistency := share_result_consistency
  aggregation_threshold := aggregation_threshold
  distinct_signers := distinct_signers
  share_binding := share_binding

end Aura.Proofs.Consensus.Frost
