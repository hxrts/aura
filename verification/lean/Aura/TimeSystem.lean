-- Core definitions for TimeStamp system verification.
-- Proves comparison reflexivity, transitivity, and privacy guarantees.

namespace Aura.TimeSystem

/-!
# Unified Time System

This module models the TimeStamp comparison logic and proves
transitivity, reflexivity, and privacy properties.

**Why this matters**: Aura uses multiple time domains (physical, logical, order)
for different purposes. Physical time can leak metadata (when you were online).
The privacy property proves that when `ignorePhysical = true`, the comparison
result depends ONLY on logical clocks, not on wall-clock time.
-/

-- Abstract timestamp with two components:
-- - logical: Lamport/vector clock for causal ordering (always safe to expose)
-- - orderClock: Physical time for tie-breaking (can leak activity patterns)
structure TimeStamp where
  logical : Nat      -- Causality-preserving clock
  orderClock : Nat   -- Physical time (privacy-sensitive)
  deriving BEq, Repr

-- Comparison policy determines which clock components to use.
-- Privacy-preserving mode ignores physical time entirely.
structure Policy where
  ignorePhysical : Bool  -- When true, only logical clock is used
  deriving BEq, Repr

-- Three-way comparison result (like Rust's std::cmp::Ordering).
inductive Ordering where
  | lt : Ordering  -- Less than
  | eq : Ordering  -- Equal
  | gt : Ordering  -- Greater than
  deriving BEq, Repr, DecidableEq

-- Standard three-way comparison for natural numbers.
def compareNat (a b : Nat) : Ordering :=
  if a < b then .lt
  else if a = b then .eq
  else .gt

-- Compare timestamps according to policy:
-- - If ignorePhysical: use only logical clock
-- - Otherwise: use logical clock, then orderClock as tiebreaker
def compare (policy : Policy) (a b : TimeStamp) : Ordering :=
  if policy.ignorePhysical then
    compareNat a.logical b.logical
  else
    match compareNat a.logical b.logical with
    | .lt => .lt
    | .gt => .gt
    | .eq => compareNat a.orderClock b.orderClock

-- **Lemma**: compareNat n n = eq (reflexivity).
-- Proof: n < n is false (irreflexivity), and n = n is true.
theorem compareNat_refl (n : Nat) : compareNat n n = .eq := by
  unfold compareNat
  simp [Nat.lt_irrefl]

-- **Lemma**: compareNat a b = lt iff a < b.
-- This connects the three-way comparison to the underlying order.
theorem compareNat_lt_iff (a b : Nat) : compareNat a b = .lt ↔ a < b := by
  unfold compareNat
  constructor
  · intro h
    split at h
    case isTrue hlt => exact hlt
    case isFalse => split at h <;> contradiction
  · intro hlt
    simp [hlt]

-- **Lemma**: compareNat is transitive for lt.
-- If a < b and b < c, then a < c.
theorem compareNat_trans_lt (a b c : Nat)
    (hab : compareNat a b = .lt) (hbc : compareNat b c = .lt) :
    compareNat a c = .lt := by
  rw [compareNat_lt_iff] at hab hbc ⊢
  exact Nat.lt_trans hab hbc

-- **Theorem: Reflexivity** - compare policy t t = eq for any policy and timestamp.
-- Proof: Both logical and orderClock compare equal to themselves.
theorem compare_refl (policy : Policy) (t : TimeStamp) :
    compare policy t t = .eq := by
  simp only [compare]
  split
  · exact compareNat_refl t.logical
  · simp only [compareNat_refl t.logical, compareNat_refl t.orderClock]

-- **Lemma**: Characterize when compareNat returns .lt.
theorem compareNat_eq_lt_iff (a b : Nat) : compareNat a b = .lt ↔ a < b := by
  unfold compareNat
  constructor
  · intro h; split at h; assumption; split at h <;> contradiction
  · intro h; simp [h]

-- **Lemma**: Characterize when compareNat returns .eq.
theorem compareNat_eq_eq_iff (a b : Nat) : compareNat a b = .eq ↔ a = b := by
  unfold compareNat
  constructor
  · intro h
    split at h
    case isTrue hlt => contradiction
    case isFalse hge =>
      split at h
      case isTrue heq => exact heq
      case isFalse => contradiction
  · intro h; simp [h, Nat.lt_irrefl]

-- **Theorem: Transitivity** - If a < b and b < c, then a < c.
-- Proof requires case analysis on logical clock comparison results.
-- When logical clocks are equal, physical clocks determine the order.
theorem compare_trans (policy : Policy) (a b c : TimeStamp) :
    compare policy a b = .lt →
    compare policy b c = .lt →
    compare policy a c = .lt := by
  intro hab hbc
  simp only [compare] at hab hbc ⊢
  split at hab <;> split at hbc <;> split
  -- Handle contradiction cases where policy doesn't match
  all_goals try simp_all
  -- Case: all ignorePhysical = true (privacy mode)
  next =>
    exact compareNat_trans_lt a.logical b.logical c.logical hab hbc
  -- Case: all ignorePhysical = false (full comparison)
  next =>
    -- Match on the logical clock comparison results
    generalize hcab : compareNat a.logical b.logical = cab at hab
    generalize hcbc : compareNat b.logical c.logical = cbc at hbc
    match cab, cbc with
    | .lt, .lt =>
      -- Logical clocks: a.logical < b.logical < c.logical
      have hlt_ab := (compareNat_eq_lt_iff a.logical b.logical).mp hcab
      have hlt_bc := (compareNat_eq_lt_iff b.logical c.logical).mp hcbc
      rw [(compareNat_eq_lt_iff a.logical c.logical).mpr (Nat.lt_trans hlt_ab hlt_bc)]
    | .lt, .eq =>
      -- a.logical < b.logical = c.logical, so a.logical < c.logical
      have hlt_ab := (compareNat_eq_lt_iff a.logical b.logical).mp hcab
      have heq_bc := (compareNat_eq_eq_iff b.logical c.logical).mp hcbc
      rw [(compareNat_eq_lt_iff a.logical c.logical).mpr (by omega : a.logical < c.logical)]
    | .lt, .gt => simp at hbc  -- b < c contradicts b.logical > c.logical
    | .eq, .lt =>
      -- a.logical = b.logical < c.logical, so a.logical < c.logical
      have heq_ab := (compareNat_eq_eq_iff a.logical b.logical).mp hcab
      have hlt_bc := (compareNat_eq_lt_iff b.logical c.logical).mp hcbc
      rw [(compareNat_eq_lt_iff a.logical c.logical).mpr (by omega : a.logical < c.logical)]
    | .eq, .eq =>
      -- Logical clocks all equal: a.logical = b.logical = c.logical
      -- Order determined by physical clocks
      have heq_ab := (compareNat_eq_eq_iff a.logical b.logical).mp hcab
      have heq_bc := (compareNat_eq_eq_iff b.logical c.logical).mp hcbc
      have heq_ac : a.logical = c.logical := by omega
      rw [(compareNat_eq_eq_iff a.logical c.logical).mpr heq_ac]
      -- Physical clocks: a.orderClock < b.orderClock < c.orderClock
      have hlt_oc_ab := (compareNat_eq_lt_iff a.orderClock b.orderClock).mp hab
      have hlt_oc_bc := (compareNat_eq_lt_iff b.orderClock c.orderClock).mp hbc
      exact (compareNat_eq_lt_iff a.orderClock c.orderClock).mpr (Nat.lt_trans hlt_oc_ab hlt_oc_bc)
    | .eq, .gt => simp at hbc  -- b < c contradicts b.logical > c.logical
    | .gt, _ => simp at hab    -- a < b contradicts a.logical > b.logical

/-!
## Privacy Theorem

When `ignorePhysical = true`, the comparison result depends ONLY on logical clocks.
Two timestamps with the same logical clock but different physical times will
compare identically to two timestamps with the same logical clock but ANY physical times.

This is the formal guarantee that privacy mode leaks no physical timing information.
-/
theorem physical_hidden
    (policy : Policy) (h : policy.ignorePhysical = true)
    (a1 a2 b1 b2 : TimeStamp)
    (h_log : a1.logical = a2.logical ∧ b1.logical = b2.logical) :
    compare policy a1 b1 = compare policy a2 b2 := by
  unfold compare
  -- With ignorePhysical = true, we only compare logical clocks
  simp only [h, ↓reduceIte, compareNat]
  obtain ⟨ha, hb⟩ := h_log
  -- Logical clocks match, so results match (orderClock is never accessed)
  simp only [ha, hb]

end Aura.TimeSystem
