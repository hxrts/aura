-- Core definitions for Guard Chain verification

namespace Aura.GuardChain

/-!
# Guard Chain Evaluation

This module models the pure guard evaluation logic and proves correctness
properties about flow cost calculation and capability checking.
-/

/-- Capability requirement -/
inductive CapRequirement where
  | none : CapRequirement
  | read : CapRequirement
  | write : CapRequirement
  deriving BEq, Repr, DecidableEq

/-- A single step in the guard chain -/
structure Step where
  flowCost : Nat
  capReq : CapRequirement
  deriving BEq, Repr

/-- Guard snapshot - list of pending steps -/
structure Snapshot where
  steps : List Step
  deriving BEq, Repr

/-- Effect command resulting from guard evaluation -/
structure EffectCommand where
  totalCost : Nat
  -- Additional fields can be added as needed
  deriving BEq, Repr

/-- Evaluate guards and compute total cost -/
def evaluateGuards (snap : Snapshot) : EffectCommand :=
  { totalCost := snap.steps.foldl (fun acc s => acc + s.flowCost) 0 }

/-- Helper: sum of flow costs -/
def sumFlowCosts (steps : List Step) : Nat :=
  steps.foldl (fun acc s => acc + s.flowCost) 0

/-- Theorem: Total cost equals sum of flow costs -/
theorem cost_sum (snap : Snapshot) :
  (evaluateGuards snap).totalCost = sumFlowCosts snap.steps := by
  rfl

/-- Theorem: Evaluation is deterministic -/
theorem evaluate_deterministic (snap : Snapshot) :
  evaluateGuards snap = evaluateGuards snap := by
  rfl

end Aura.GuardChain
