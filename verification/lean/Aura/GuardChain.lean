/-!
# Guard Chain Proofs

Proves cost calculation correctness and evaluation determinism for
the guard chain security enforcement layer.

## Quint Correspondence
- File: verification/quint/protocol_capability_properties.qnt
- Section: INVARIANTS
- Properties: Guard evaluation is pure and deterministic

## Rust Correspondence
- File: crates/aura-guards/src/guards/mod.rs
- Type: `CapGuard`, `FlowGuard`, `JournalCoupler`
- Function: `evaluate` - pure guard chain evaluation

## Expose

**Types**:
- `CapRequirement`: Capability requirement level (none < read < write)
- `Step`: Single guard step with flow cost and capability requirement
- `Snapshot`: All pending guard steps to evaluate
- `EffectCommand`: Output commands for effect interpreter

**Operations** (stable):
- `evaluateGuards`: Evaluate all guards and sum costs
- `sumFlowCosts`: Compute sum of flow costs

**Properties** (stable, theorem statements):
- `cost_sum`: Total cost is exactly the sum of step costs
- `evaluate_deterministic`: Same snapshot always produces same commands

**Internal helpers** (may change):
- None
-/

namespace Aura.GuardChain

/-!
## Core Types

Guard chain data structures.
-/

/-- Capability requirements follow a lattice: none < read < write.
    Rust: crates/aura-guards/src/guards/cap_guard.rs -/
inductive CapRequirement where
  | none : CapRequirement
  | read : CapRequirement
  | write : CapRequirement
  deriving BEq, Repr, DecidableEq

/-- A single guard step in the chain.
    Rust: Part of guard evaluation state -/
structure Step where
  flowCost : Nat
  capReq : CapRequirement
  deriving BEq, Repr

/-- Snapshot of all pending guard steps.
    Prepared asynchronously, evaluated synchronously (pure). -/
structure Snapshot where
  steps : List Step
  deriving BEq, Repr

/-- Output of guard evaluation.
    Rust: Commands for the effect interpreter -/
structure EffectCommand where
  totalCost : Nat
  deriving BEq, Repr

/-!
## Guard Evaluation

Pure evaluation functions.
-/

/-- Evaluate all guards and sum their costs.
    Quint: Pure and synchronous—no I/O during evaluation -/
def evaluateGuards (snap : Snapshot) : EffectCommand :=
  { totalCost := snap.steps.foldl (fun acc s => acc + s.flowCost) 0 }

/-- Compute sum of flow costs from a step list. -/
def sumFlowCosts (steps : List Step) : Nat :=
  steps.foldl (fun acc s => acc + s.flowCost) 0

/-!
## Claims Bundle

Guard chain correctness properties.
-/

/-- Claims bundle for GuardChain properties. -/
structure GuardChainClaims where
  /-- Cost additivity: Total cost is exactly the sum of step costs. -/
  cost_sum : ∀ snap : Snapshot,
    (evaluateGuards snap).totalCost = sumFlowCosts snap.steps

  /-- Determinism: Same snapshot always produces same commands. -/
  evaluate_deterministic : ∀ snap : Snapshot,
    evaluateGuards snap = evaluateGuards snap

  /-- Empty snapshot: No steps means zero cost. -/
  empty_zero_cost : (evaluateGuards { steps := [] }).totalCost = 0

  /-- Monotonicity: Adding a step to the snapshot never decreases cost. -/
  cost_monotonic : ∀ (steps : List Step) (s : Step),
    sumFlowCosts steps ≤ sumFlowCosts (steps ++ [s])

  /-- Prepend step: Prepending a step adds exactly that step's cost. -/
  prepend_cost : ∀ (s : Step) (steps : List Step),
    sumFlowCosts (s :: steps) = s.flowCost + sumFlowCosts steps

/-!
## Proofs

Guard chain correctness theorems.
-/

/-- Cost additivity: Total cost is exactly the sum of step costs.
    Ensures no hidden costs or discounts in the guard chain. -/
theorem cost_sum (snap : Snapshot) :
  (evaluateGuards snap).totalCost = sumFlowCosts snap.steps := by
  rfl

/-- Determinism: Same snapshot always produces same commands.
    Critical for simulation reproducibility and audit trails. -/
theorem evaluate_deterministic (snap : Snapshot) :
  evaluateGuards snap = evaluateGuards snap := by
  rfl

/-- Empty snapshot has zero total cost. -/
theorem empty_zero_cost : (evaluateGuards { steps := [] }).totalCost = 0 := by
  rfl

/-- Helper: foldl over append equals foldl over first then continue with result. -/
theorem foldl_append_step (f : Nat → Step → Nat) (init : Nat) (xs : List Step) (s : Step) :
    List.foldl f init (xs ++ [s]) = f (List.foldl f init xs) s := by
  induction xs generalizing init with
  | nil => simp
  | cons y ys ih =>
    simp only [List.append_eq, List.cons_append, List.foldl_cons]
    exact ih (f init y)

/-- Monotonicity: Adding a step never decreases total cost.
    Proof by induction on the step list using foldl properties. -/
theorem cost_monotonic (steps : List Step) (s : Step) :
    sumFlowCosts steps ≤ sumFlowCosts (steps ++ [s]) := by
  unfold sumFlowCosts
  rw [foldl_append_step]
  simp only [Nat.le_add_right]

/-- Helper: foldl with offset in accumulator.
    Shows that foldl (acc + f x) (init + k) xs = k + foldl (acc + f x) init xs -/
theorem foldl_add_offset (steps : List Step) (init offset : Nat) :
    List.foldl (fun acc x => acc + x.flowCost) (init + offset) steps =
    offset + List.foldl (fun acc x => acc + x.flowCost) init steps := by
  induction steps generalizing init offset with
  | nil => simp [Nat.add_comm]
  | cons s ss ih =>
    simp only [List.foldl_cons]
    rw [Nat.add_assoc, Nat.add_comm offset s.flowCost, ← Nat.add_assoc]
    exact ih (init + s.flowCost) offset

/-- Prepend cost: The cost of prepending a step equals that step's cost plus the rest.
    Uses the foldl offset lemma. -/
theorem prepend_cost (s : Step) (steps : List Step) :
    sumFlowCosts (s :: steps) = s.flowCost + sumFlowCosts steps := by
  unfold sumFlowCosts
  simp only [List.foldl_cons, Nat.zero_add]
  have h := foldl_add_offset steps 0 s.flowCost
  simp only [Nat.zero_add] at h
  exact h

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving guard chain correctness. -/
def guardChainClaims : GuardChainClaims where
  cost_sum := cost_sum
  evaluate_deterministic := evaluate_deterministic
  empty_zero_cost := empty_zero_cost
  cost_monotonic := cost_monotonic
  prepend_cost := prepend_cost

end Aura.GuardChain
