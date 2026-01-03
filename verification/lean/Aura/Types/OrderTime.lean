import Lean.Data.Json
import Aura.Types.ByteArray32

/-! # Aura.Types.OrderTime

Opaque 32-byte total ordering key for deterministic fact ordering.

## Rust Correspondence
- File: crates/aura-core/src/time/mod.rs
- Type: `OrderTime([u8; 32])`
- Trait: Derived `Ord` uses lexicographic comparison

## Quint Correspondence
- File: verification/quint/protocol_journal.qnt
- Property: Facts ordered by opaque OrderTime, not semantic timestamp

## Expose

**Types** (stable):
- `OrderTime`: Opaque 32-byte ordering key

**Operations** (stable):
- `compare`: Lexicographic comparison returning `Ordering`
- `Ord` instance for use in sorted collections

**Properties** (theorem statements):
- `compare_refl`: compare a a = .eq
- `compare_antisym`: compare a b = .eq → a = b
- `compare_trans_lt`: Transitivity for less-than
-/

namespace Aura.Types.OrderTime

open Lean (Json ToJson FromJson)
open Aura.Types.ByteArray32 (ByteArray32)

/-! ## Core Type -/

/-- Opaque total-order key for deterministic fact merges.
    The value has no semantic meaning (unlike TimeStamp).
    Rust: aura-core/src/time/mod.rs::OrderTime([u8; 32]) -/
structure OrderTime where
  /-- The underlying 32-byte value. -/
  value : ByteArray32
  deriving Repr

/-! ## Equality -/

instance : BEq OrderTime where
  beq a b := a.value == b.value

instance : DecidableEq OrderTime := fun a b =>
  if h : a.value = b.value then
    isTrue (by cases a; cases b; simp_all)
  else
    isFalse (by intro heq; cases heq; exact h rfl)

/-! ## Comparison -/

/-- Lexicographic comparison for OrderTime.
    Delegates to ByteArray32.compare. -/
def compare (a b : OrderTime) : Ordering :=
  Aura.Types.ByteArray32.compare a.value b.value

/-- Ord instance for OrderTime using lexicographic comparison. -/
instance : Ord OrderTime where
  compare := compare

/-! ## Claims Bundle -/

/-- Claims for OrderTime ordering properties.
    These establish that compare forms a total order. -/
structure OrderTimeClaims where
  /-- Comparison is reflexive: compare a a = .eq -/
  compare_refl : ∀ a : OrderTime, compare a a = .eq
  /-- Antisymmetry: equal comparison implies value equality -/
  compare_antisym : ∀ a b : OrderTime, compare a b = .eq → a = b
  /-- Transitivity for less-than -/
  compare_trans_lt : ∀ a b c : OrderTime,
    compare a b = .lt → compare b c = .lt → compare a c = .lt
  /-- Transitivity for greater-than -/
  compare_trans_gt : ∀ a b c : OrderTime,
    compare a b = .gt → compare b c = .gt → compare a c = .gt

/-! ## Proofs -/

/-- Comparison is reflexive. -/
theorem compare_refl (a : OrderTime) : compare a a = .eq := by
  unfold compare
  exact Aura.Types.ByteArray32.compare_refl a.value

/-- Antisymmetry: equal comparison implies value equality. -/
theorem compare_antisym (a b : OrderTime) (h : compare a b = .eq) : a = b := by
  unfold compare at h
  have heq := Aura.Types.ByteArray32.compare_antisym a.value b.value h
  cases a; cases b
  simp_all

/-- Transitivity for less-than.
    TODO: Complete by delegating to ByteArray32 proof. -/
theorem compare_trans_lt (a b c : OrderTime)
    (hab : compare a b = .lt) (hbc : compare b c = .lt) :
    compare a c = .lt := by
  unfold compare at hab hbc ⊢
  exact Aura.Types.ByteArray32.compare_trans_lt a.value b.value c.value hab hbc

/-- Transitivity for greater-than.
    TODO: Complete by delegating to ByteArray32 proof. -/
theorem compare_trans_gt (a b c : OrderTime)
    (hab : compare a b = .gt) (hbc : compare b c = .gt) :
    compare a c = .gt := by
  unfold compare at hab hbc ⊢
  exact Aura.Types.ByteArray32.compare_trans_gt a.value b.value c.value hab hbc

/-- Construct the claims bundle. -/
def orderTimeClaims : OrderTimeClaims where
  compare_refl := compare_refl
  compare_antisym := compare_antisym
  compare_trans_lt := compare_trans_lt
  compare_trans_gt := compare_trans_gt

/-! ## JSON Serialization -/

/-- JSON serialization: encode as 64-character hex string. -/
instance : ToJson OrderTime where
  toJson o := ToJson.toJson o.value

/-- JSON deserialization: parse 64-character hex string. -/
instance : FromJson OrderTime where
  fromJson? j := do
    let bytes ← FromJson.fromJson? j
    pure ⟨bytes⟩

/-! ## Utilities -/

/-- Create an OrderTime filled with zeros (minimal value). -/
def zero : OrderTime := ⟨ByteArray32.zero⟩

/-- Create an OrderTime from a single byte repeated (for testing). -/
def replicate (b : UInt8) : OrderTime := ⟨ByteArray32.replicate b⟩

end Aura.Types.OrderTime
