-- Core definitions for FROST protocol state machine verification

namespace Aura.Frost

/-!
# FROST Protocol Orchestration

This module models the FROST signing state machine and proves that
aggregate is never called with mixed sessions or rounds.
-/

/-- Session identifier -/
structure SessionId where
  id : Nat
  deriving Repr, DecidableEq

/-- Manual BEq for SessionId (needed for lawful proofs) -/
instance : BEq SessionId where
  beq a b := a.id == b.id

/-- Commitment round -/
structure Round where
  idx : Nat
  deriving Repr, DecidableEq

/-- Manual BEq for Round (needed for lawful proofs) -/
instance : BEq Round where
  beq a b := a.idx == b.idx

/-- Witness identifier -/
structure WitnessId where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Abstract share data -/
structure ShareData where
  value : Nat  -- Abstract
  deriving BEq, Repr

/-- A signature share -/
structure Share where
  sid : SessionId
  round : Round
  witness : WitnessId
  data : ShareData
  deriving BEq, Repr

/-- Aggregator state -/
structure AggregatorState where
  pending : List Share
  deriving BEq, Repr

/-- Check if all pending shares are from same session and round -/
def canAggregate (state : AggregatorState) : Bool :=
  match state.pending with
  | [] => false
  | sh :: tl =>
    tl.all (fun sh' => sh'.sid == sh.sid && sh'.round == sh.round)

/-- Abstract signature result -/
structure Signature where
  value : Nat  -- Abstract
  deriving BEq, Repr

/-- Aggregate shares if valid -/
def aggregate (state : AggregatorState) : Option Signature :=
  if canAggregate state then
    some { value := 0 }  -- Abstract combination
  else
    none

/-- Helper: List.all means predicate holds for all elements -/
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

/-- BEq for SessionId implies equality -/
theorem SessionId.eq_of_beq {a b : SessionId} (h : (a == b) = true) : a = b := by
  cases a with | mk aid =>
  cases b with | mk bid =>
  simp only [SessionId.mk.injEq]
  -- Simplify the BEq to get decidable equality
  simp only [BEq.beq] at h
  -- h : decide (aid = bid) = true
  exact of_decide_eq_true h

/-- BEq for Round implies equality -/
theorem Round.eq_of_beq {a b : Round} (h : (a == b) = true) : a = b := by
  cases a with | mk aidx =>
  cases b with | mk bidx =>
  simp only [Round.mk.injEq]
  simp only [BEq.beq] at h
  exact of_decide_eq_true h

/-- Helper for extracting equality from beq on derived structures -/
theorem beq_and_true_imp {a b : SessionId} {c d : Round}
    (h : (a == b && c == d) = true) : a = b ∧ c = d := by
  simp only [Bool.and_eq_true] at h
  exact ⟨SessionId.eq_of_beq h.1, Round.eq_of_beq h.2⟩

/-- Theorem: Successful aggregation implies all shares match

    The proof structure is:
    1. aggregate succeeds only when canAggregate returns true
    2. canAggregate checks that all shares have the same session and round as the first share
    3. We extract this property from the List.all check
-/
theorem aggregate_same_session_round
  (state : AggregatorState) (sig : Signature)
  (h : aggregate state = some sig) :
  ∃ sid rnd, ∀ sh ∈ state.pending, sh.sid = sid ∧ sh.round = rnd := by
  unfold aggregate at h
  split at h
  case isTrue hcan =>
    cases hpend : state.pending with
    | nil =>
      -- canAggregate [] = false, so hcan : false = true is a contradiction
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
    -- canAggregate is false, so aggregate returns none, contradiction with h : none = some sig
    cases h

end Aura.Frost
