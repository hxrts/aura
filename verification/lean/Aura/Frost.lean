/-!
# FROST Protocol Orchestration Proofs

Proves session/round consistency for threshold signature aggregation,
ensuring shares from different sessions are never mixed.

## Quint Correspondence
- File: verification/quint/protocol_frost.qnt
- Section: INVARIANTS
- Properties: Aggregate only combines shares from same session/round

## Rust Correspondence
- File: crates/aura-core/src/crypto/tree_signing.rs
- Type: `SigningSession`, `Share`
- Function: `aggregate` - combines threshold shares into signature

## Expose

**Types**:
- `SessionId`: Groups shares from one signing request
- `Round`: Round within a session (commitment, signing)
- `WitnessId`: Threshold participant identifier
- `Share`: Signature share with session/round/witness tagging
- `AggregatorState`: Collects shares until threshold

**Operations** (stable):
- `canAggregate`: Check if shares are from same session/round
- `aggregate`: Combine shares into signature (if valid)

**Properties** (stable, theorem statements):
- `aggregate_same_session_round`: Successful aggregation implies all shares match

**Internal helpers** (may change):
- `list_all_forall`: List.all means predicate holds for all elements
- `SessionId.eq_of_beq`, `Round.eq_of_beq`: BEq to propositional equality
-/

namespace Aura.Frost

/-!
## Core Types

FROST protocol data structures.
-/

/-- Session identifier: groups shares from one signing request.
    Rust: aura-core/src/crypto/tree_signing.rs -/
structure SessionId where
  id : Nat
  deriving Repr, DecidableEq

/-- Manual BEq for SessionId (unfoldable for proofs). -/
instance : BEq SessionId where
  beq a b := a.id == b.id

/-- Round within a session (commitment, signing, etc.).
    Rust: Corresponds to FROST protocol rounds -/
structure Round where
  idx : Nat
  deriving Repr, DecidableEq

/-- Manual BEq for Round. -/
instance : BEq Round where
  beq a b := a.idx == b.idx

/-- Witness identifier: which threshold participant produced this share.
    Rust: Maps to devices in commitment tree -/
structure WitnessId where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Abstract share data (actual cryptographic share value).
    Rust: Schnorr signature share -/
structure ShareData where
  value : Nat
  deriving BEq, Repr

/-- A signature share: one participant's contribution.
    Rust: aura-core/src/crypto/tree_signing.rs::Share -/
structure Share where
  sid : SessionId
  round : Round
  witness : WitnessId
  data : ShareData
  deriving BEq, Repr

/-- Aggregator state: collects shares until threshold.
    Rust: Part of FrostOrchestrator state -/
structure AggregatorState where
  pending : List Share
  deriving BEq, Repr

/-- Abstract signature result.
    Rust: Schnorr signature verifying against group public key -/
structure Signature where
  value : Nat
  deriving BEq, Repr

/-!
## Aggregation Functions

Check and perform share aggregation.
-/

/-- Check if aggregation is safe: all shares from same session/round.
    Quint: Precondition for aggregateShares action -/
def canAggregate (state : AggregatorState) : Bool :=
  match state.pending with
  | [] => false
  | sh :: tl =>
    tl.all (fun sh' => sh'.sid == sh.sid && sh'.round == sh.round)

/-- Aggregate shares into a signature (if valid).
    Quint: Only succeeds when canAggregate returns true -/
def aggregate (state : AggregatorState) : Option Signature :=
  if canAggregate state then
    some { value := 0 }
  else
    none

/-!
## Claims Bundle

FROST aggregation safety properties.
-/

/-- Claims bundle for FROST properties. -/
structure FrostOrchestratorClaims where
  /-- Session/round consistency: Successful aggregation implies all shares match. -/
  aggregate_same_session_round : ∀ (state : AggregatorState) (sig : Signature),
    aggregate state = some sig →
    ∃ sid rnd, ∀ sh ∈ state.pending, sh.sid = sid ∧ sh.round = rnd

/-!
## Helper Lemmas

Lemmas for BEq and List.all.
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
## Proofs

Main aggregation safety theorem.
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

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving FROST aggregation safety. -/
def frostOrchestratorClaims : FrostOrchestratorClaims where
  aggregate_same_session_round := aggregate_same_session_round

end Aura.Frost
