-- Core definitions for TimeStamp system verification

namespace Aura.TimeSystem

/-!
# Unified Time System

This module models the TimeStamp comparison logic and proves
transitivity, reflexivity, and privacy properties.
-/

/-- Abstract timestamp -/
structure TimeStamp where
  logical : Nat
  orderClock : Nat  -- Physical time, should not leak
  deriving BEq, Repr

/-- Comparison policy -/
structure Policy where
  ignorePhysical : Bool
  deriving BEq, Repr

/-- Ordering result -/
inductive Ordering where
  | lt : Ordering
  | eq : Ordering
  | gt : Ordering
  deriving BEq, Repr, DecidableEq

/-- Compare two natural numbers -/
def compareNat (a b : Nat) : Ordering :=
  if a < b then .lt
  else if a = b then .eq
  else .gt

/-- Compare two timestamps under a policy -/
def compare (policy : Policy) (a b : TimeStamp) : Ordering :=
  if policy.ignorePhysical then
    compareNat a.logical b.logical
  else
    match compareNat a.logical b.logical with
    | .lt => .lt
    | .gt => .gt
    | .eq => compareNat a.orderClock b.orderClock

/-- Helper: compareNat is reflexive -/
theorem compareNat_refl (n : Nat) : compareNat n n = .eq := by
  unfold compareNat
  simp [Nat.lt_irrefl]

/-- Helper: compareNat lt means first arg is less -/
theorem compareNat_lt_iff (a b : Nat) : compareNat a b = .lt ↔ a < b := by
  unfold compareNat
  constructor
  · intro h
    split at h
    case isTrue hlt => exact hlt
    case isFalse => split at h <;> contradiction
  · intro hlt
    simp [hlt]

/-- Helper: compareNat is transitive for lt -/
theorem compareNat_trans_lt (a b c : Nat)
    (hab : compareNat a b = .lt) (hbc : compareNat b c = .lt) :
    compareNat a c = .lt := by
  rw [compareNat_lt_iff] at hab hbc ⊢
  exact Nat.lt_trans hab hbc

/-- Theorem: Comparison is reflexive -/
theorem compare_refl (policy : Policy) (t : TimeStamp) :
    compare policy t t = .eq := by
  simp only [compare]
  split
  · exact compareNat_refl t.logical
  · simp only [compareNat_refl t.logical, compareNat_refl t.orderClock]

/-- Helper: compareNat produces .lt exactly when a < b -/
theorem compareNat_eq_lt_iff (a b : Nat) : compareNat a b = .lt ↔ a < b := by
  unfold compareNat
  constructor
  · intro h; split at h; assumption; split at h <;> contradiction
  · intro h; simp [h]

/-- Helper: compareNat produces .eq exactly when a = b -/
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

/-- Theorem: Comparison is transitive for lt -/
theorem compare_trans (policy : Policy) (a b c : TimeStamp) :
    compare policy a b = .lt →
    compare policy b c = .lt →
    compare policy a c = .lt := by
  intro hab hbc
  simp only [compare] at hab hbc ⊢
  split at hab <;> split at hbc <;> split
  -- Handle contradiction cases where policy doesn't match
  all_goals try simp_all
  -- Case: all ignorePhysical = true
  next =>
    exact compareNat_trans_lt a.logical b.logical c.logical hab hbc
  -- Case: all ignorePhysical = false
  next =>
    -- Extract info from the match
    generalize hcab : compareNat a.logical b.logical = cab at hab
    generalize hcbc : compareNat b.logical c.logical = cbc at hbc
    -- Case analysis on comparison results
    match cab, cbc with
    | .lt, .lt =>
      have hlt_ab := (compareNat_eq_lt_iff a.logical b.logical).mp hcab
      have hlt_bc := (compareNat_eq_lt_iff b.logical c.logical).mp hcbc
      rw [(compareNat_eq_lt_iff a.logical c.logical).mpr (Nat.lt_trans hlt_ab hlt_bc)]
    | .lt, .eq =>
      have hlt_ab := (compareNat_eq_lt_iff a.logical b.logical).mp hcab
      have heq_bc := (compareNat_eq_eq_iff b.logical c.logical).mp hcbc
      rw [(compareNat_eq_lt_iff a.logical c.logical).mpr (by omega : a.logical < c.logical)]
    | .lt, .gt => simp at hbc
    | .eq, .lt =>
      have heq_ab := (compareNat_eq_eq_iff a.logical b.logical).mp hcab
      have hlt_bc := (compareNat_eq_lt_iff b.logical c.logical).mp hcbc
      rw [(compareNat_eq_lt_iff a.logical c.logical).mpr (by omega : a.logical < c.logical)]
    | .eq, .eq =>
      have heq_ab := (compareNat_eq_eq_iff a.logical b.logical).mp hcab
      have heq_bc := (compareNat_eq_eq_iff b.logical c.logical).mp hcbc
      have heq_ac : a.logical = c.logical := by omega
      rw [(compareNat_eq_eq_iff a.logical c.logical).mpr heq_ac]
      -- Order clocks determine the result
      have hlt_oc_ab := (compareNat_eq_lt_iff a.orderClock b.orderClock).mp hab
      have hlt_oc_bc := (compareNat_eq_lt_iff b.orderClock c.orderClock).mp hbc
      exact (compareNat_eq_lt_iff a.orderClock c.orderClock).mpr (Nat.lt_trans hlt_oc_ab hlt_oc_bc)
    | .eq, .gt => simp at hbc
    | .gt, _ => simp at hab

/-- Theorem: Physical time is hidden when ignorePhysical is true -/
theorem physical_hidden
    (policy : Policy) (h : policy.ignorePhysical = true)
    (a1 a2 b1 b2 : TimeStamp)
    (h_log : a1.logical = a2.logical ∧ b1.logical = b2.logical) :
    compare policy a1 b1 = compare policy a2 b2 := by
  unfold compare
  simp only [h, ↓reduceIte, compareNat]
  obtain ⟨ha, hb⟩ := h_log
  simp only [ha, hb]

end Aura.TimeSystem
