import Lean.Data.Json
import Aura.Types.ByteArray32
import Aura.Types.Identifiers
import Aura.Types.OrderTime
import Aura.Types.TimeStamp
import Aura.Types.AttestedOp
import Aura.Types.ProtocolFacts

/-! # Aura.Types.FactContent

Fact content types: the 4 variants of fact payload.

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Enum: `FactContent` with AttestedOp, Relational, Snapshot, RendezvousReceipt
- Enum: `RelationalFact` with Protocol and Generic variants

## Expose

**Types** (stable):
- `SnapshotFact`: Garbage collection marker
- `RelationalFact`: Protocol or Generic relational binding
- `FactContent`: Main enum (AttestedOp, Relational, Snapshot, RendezvousReceipt)
-/

namespace Aura.Types.FactContent

open Lean (Json ToJson FromJson)
open Aura.Types.ByteArray32 (ByteArray32)
open Aura.Types.Identifiers (Hash32 AuthorityId ContextId)
open Aura.Types.OrderTime (OrderTime)
open Aura.Types.TimeStamp (TimeStamp)
open Aura.Types.AttestedOp (AttestedOp)
open Aura.Types.ProtocolFacts (ProtocolRelationalFact)

/-! ## Snapshot Fact -/

/-- Snapshot marker for garbage collection.
    Rust: aura-journal/src/fact.rs::SnapshotFact -/
structure SnapshotFact where
  /-- Hash of the state at snapshot time. -/
  state_hash : Hash32
  /-- Facts that can be garbage collected. -/
  superseded_facts : List OrderTime
  /-- Snapshot sequence number. -/
  sequence : Nat
  deriving Repr, BEq

instance : ToJson SnapshotFact where
  toJson s := Json.mkObj [
    ("state_hash", ToJson.toJson s.state_hash),
    ("superseded_facts", ToJson.toJson s.superseded_facts),
    ("sequence", Json.num s.sequence)
  ]

instance : FromJson SnapshotFact where
  fromJson? j := do
    let state_hash ← j.getObjValAs? Hash32 "state_hash"
    let superseded_facts ← j.getObjValAs? (List OrderTime) "superseded_facts"
    let sequence ← j.getObjValAs? Nat "sequence"
    pure ⟨state_hash, superseded_facts, sequence⟩

/-! ## Relational Fact -/

/-- Relational fact for cross-authority coordination.
    Has two variants: Protocol (typed) and Generic (extensible).
    Rust: aura-journal/src/fact.rs::RelationalFact -/
inductive RelationalFact where
  /-- Protocol-level facts with specialized reduction logic. -/
  | protocol (fact : ProtocolRelationalFact)
  /-- Generic extensible binding for domain-specific facts. -/
  | generic (context_id : ContextId) (binding_type : String) (binding_data : List UInt8)
  deriving Repr, BEq

private def bytesToJsonArray (bytes : List UInt8) : Json :=
  Json.arr (bytes.map (fun b => Json.num b.toNat)).toArray

private def jsonArrayToBytes (j : Json) : Except String (List UInt8) := do
  let arr ← j.getArr?
  arr.toList.mapM fun v => do
    let n ← v.getNat?
    if n > 255 then throw "byte value out of range"
    pure n.toUInt8

instance : ToJson RelationalFact where
  toJson
    | .protocol fact => Json.mkObj [
        ("variant", "protocol"),
        ("data", ToJson.toJson fact)
      ]
    | .generic ctx btype bdata => Json.mkObj [
        ("variant", "generic"),
        ("context_id", ToJson.toJson ctx),
        ("binding_type", Json.str btype),
        ("binding_data", bytesToJsonArray bdata)
      ]

instance : FromJson RelationalFact where
  fromJson? j := do
    let variant ← j.getObjValAs? String "variant"
    match variant with
    | "protocol" =>
      .protocol <$> j.getObjValAs? ProtocolRelationalFact "data"
    | "generic" => do
      let ctx ← j.getObjValAs? ContextId "context_id"
      let btype ← j.getObjValAs? String "binding_type"
      let bdata_json ← j.getObjVal? "binding_data"
      let bdata ← jsonArrayToBytes bdata_json
      pure (.generic ctx btype bdata)
    | _ => throw s!"Unknown RelationalFact variant: {variant}"

/-! ## Fact Content -/

/-- Main fact content type with 4 variants.
    Rust: aura-journal/src/fact.rs::FactContent -/
inductive FactContent where
  /-- Attested operation on the commitment tree. -/
  | attestedOp (op : AttestedOp)
  /-- Relational fact for cross-authority coordination. -/
  | relational (fact : RelationalFact)
  /-- Snapshot marker for garbage collection. -/
  | snapshot (fact : SnapshotFact)
  /-- Rendezvous receipt for tracking message flow. -/
  | rendezvousReceipt
      (envelope_id : ByteArray32)
      (authority_id : AuthorityId)
      (timestamp : TimeStamp)
      (signature : List UInt8)
  deriving Repr, BEq

instance : ToJson FactContent where
  toJson
    | .attestedOp op => Json.mkObj [
        ("variant", "attestedOp"),
        ("data", ToJson.toJson op)
      ]
    | .relational fact => Json.mkObj [
        ("variant", "relational"),
        ("data", ToJson.toJson fact)
      ]
    | .snapshot fact => Json.mkObj [
        ("variant", "snapshot"),
        ("data", ToJson.toJson fact)
      ]
    | .rendezvousReceipt eid aid ts sig => Json.mkObj [
        ("variant", "rendezvousReceipt"),
        ("envelope_id", ToJson.toJson eid),
        ("authority_id", ToJson.toJson aid),
        ("timestamp", ToJson.toJson ts),
        ("signature", bytesToJsonArray sig)
      ]

instance : FromJson FactContent where
  fromJson? j := do
    let variant ← j.getObjValAs? String "variant"
    match variant with
    | "attestedOp" =>
      .attestedOp <$> j.getObjValAs? AttestedOp "data"
    | "relational" =>
      .relational <$> j.getObjValAs? RelationalFact "data"
    | "snapshot" =>
      .snapshot <$> j.getObjValAs? SnapshotFact "data"
    | "rendezvousReceipt" => do
      let eid ← j.getObjValAs? ByteArray32 "envelope_id"
      let aid ← j.getObjValAs? AuthorityId "authority_id"
      let ts ← j.getObjValAs? TimeStamp "timestamp"
      let sig_json ← j.getObjVal? "signature"
      let sig ← jsonArrayToBytes sig_json
      pure (.rendezvousReceipt eid aid ts sig)
    | _ => throw s!"Unknown FactContent variant: {variant}"

end Aura.Types.FactContent
