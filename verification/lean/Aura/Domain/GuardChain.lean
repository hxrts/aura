/-!
# Guard Chain Types and Operations

Types and evaluation functions for the guard chain security layer.

## Quint Correspondence
- File: verification/quint/protocol_capability_properties.qnt
- Section: TYPES, OPERATIONS

## Rust Correspondence
- File: crates/aura-guards/src/guards/mod.rs
- Type: `CapGuard`, `FlowGuard`, `JournalCoupler`

## Expose

**Types** (stable):
- `CapRequirement`: Capability requirement level (none < read < write)
- `Step`: Single guard step with flow cost and capability requirement
- `Snapshot`: All pending guard steps to evaluate
- `EffectCommand`: Output commands for effect interpreter

**Operations** (stable):
- `evaluateGuards`: Evaluate all guards and sum costs
- `sumFlowCosts`: Compute sum of flow costs
-/

namespace Aura.Domain.GuardChain

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

end Aura.Domain.GuardChain
