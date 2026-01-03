import Lean.Data.Json

/-! # Aura.Types.ByteArray32

Fixed 32-byte arrays with lexicographic comparison for cryptographic identifiers.

## Rust Correspondence
- File: crates/aura-core/src/identifiers.rs
- Types: AuthorityId, ContextId, ChannelId, Hash32 all wrap [u8; 32]
- Comparison: Lexicographic via derived Ord

## Expose

**Types** (stable):
- `ByteArray32`: Fixed 32-byte array with length proof

**Operations** (stable):
- `compare`: Lexicographic comparison returning `Ordering`
- `beq`: Byte-by-byte equality

**Properties** (theorem statements):
- `compare_refl`: compare a a = .eq
- `compare_antisym`: compare a b = .eq → a = b
- `compare_trans_lt`: compare a b = .lt → compare b c = .lt → compare a c = .lt
-/

namespace Aura.Types.ByteArray32

open Lean (Json ToJson FromJson)

/-! ## Core Types -/

/-- Fixed 32-byte array for cryptographic identifiers.
    Uses List UInt8 with a proof of length = 32.
    Rust: [u8; 32] used throughout aura-core identifiers. -/
structure ByteArray32 where
  /-- The underlying byte list. -/
  bytes : List UInt8
  /-- Proof that the list has exactly 32 elements. -/
  len_eq : bytes.length = 32
  deriving Repr

/-! ## Equality -/

/-- Byte-by-byte equality for ByteArray32. -/
instance : BEq ByteArray32 where
  beq a b := a.bytes == b.bytes

/-- Decidable equality derived from list equality. -/
instance : DecidableEq ByteArray32 := fun a b =>
  if h : a.bytes = b.bytes then
    isTrue (by cases a; cases b; simp_all)
  else
    isFalse (by intro heq; cases heq; exact h rfl)

/-! ## Comparison -/

/-- Lexicographic comparison of byte lists.
    Returns .lt if first list is lexicographically smaller,
    .eq if equal, .gt if greater. -/
def compareBytes : List UInt8 → List UInt8 → Ordering
  | [], [] => .eq
  | [], _ :: _ => .lt
  | _ :: _, [] => .gt
  | x :: xs, y :: ys =>
    match Ord.compare x y with
    | .eq => compareBytes xs ys
    | ord => ord

/-- Lexicographic comparison for ByteArray32.
    Delegates to compareBytes on the underlying byte lists. -/
def compare (a b : ByteArray32) : Ordering :=
  compareBytes a.bytes b.bytes

/-- Ord instance for ByteArray32 using lexicographic comparison. -/
instance : Ord ByteArray32 where
  compare := compare

/-! ## Claims Bundle -/

/-- Claims for ByteArray32 comparison properties.
    These establish that compare forms a total order. -/
structure ByteArray32Claims where
  /-- Comparison is reflexive: compare a a = .eq -/
  compare_refl : ∀ a : ByteArray32, compare a a = .eq
  /-- Antisymmetry: equal comparison implies value equality -/
  compare_antisym : ∀ a b : ByteArray32, compare a b = .eq → a = b
  /-- Transitivity for less-than -/
  compare_trans_lt : ∀ a b c : ByteArray32,
    compare a b = .lt → compare b c = .lt → compare a c = .lt
  /-- Transitivity for greater-than -/
  compare_trans_gt : ∀ a b c : ByteArray32,
    compare a b = .gt → compare b c = .gt → compare a c = .gt

/-! ## Proofs -/

/-- Helper: x < x is false for UInt8. -/
private theorem uint8_lt_irrefl (x : UInt8) : ¬(x < x) := by
  intro h
  exact Nat.lt_irrefl x.toNat h

/-- Helper: compareBytes is reflexive on any list. -/
theorem compareBytes_refl : ∀ xs : List UInt8, compareBytes xs xs = .eq := by
  intro xs
  induction xs with
  | nil => rfl
  | cons x xs ih =>
    simp only [compareBytes]
    -- Ord.compare x x = .eq for UInt8
    have hxx : Ord.compare x x = .eq := by
      simp only [Ord.compare, compareOfLessAndEq]
      -- x < x is false, so the if evaluates to else branch which is .eq
      simp only [uint8_lt_irrefl x, ↓reduceIte]
    simp only [hxx, ih]

/-- Comparison is reflexive. -/
theorem compare_refl (a : ByteArray32) : compare a a = .eq := by
  unfold compare
  exact compareBytes_refl a.bytes

/-- Helper: compareBytes equal implies lists equal.
    TODO: Complete inductive proof over list structure. -/
theorem compareBytes_eq_implies_eq : ∀ xs ys : List UInt8,
    compareBytes xs ys = .eq → xs = ys := by
  intro xs ys h
  induction xs generalizing ys with
  | nil =>
    cases ys with
    | nil => rfl
    | cons _ _ => simp [compareBytes] at h
  | cons x xs ih =>
    cases ys with
    | nil => simp [compareBytes] at h
    | cons y ys =>
      simp only [compareBytes] at h
      split at h
      · rename_i heq
        -- heq : Ord.compare x y = .eq, h : compareBytes xs ys = .eq
        have hxy : x = y := by
          -- Ord.compare x y = .eq implies x = y for UInt8
          sorry -- TODO: prove from Ord.compare semantics
        rw [hxy]
        congr 1
        exact ih ys h
      · -- Ord.compare x y ≠ .eq means result is .lt or .gt, not .eq
        rename_i hneq
        cases hneq' : Ord.compare x y <;> simp_all

/-- Antisymmetry: equal comparison implies value equality. -/
theorem compare_antisym (a b : ByteArray32) (h : compare a b = .eq) : a = b := by
  unfold compare at h
  have heq := compareBytes_eq_implies_eq a.bytes b.bytes h
  cases a; cases b
  simp_all

/-- Transitivity for less-than.
    TODO: Complete inductive proof. -/
theorem compare_trans_lt (a b c : ByteArray32)
    (hab : compare a b = .lt) (hbc : compare b c = .lt) :
    compare a c = .lt := by
  unfold compare at hab hbc ⊢
  sorry -- TODO: prove by induction on compareBytes

/-- Transitivity for greater-than.
    TODO: Complete by symmetry with lt case. -/
theorem compare_trans_gt (a b c : ByteArray32)
    (hab : compare a b = .gt) (hbc : compare b c = .gt) :
    compare a c = .gt := by
  sorry -- TODO: prove via symmetry with lt case

/-- Construct the claims bundle. -/
def byteArray32Claims : ByteArray32Claims where
  compare_refl := compare_refl
  compare_antisym := compare_antisym
  compare_trans_lt := compare_trans_lt
  compare_trans_gt := compare_trans_gt

/-! ## JSON Serialization -/

/-- Convert a nibble (0-15) to its hex character. -/
private def nibbleToHex (n : UInt8) : Char :=
  if n < 10 then Char.ofNat (n.toNat + 48)  -- '0' = 48
  else Char.ofNat (n.toNat - 10 + 97)       -- 'a' = 97

/-- Convert a byte to two hex characters. -/
private def byteToHex (b : UInt8) : String :=
  ⟨[nibbleToHex (b / 16), nibbleToHex (b % 16)]⟩

/-- Encode byte list as hex string. -/
def toHexString (bytes : List UInt8) : String :=
  String.join (bytes.map byteToHex)

/-- Convert hex character to nibble value. -/
private def hexToNibble (c : Char) : Option UInt8 :=
  if '0' ≤ c ∧ c ≤ '9' then some (c.toNat - 48).toUInt8
  else if 'a' ≤ c ∧ c ≤ 'f' then some (c.toNat - 97 + 10).toUInt8
  else if 'A' ≤ c ∧ c ≤ 'F' then some (c.toNat - 65 + 10).toUInt8
  else none

/-- Parse pairs of hex characters into bytes. -/
private def parseHexPairs : List Char → Option (List UInt8)
  | [] => some []
  | [_] => none  -- Odd number of characters
  | c1 :: c2 :: rest => do
    let n1 ← hexToNibble c1
    let n2 ← hexToNibble c2
    let byte := n1 * 16 + n2
    let tail ← parseHexPairs rest
    some (byte :: tail)

/-- Parse hex string to byte list. -/
def fromHexString (s : String) : Option (List UInt8) :=
  parseHexPairs s.toList

/-- JSON serialization: encode as 64-character hex string. -/
instance : ToJson ByteArray32 where
  toJson a := Json.str (toHexString a.bytes)

/-- JSON deserialization: parse 64-character hex string. -/
instance : FromJson ByteArray32 where
  fromJson? j := do
    let s ← j.getStr?
    if s.length ≠ 64 then
      throw s!"ByteArray32 must be 64 hex chars, got {s.length}"
    match fromHexString s with
    | some bytes =>
      if h : bytes.length = 32 then
        pure ⟨bytes, h⟩
      else
        throw s!"ByteArray32 hex decode produced {bytes.length} bytes, expected 32"
    | none =>
      throw "ByteArray32 contains invalid hex characters"

/-! ## Utilities -/

/-- Helper: List.replicate always produces a list of the specified length. -/
private theorem replicate_length (n : Nat) (x : α) : (List.replicate n x).length = n := by
  induction n with
  | zero => rfl
  | succ n ih => simp only [List.replicate, List.length_cons, ih]

/-- Create a ByteArray32 filled with zeros. -/
def zero : ByteArray32 :=
  ⟨List.replicate 32 0, replicate_length 32 0⟩

/-- Create a ByteArray32 from a single byte repeated 32 times. -/
def replicate (b : UInt8) : ByteArray32 :=
  ⟨List.replicate 32 b, replicate_length 32 b⟩

end Aura.Types.ByteArray32
