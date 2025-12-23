/-!
# Flow Budget Proofs

Proves charging monotonicity and exactness for the flow budget rate limiting
system that enforces charge-before-send.

## Quint Correspondence
- File: verification/quint/protocol_capability_properties.qnt
- Section: INVARIANTS
- Properties: Budget charging is monotonically decreasing

## Rust Correspondence
- File: crates/aura-core/src/domain/flow_budget.rs
- Type: `FlowBudget`
- Function: `charge` - deduct cost from available budget

## Expose

**Types**:
- `Budget`: Available units remaining for rate limiting

**Operations** (stable):
- `charge`: Deduct cost from budget, returning None if insufficient

**Properties** (stable, theorem statements):
- `charge_decreases`: Charging never increases the budget
- `charge_exact`: Charging exact remaining amount yields zero

**Internal helpers** (may change):
- None
-/

namespace Aura.FlowBudget

/-!
## Core Types

Budget state for rate limiting.
-/

/-- Budget state: tracks available units remaining.
    Rust: FlowBudget { limit, spent } where available = limit - spent -/
structure Budget where
  available : Nat
  deriving BEq, Repr

/-!
## Charge Operation

Deduct cost from budget with underflow protection.
-/

/-- Charge a cost against a budget. Returns None if insufficient.
    Quint: Models FlowGuard check: if spent + cost > limit, block -/
def charge (budget : Budget) (cost : Nat) : Option Budget :=
  if budget.available >= cost then
    some { available := budget.available - cost }
  else
    none

/-!
## Claims Bundle

Budget charging properties.
-/

/-- Claims bundle for FlowBudget properties. -/
structure FlowBudgetClaims where
  /-- Monotonic decrease: Charging never increases the budget. -/
  charge_decreases : ∀ (budget : Budget) (cost : Nat) (result : Budget),
    charge budget cost = some result →
    result.available ≤ budget.available

  /-- Exact exhaustion: Charging exact remaining amount yields zero. -/
  charge_exact : ∀ (budget : Budget) (result : Budget),
    charge budget budget.available = some result →
    result.available = 0

  /-- Zero cost: Charging zero always succeeds with unchanged budget. -/
  charge_zero : ∀ (budget : Budget),
    charge budget 0 = some budget

  /-- Insufficient funds: If charge fails, cost exceeds available. -/
  charge_insufficient : ∀ (budget : Budget) (cost : Nat),
    charge budget cost = none →
    cost > budget.available

  /-- Additivity: Sequential charges equal single combined charge (when both succeed). -/
  charge_additive : ∀ (budget r1 r2 : Budget) (c1 c2 : Nat),
    charge budget c1 = some r1 →
    charge r1 c2 = some r2 →
    charge budget (c1 + c2) = some r2

/-!
## Proofs

Budget charging theorems.
-/

/-- Monotonic decrease: Charging never increases the budget.
    After a successful charge, result.available ≤ budget.available. -/
theorem charge_decreases (budget : Budget) (cost : Nat) (result : Budget) :
    charge budget cost = some result →
    result.available ≤ budget.available := by
  intro h
  unfold charge at h
  split at h
  case isTrue hge =>
    cases h
    exact Nat.sub_le budget.available cost
  case isFalse =>
    contradiction

/-- Exact exhaustion: Charging the exact remaining amount yields zero.
    If we charge budget.available, we get exactly 0 remaining. -/
theorem charge_exact (budget : Budget) (result : Budget) :
    charge budget budget.available = some result →
    result.available = 0 := by
  intro h
  unfold charge at h
  split at h
  case isTrue =>
    cases h
    exact Nat.sub_self budget.available
  case isFalse hlt =>
    exact absurd (Nat.le_refl budget.available) hlt

/-- Zero cost: Charging zero always succeeds with unchanged budget.
    This is the identity element for the charge operation. -/
theorem charge_zero (budget : Budget) : charge budget 0 = some budget := by
  unfold charge
  simp only [Nat.zero_le, ↓reduceIte, Nat.sub_zero]

/-- Insufficient funds: If charge fails, cost exceeds available.
    The negation gives us: if cost ≤ available, charge succeeds. -/
theorem charge_insufficient (budget : Budget) (cost : Nat) :
    charge budget cost = none →
    cost > budget.available := by
  intro h
  unfold charge at h
  split at h
  case isTrue hge => contradiction
  case isFalse hlt => exact Nat.lt_of_not_ge hlt

/-- Additivity: Sequential charges equal single combined charge.
    Proof uses subtraction arithmetic to show equivalence. -/
theorem charge_additive (budget r1 r2 : Budget) (c1 c2 : Nat) :
    charge budget c1 = some r1 →
    charge r1 c2 = some r2 →
    charge budget (c1 + c2) = some r2 := by
  intro h1 h2
  unfold charge at h1 h2 ⊢
  split at h1
  case isTrue hge1 =>
    cases h1
    split at h2
    case isTrue hge2 =>
      cases h2
      -- Need to show: budget.available >= c1 + c2 and result matches
      -- hge1: budget.available >= c1
      -- hge2: budget.available - c1 >= c2
      -- Use: c1 + c2 <= budget.available follows from these
      have hge_combined : budget.available >= c1 + c2 := by
        have h := Nat.add_le_of_le_sub hge1 hge2
        rw [Nat.add_comm] at h
        exact h
      simp only [hge_combined, ↓reduceIte]
      congr 1
      -- Need: budget.available - (c1 + c2) = budget.available - c1 - c2
      rw [Nat.sub_add_eq]
    case isFalse hlt2 => contradiction
  case isFalse hlt1 => contradiction

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving flow budget correctness. -/
def flowBudgetClaims : FlowBudgetClaims where
  charge_decreases := charge_decreases
  charge_exact := charge_exact
  charge_zero := charge_zero
  charge_insufficient := charge_insufficient
  charge_additive := charge_additive

end Aura.FlowBudget
