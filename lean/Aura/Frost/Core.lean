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
  deriving BEq, Repr, DecidableEq

/-- Commitment round -/
structure Round where
  idx : Nat
  deriving BEq, Repr, DecidableEq

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
    let sid := sh.sid
    let rnd := sh.round
    tl.all (fun sh' => sh'.sid == sid && sh'.round == rnd)

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

/-- Theorem: Successful aggregation implies all shares match -/
theorem aggregate_same_session_round
  (state : AggregatorState) (sig : Signature)
  (h : aggregate state = some sig) :
  ∃ sid rnd, ∀ sh ∈ state.pending, sh.sid = sid ∧ sh.round = rnd := by
  sorry  -- To be proven

end Aura.Frost
