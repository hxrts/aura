import Lean.Data.Json
import Aura.Types.Identifiers
import Aura.Types.TreeOp

/-! # Aura.Types.AttestedOp

Attested tree operations with threshold signatures.

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Struct: `AttestedOp` with tree_op, commitments, threshold, signature

## Expose

**Types** (stable):
- `AttestedOp`: Tree operation attested by threshold of witnesses
-/

namespace Aura.Types.AttestedOp

open Lean (Json ToJson FromJson)
open Aura.Types.Identifiers (Hash32)
open Aura.Types.TreeOp (TreeOpKind)

/-! ## Core Type -/

/-- Attested tree operation with threshold signature.
    Rust: aura-journal/src/fact.rs::AttestedOp -/
structure AttestedOp where
  /-- The tree operation being attested. -/
  tree_op : TreeOpKind
  /-- Commitment before the operation. -/
  parent_commitment : Hash32
  /-- Commitment after the operation. -/
  new_commitment : Hash32
  /-- Number of witnesses that attested. -/
  witness_threshold : Nat
  /-- Aggregated threshold signature (opaque bytes). -/
  signature : List UInt8
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

instance : ToJson AttestedOp where
  toJson op := Json.mkObj [
    ("tree_op", ToJson.toJson op.tree_op),
    ("parent_commitment", ToJson.toJson op.parent_commitment),
    ("new_commitment", ToJson.toJson op.new_commitment),
    ("witness_threshold", Json.num op.witness_threshold),
    ("signature", bytesToJsonArray op.signature)
  ]

instance : FromJson AttestedOp where
  fromJson? j := do
    let tree_op ← j.getObjValAs? TreeOpKind "tree_op"
    let parent_commitment ← j.getObjValAs? Hash32 "parent_commitment"
    let new_commitment ← j.getObjValAs? Hash32 "new_commitment"
    let witness_threshold ← j.getObjValAs? Nat "witness_threshold"
    let sig_json ← j.getObjVal? "signature"
    let signature ← jsonArrayToBytes sig_json
    pure ⟨tree_op, parent_commitment, new_commitment, witness_threshold, signature⟩

end Aura.Types.AttestedOp
