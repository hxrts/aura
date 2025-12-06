-- Core definitions for Flow Budget verification.
-- Proves charging monotonicity and exactness for rate limiting.

namespace Aura.FlowBudget

/-!
# Flow Budget Mathematics

This module models the flow budget system and proves correctness properties.

**Why this matters**: Flow budgets prevent spam and DoS attacks by limiting
how many messages an authority can send. The "charge-before-send" invariant
requires FlowGuard to deduct cost BEFORE any network I/O occurs. These proofs
ensure the budget accounting is correct and cannot go negative.
-/

-- Budget state: tracks available units remaining.
-- In Rust, this is `FlowBudget { limit, spent }` where available = limit - spent.
structure Budget where
  available : Nat
  deriving BEq, Repr

-- Charge a cost against a budget. Returns None if insufficient funds.
-- This models the FlowGuard check: if spent + cost > limit, block the send.
def charge (budget : Budget) (cost : Nat) : Option Budget :=
  if budget.available >= cost then
    some { available := budget.available - cost }
  else
    none

-- **Theorem: Monotonic Decrease** - Charging never increases the budget.
-- After a successful charge, result.available ≤ budget.available.
-- Proof: When available >= cost, we compute available - cost, which is ≤ available.
theorem charge_decreases (budget : Budget) (cost : Nat) (result : Budget) :
    charge budget cost = some result →
    result.available ≤ budget.available := by
  intro h
  -- Expand the definition of charge and case-split on the if condition
  unfold charge at h
  split at h
  case isTrue hge =>
    -- When available >= cost, result = {available - cost}
    -- Nat.sub_le proves (n - m) ≤ n for any n, m
    cases h
    exact Nat.sub_le budget.available cost
  case isFalse =>
    -- When available < cost, charge returns none, contradicting h
    contradiction

-- **Theorem: Exact Exhaustion** - Charging the exact remaining amount yields zero.
-- If we charge budget.available, we get exactly 0 remaining.
-- Proof: n - n = 0 by Nat.sub_self.
theorem charge_exact (budget : Budget) (result : Budget) :
    charge budget budget.available = some result →
    result.available = 0 := by
  intro h
  unfold charge at h
  split at h
  case isTrue =>
    -- available >= available is always true, so we get {available - available}
    cases h
    exact Nat.sub_self budget.available
  case isFalse hlt =>
    -- This case is impossible: we're checking available >= available
    -- which is always true (reflexivity), contradicting hlt
    exact absurd (Nat.le_refl budget.available) hlt

end Aura.FlowBudget
