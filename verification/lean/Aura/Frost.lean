-- Core definitions for FROST protocol state machine verification.
-- Proves session/round consistency for threshold signature aggregation.

namespace Aura.Frost

/-!
# FROST Protocol Orchestration

This module models the FROST signing state machine and proves that
aggregate is never called with mixed sessions or rounds.

**Why this matters**: FROST is a threshold signature scheme where k-of-n
participants produce shares that combine into one valid signature. If shares
from different sessions or rounds are mixed, the resulting "signature" would
be invalid or could leak key material. This proof ensures the aggregator
only combines shares from the same signing session.
-/

-- Session identifier: groups shares from one signing request.
-- Each threshold signing operation creates a unique session.
structure SessionId where
  id : Nat
  deriving Repr, DecidableEq

-- Manual BEq needed because derived BEq creates opaque functions.
-- The manual version uses decide, which simp can unfold for proofs.
instance : BEq SessionId where
  beq a b := a.id == b.id

-- Round within a session (commitment round, signing round, etc.).
-- FROST has multiple rounds; shares must match the same round.
structure Round where
  idx : Nat
  deriving Repr, DecidableEq

-- Manual BEq for Round (same reasoning as SessionId).
instance : BEq Round where
  beq a b := a.idx == b.idx

-- Witness identifier: which threshold participant produced this share.
-- In Aura, witnesses are devices in the commitment tree.
structure WitnessId where
  id : Nat
  deriving BEq, Repr, DecidableEq

-- Abstract share data (the actual cryptographic share value).
-- Real implementation uses Schnorr signature shares.
structure ShareData where
  value : Nat
  deriving BEq, Repr

-- A signature share: one participant's contribution to the threshold signature.
-- Tagged with session, round, and witness for routing and validation.
structure Share where
  sid : SessionId
  round : Round
  witness : WitnessId
  data : ShareData
  deriving BEq, Repr

-- Aggregator state: collects shares until threshold is reached.
-- The aggregator is typically the signing request initiator.
structure AggregatorState where
  pending : List Share
  deriving BEq, Repr

-- Check if aggregation is safe: all shares must be from the same session/round.
-- Returns false for empty list (need at least k shares to aggregate).
def canAggregate (state : AggregatorState) : Bool :=
  match state.pending with
  | [] => false
  | sh :: tl =>
    -- All remaining shares must match the first share's session and round
    tl.all (fun sh' => sh'.sid == sh.sid && sh'.round == sh.round)

-- Abstract signature result. In practice, this is a Schnorr signature
-- that verifies against the group public key.
structure Signature where
  value : Nat
  deriving BEq, Repr

-- Aggregate shares into a signature (if valid).
-- Only succeeds when canAggregate returns true.
def aggregate (state : AggregatorState) : Option Signature :=
  if canAggregate state then
    some { value := 0 }  -- Abstract combination (real impl does Lagrange interpolation)
  else
    none

-- **Lemma**: List.all means the predicate holds for every element.
-- Proof by induction: base case is vacuous, inductive case splits the conjunction.
theorem list_all_forall {α : Type} (p : α → Bool) (xs : List α) :
    xs.all p = true → ∀ x ∈ xs, p x = true := by
  induction xs with
  | nil => intro _ x hx; cases hx  -- No elements in [], so vacuously true
  | cons y ys ih =>
    intro hall x hx
    -- hall : (p y && ys.all p) = true, which means p y = true AND ys.all p = true
    simp only [List.all, Bool.and_eq_true] at hall
    cases hx with
    | head => exact hall.1           -- x = y, so p x = p y = true
    | tail _ htail => exact ih hall.2 x htail  -- x ∈ ys, use inductive hypothesis

-- **Lemma**: BEq equality for SessionId implies propositional equality.
-- Needed because beq uses decide, and we need to convert Bool to Prop.
theorem SessionId.eq_of_beq {a b : SessionId} (h : (a == b) = true) : a = b := by
  cases a with | mk aid =>
  cases b with | mk bid =>
  simp only [SessionId.mk.injEq]
  simp only [BEq.beq] at h
  exact of_decide_eq_true h  -- Convert decide (aid = bid) = true to aid = bid

-- **Lemma**: BEq equality for Round implies propositional equality.
theorem Round.eq_of_beq {a b : Round} (h : (a == b) = true) : a = b := by
  cases a with | mk aidx =>
  cases b with | mk bidx =>
  simp only [Round.mk.injEq]
  simp only [BEq.beq] at h
  exact of_decide_eq_true h

-- **Helper**: Extract equalities from a conjunction of BEq checks.
-- If (a == b && c == d) = true, then a = b and c = d.
theorem beq_and_true_imp {a b : SessionId} {c d : Round}
    (h : (a == b && c == d) = true) : a = b ∧ c = d := by
  simp only [Bool.and_eq_true] at h
  exact ⟨SessionId.eq_of_beq h.1, Round.eq_of_beq h.2⟩

/-!
## Main Theorem: Aggregation Session/Round Consistency

If `aggregate` succeeds, then ALL shares in the pending list have the same
session ID and round. This is the key safety property that prevents mixing
shares from different signing operations.

Proof outline:
1. aggregate succeeds only when canAggregate returns true
2. canAggregate checks all shares match the first share's session/round
3. We extract this property via list_all_forall and beq_and_true_imp
-/
theorem aggregate_same_session_round
  (state : AggregatorState) (sig : Signature)
  (h : aggregate state = some sig) :
  ∃ sid rnd, ∀ sh ∈ state.pending, sh.sid = sid ∧ sh.round = rnd := by
  unfold aggregate at h
  split at h
  case isTrue hcan =>
    -- Case: canAggregate state = true, so aggregate returns Some sig
    cases hpend : state.pending with
    | nil =>
      -- Empty list: canAggregate [] = false, contradicts hcan
      simp only [canAggregate, hpend] at hcan
      exact False.elim (Bool.false_ne_true hcan)
    | cons first rest =>
      -- Non-empty list: first share defines the session/round
      refine ⟨first.sid, first.round, ?_⟩
      intro x hx
      cases hx with
      | head => exact ⟨rfl, rfl⟩  -- x = first, trivially matches
      | tail _ htail =>
        -- x ∈ rest, so canAggregate's List.all ensures x matches first
        simp only [canAggregate, hpend] at hcan
        have hpred := list_all_forall _ rest hcan x htail
        exact beq_and_true_imp hpred
  case isFalse hcant =>
    -- Case: canAggregate = false, so aggregate returns None
    -- But h says aggregate = Some sig, contradiction
    cases h

end Aura.Frost
