-- Core definitions for Flow Budget verification

namespace Aura.FlowBudget

/-!
# Flow Budget Mathematics

This module models the flow budget system and proves correctness properties.
-/

/-- Flow budget state -/
structure Budget where
  available : Nat
  deriving BEq, Repr

/-- Charge a cost against a budget -/
def charge (budget : Budget) (cost : Nat) : Option Budget :=
  if budget.available >= cost then
    some { available := budget.available - cost }
  else
    none

/-- Theorem: Charging never increases available budget -/
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

/-- Theorem: Charging exact amount results in zero budget -/
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

end Aura.FlowBudget
