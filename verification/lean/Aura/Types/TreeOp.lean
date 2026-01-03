import Lean.Data.Json

/-! # Aura.Types.TreeOp

Commitment tree operation types for authority state management.

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Enum: `TreeOpKind` with AddLeaf, RemoveLeaf, UpdatePolicy, RotateEpoch variants
- File: crates/aura-core/src/tree/types.rs
- Enum: `LeafRole` with Device and Guardian variants

## Expose

**Types** (stable):
- `LeafRole`: Device or Guardian role for tree leaves
- `TreeOpKind`: Tree operations (AddLeaf, RemoveLeaf, UpdatePolicy, RotateEpoch)
-/

namespace Aura.Types.TreeOp

open Lean (Json ToJson FromJson)

/-! ## Leaf Role -/

/-- Role of a leaf node in the commitment tree.
    Rust: aura-core/src/tree/types.rs::LeafRole -/
inductive LeafRole where
  /-- A device owned and controlled by the identity. -/
  | device
  /-- A guardian trusted to help with recovery. -/
  | guardian
  deriving Repr, BEq, DecidableEq

instance : ToJson LeafRole where
  toJson
    | .device => Json.str "device"
    | .guardian => Json.str "guardian"

instance : FromJson LeafRole where
  fromJson? j := do
    let s ← j.getStr?
    match s with
    | "device" => pure .device
    | "guardian" => pure .guardian
    | _ => throw s!"Unknown LeafRole: {s}"

/-! ## Tree Operation Kind -/

/-- Tree operation type for commitment tree modifications.
    Rust: aura-journal/src/fact.rs::TreeOpKind -/
inductive TreeOpKind where
  /-- Add a new device/leaf to the tree. -/
  | addLeaf (public_key : List UInt8) (role : LeafRole)
  /-- Remove a device/leaf from the tree. -/
  | removeLeaf (leaf_index : Nat)
  /-- Update the threshold policy for the tree. -/
  | updatePolicy (threshold : Nat)
  /-- Rotate to a new epoch (key rotation). -/
  | rotateEpoch
  deriving Repr, BEq

/-! ## JSON Serialization -/

private def bytesToJsonArray (bytes : List UInt8) : Json :=
  Json.arr (bytes.map (fun b => Json.num b.toNat)).toArray

private def jsonArrayToBytes (j : Json) : Except String (List UInt8) := do
  let arr ← j.getArr?
  arr.toList.mapM fun v => do
    let n ← v.getNat?
    if n > 255 then throw "byte value out of range"
    pure n.toUInt8

instance : ToJson TreeOpKind where
  toJson
    | .addLeaf pk role => Json.mkObj [
        ("variant", "addLeaf"),
        ("public_key", bytesToJsonArray pk),
        ("role", ToJson.toJson role)
      ]
    | .removeLeaf idx => Json.mkObj [
        ("variant", "removeLeaf"),
        ("leaf_index", Json.num idx)
      ]
    | .updatePolicy thresh => Json.mkObj [
        ("variant", "updatePolicy"),
        ("threshold", Json.num thresh)
      ]
    | .rotateEpoch => Json.mkObj [("variant", "rotateEpoch")]

instance : FromJson TreeOpKind where
  fromJson? j := do
    let variant ← j.getObjValAs? String "variant"
    match variant with
    | "addLeaf" => do
      let pk_json ← j.getObjVal? "public_key"
      let pk ← jsonArrayToBytes pk_json
      let role ← j.getObjValAs? LeafRole "role"
      pure (.addLeaf pk role)
    | "removeLeaf" =>
      .removeLeaf <$> j.getObjValAs? Nat "leaf_index"
    | "updatePolicy" =>
      .updatePolicy <$> j.getObjValAs? Nat "threshold"
    | "rotateEpoch" =>
      pure .rotateEpoch
    | _ => throw s!"Unknown TreeOpKind variant: {variant}"

end Aura.Types.TreeOp
