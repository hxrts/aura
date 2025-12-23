/-!
# Unified Time System Proofs

Proves comparison reflexivity, transitivity, and privacy guarantees for
the multi-domain timestamp system.

## Quint Correspondence
- File: verification/quint/protocol_core.qnt
- Section: TYPE DEFINITIONS
- Properties: Timestamp ordering is transitive and reflexive

## Rust Correspondence
- File: crates/aura-core/src/time.rs
- Type: `TimeStamp`, `Policy`
- Function: `compare` - policy-aware timestamp comparison

## Expose

**Types**:
- `TimeStamp`: Abstract timestamp with logical and order clock components
- `Policy`: Comparison policy (ignorePhysical for privacy)
- `Ordering`: Three-way comparison result (lt, eq, gt)

**Operations** (stable):
- `compare`: Compare timestamps according to policy
- `compareNat`: Three-way comparison for natural numbers

**Properties** (stable, theorem statements):
- `compare_refl`: Comparison is reflexive
- `compare_trans`: Comparison is transitive for lt
- `physical_hidden`: Privacy mode hides physical time

**Internal helpers** (may change):
- `compareNat_*` lemmas
-/

namespace Aura.TimeSystem

/-!
## Core Types

Timestamp and policy types.
-/

/-- Abstract timestamp with two components.
    Rust: aura-core/src/time.rs::TimeStamp -/
structure TimeStamp where
  logical : Nat
  orderClock : Nat
  deriving BEq, Repr

/-- Comparison policy determines which clock components to use.
    Rust: aura-core/src/time.rs::ComparePolicy -/
structure Policy where
  ignorePhysical : Bool
  deriving BEq, Repr

/-- Three-way comparison result.
    Rust: std::cmp::Ordering -/
inductive Ordering where
  | lt : Ordering
  | eq : Ordering
  | gt : Ordering
  deriving BEq, Repr, DecidableEq

/-!
## Comparison Functions

Policy-aware timestamp comparison.
-/

/-- Standard three-way comparison for natural numbers. -/
def compareNat (a b : Nat) : Ordering :=
  if a < b then .lt
  else if a = b then .eq
  else .gt

/-- Compare timestamps according to policy.
    Quint: If ignorePhysical, only logical clock is used -/
def compare (policy : Policy) (a b : TimeStamp) : Ordering :=
  if policy.ignorePhysical then
    compareNat a.logical b.logical
  else
    match compareNat a.logical b.logical with
    | .lt => .lt
    | .gt => .gt
    | .eq => compareNat a.orderClock b.orderClock

/-!
## Claims Bundle

Timestamp comparison properties.
-/

/-- Claims bundle for TimeSystem properties. -/
structure TimeSystemClaims where
  /-- Reflexivity: compare policy t t = eq for any policy and timestamp. -/
  compare_refl : ∀ (policy : Policy) (t : TimeStamp),
    compare policy t t = .eq

  /-- Transitivity: If a < b and b < c, then a < c. -/
  compare_trans : ∀ (policy : Policy) (a b c : TimeStamp),
    compare policy a b = .lt →
    compare policy b c = .lt →
    compare policy a c = .lt

  /-- Privacy: When ignorePhysical = true, result depends only on logical clocks. -/
  physical_hidden : ∀ (policy : Policy),
    policy.ignorePhysical = true →
    ∀ (a1 a2 b1 b2 : TimeStamp),
      a1.logical = a2.logical ∧ b1.logical = b2.logical →
      compare policy a1 b1 = compare policy a2 b2

/-!
## Helper Lemmas

Lemmas about compareNat.
-/

/-- compareNat n n = eq (reflexivity). -/
theorem compareNat_refl (n : Nat) : compareNat n n = .eq := by
  unfold compareNat
  simp [Nat.lt_irrefl]

/-- compareNat a b = lt iff a < b. -/
theorem compareNat_lt_iff (a b : Nat) : compareNat a b = .lt ↔ a < b := by
  unfold compareNat
  constructor
  · intro h
    split at h
    case isTrue hlt => exact hlt
    case isFalse => split at h <;> contradiction
  · intro hlt
    simp [hlt]

/-- compareNat is transitive for lt. -/
theorem compareNat_trans_lt (a b c : Nat)
    (hab : compareNat a b = .lt) (hbc : compareNat b c = .lt) :
    compareNat a c = .lt := by
  rw [compareNat_lt_iff] at hab hbc ⊢
  exact Nat.lt_trans hab hbc

/-- compareNat a b = eq iff a = b. -/
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

/-- Alias for consistency. -/
theorem compareNat_eq_lt_iff (a b : Nat) : compareNat a b = .lt ↔ a < b :=
  compareNat_lt_iff a b

/-!
## Proofs

Main timestamp comparison theorems.
-/

/-- Reflexivity: compare policy t t = eq for any policy and timestamp. -/
theorem compare_refl (policy : Policy) (t : TimeStamp) :
    compare policy t t = .eq := by
  simp only [compare]
  split
  · exact compareNat_refl t.logical
  · simp only [compareNat_refl t.logical, compareNat_refl t.orderClock]

/-- Transitivity: If a < b and b < c, then a < c.
    Proof requires case analysis on logical clock comparison results. -/
theorem compare_trans (policy : Policy) (a b c : TimeStamp) :
    compare policy a b = .lt →
    compare policy b c = .lt →
    compare policy a c = .lt := by
  intro hab hbc
  simp only [compare] at hab hbc ⊢
  split at hab <;> split at hbc <;> split
  all_goals try simp_all
  next =>
    exact compareNat_trans_lt a.logical b.logical c.logical hab hbc
  next =>
    generalize hcab : compareNat a.logical b.logical = cab at hab
    generalize hcbc : compareNat b.logical c.logical = cbc at hbc
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
      have hlt_oc_ab := (compareNat_eq_lt_iff a.orderClock b.orderClock).mp hab
      have hlt_oc_bc := (compareNat_eq_lt_iff b.orderClock c.orderClock).mp hbc
      exact (compareNat_eq_lt_iff a.orderClock c.orderClock).mpr (Nat.lt_trans hlt_oc_ab hlt_oc_bc)
    | .eq, .gt => simp at hbc
    | .gt, _ => simp at hab

/-- Privacy: When ignorePhysical = true, result depends only on logical clocks.
    This is the formal guarantee that privacy mode leaks no physical timing. -/
theorem physical_hidden
    (policy : Policy) (h : policy.ignorePhysical = true)
    (a1 a2 b1 b2 : TimeStamp)
    (h_log : a1.logical = a2.logical ∧ b1.logical = b2.logical) :
    compare policy a1 b1 = compare policy a2 b2 := by
  unfold compare
  simp only [h, ↓reduceIte, compareNat]
  obtain ⟨ha, hb⟩ := h_log
  simp only [ha, hb]

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving timestamp comparison correctness. -/
def timeSystemClaims : TimeSystemClaims where
  compare_refl := compare_refl
  compare_trans := compare_trans
  physical_hidden := physical_hidden

end Aura.TimeSystem
