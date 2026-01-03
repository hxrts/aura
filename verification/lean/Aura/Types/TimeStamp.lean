import Lean.Data.Json
import Aura.Types.ByteArray32
import Aura.Types.OrderTime

/-! # Aura.Types.TimeStamp

Unified timestamp type with 4 domain-specific variants.

## Rust Correspondence
- File: crates/aura-core/src/time/mod.rs
- Enum: `TimeStamp` with LogicalClock, OrderClock, PhysicalClock, Range variants
- Key property: Domain separation - cross-domain comparisons are incomparable

## Expose

**Types** (stable):
- `PhysicalTime`: Wall-clock time with optional uncertainty
- `LogicalTime`: Vector clock + Lamport counter for causality
- `RangeTime`: Validity window constraints
- `TimeStamp`: 4-variant enum (LogicalClock, OrderClock, PhysicalClock, Range)

**Operations** (stable):
- JSON (de)serialization with variant tags
- BEq instance for each variant

Note: TimeStamp does NOT implement Ord because cross-domain comparisons are undefined.
Only OrderTime (via Fact.order) provides total ordering for journal merge.
-/

namespace Aura.Types.TimeStamp

open Lean (Json ToJson FromJson)
open Aura.Types.ByteArray32 (ByteArray32)
open Aura.Types.OrderTime (OrderTime)

/-! ## Physical Time -/

/-- Physical wall-clock time with optional uncertainty.
    Rust: aura-core/src/time/mod.rs::PhysicalTime -/
structure PhysicalTime where
  /-- Milliseconds since UNIX epoch. -/
  ts_ms : Nat
  /-- Optional uncertainty in milliseconds. -/
  uncertainty : Option Nat
  deriving Repr, BEq, DecidableEq

instance : ToJson PhysicalTime where
  toJson p := Json.mkObj [
    ("ts_ms", Json.num p.ts_ms),
    ("uncertainty", match p.uncertainty with
      | some u => Json.num u
      | none => Json.null)
  ]

instance : FromJson PhysicalTime where
  fromJson? j := do
    let ts_ms ← j.getObjValAs? Nat "ts_ms"
    let uncertainty ← match j.getObjVal? "uncertainty" with
      | .ok (Json.null) => pure none
      | .ok v => some <$> FromJson.fromJson? v
      | .error _ => pure none
    pure ⟨ts_ms, uncertainty⟩

/-! ## Logical Time -/

/-- Device ID for vector clock entries.
    Simplified representation for Lean model (16 bytes, padded to 32).
    Rust: DeviceId(Uuid) in identifiers.rs -/
structure DeviceId where
  /-- The underlying identifier value. -/
  value : ByteArray32
  deriving Repr, BEq, DecidableEq

instance : Ord DeviceId where
  compare a b := Aura.Types.ByteArray32.compare a.value b.value

instance : ToJson DeviceId where
  toJson d := ToJson.toJson d.value

instance : FromJson DeviceId where
  fromJson? j := do
    let bytes ← FromJson.fromJson? j
    pure ⟨bytes⟩

/-- A single entry in a vector clock. -/
structure VectorEntry where
  device : DeviceId
  counter : Nat
  deriving Repr, BEq

/-- Vector clock for causal ordering.
    Simplified as list of entries (Rust uses optimized enum with Single/Multiple).
    Rust: aura-core/src/time/mod.rs::VectorClock -/
structure VectorClock where
  /-- Entries sorted by device ID. -/
  entries : List VectorEntry
  deriving Repr, BEq

/-- Create an empty vector clock. -/
def VectorClock.empty : VectorClock := ⟨[]⟩

/-- Logical time with vector clock and Lamport counter.
    Rust: aura-core/src/time/mod.rs::LogicalTime -/
structure LogicalTime where
  /-- Vector clock for partial ordering. -/
  vector : VectorClock
  /-- Lamport counter for tie-breaking. -/
  lamport : Nat
  deriving Repr, BEq

instance : ToJson VectorEntry where
  toJson e := Json.mkObj [
    ("device", ToJson.toJson e.device),
    ("counter", Json.num e.counter)
  ]

instance : FromJson VectorEntry where
  fromJson? j := do
    let device ← j.getObjValAs? DeviceId "device"
    let counter ← j.getObjValAs? Nat "counter"
    pure ⟨device, counter⟩

instance : ToJson VectorClock where
  toJson v := ToJson.toJson v.entries

instance : FromJson VectorClock where
  fromJson? j := do
    let entries ← FromJson.fromJson? j
    pure ⟨entries⟩

instance : ToJson LogicalTime where
  toJson l := Json.mkObj [
    ("vector", ToJson.toJson l.vector),
    ("lamport", Json.num l.lamport)
  ]

instance : FromJson LogicalTime where
  fromJson? j := do
    let vector ← j.getObjValAs? VectorClock "vector"
    let lamport ← j.getObjValAs? Nat "lamport"
    pure ⟨vector, lamport⟩

/-! ## Range Time -/

/-- Confidence level for range time constraints.
    Rust: aura-core/src/time/mod.rs::TimeConfidence -/
inductive TimeConfidence where
  | low
  | medium
  | high
  deriving Repr, BEq, DecidableEq

instance : ToJson TimeConfidence where
  toJson
    | .low => Json.str "low"
    | .medium => Json.str "medium"
    | .high => Json.str "high"

instance : FromJson TimeConfidence where
  fromJson? j := do
    let s ← j.getStr?
    match s with
    | "low" => pure .low
    | "medium" => pure .medium
    | "high" => pure .high
    | _ => throw s!"Unknown TimeConfidence: {s}"

/-- Range time with validity window constraints.
    Rust: aura-core/src/time/mod.rs::RangeTime -/
structure RangeTime where
  /-- Earliest valid time in milliseconds. -/
  earliest_ms : Nat
  /-- Latest valid time in milliseconds. -/
  latest_ms : Nat
  /-- Confidence level. -/
  confidence : TimeConfidence
  deriving Repr, BEq

instance : ToJson RangeTime where
  toJson r := Json.mkObj [
    ("earliest_ms", Json.num r.earliest_ms),
    ("latest_ms", Json.num r.latest_ms),
    ("confidence", ToJson.toJson r.confidence)
  ]

instance : FromJson RangeTime where
  fromJson? j := do
    let earliest_ms ← j.getObjValAs? Nat "earliest_ms"
    let latest_ms ← j.getObjValAs? Nat "latest_ms"
    let confidence ← j.getObjValAs? TimeConfidence "confidence"
    pure ⟨earliest_ms, latest_ms, confidence⟩

/-! ## TimeStamp Enum -/

/-- Unified timestamp type with domain separation.
    Cross-domain comparisons are undefined (return Incomparable in Rust).
    Only OrderTime provides total ordering for journal merge.
    Rust: aura-core/src/time/mod.rs::TimeStamp -/
inductive TimeStamp where
  /-- Logical clock for causal partial ordering. -/
  | logicalClock (time : LogicalTime)
  /-- Order clock for opaque total ordering (no causality). -/
  | orderClock (time : OrderTime)
  /-- Physical clock for local wall-clock claims. -/
  | physicalClock (time : PhysicalTime)
  /-- Range constraint on validity window. -/
  | range (time : RangeTime)
  deriving Repr, BEq

/-! ## JSON Serialization with Variant Tags -/

instance : ToJson TimeStamp where
  toJson
    | .logicalClock t => Json.mkObj [("variant", "logicalClock"), ("value", ToJson.toJson t)]
    | .orderClock t => Json.mkObj [("variant", "orderClock"), ("value", ToJson.toJson t)]
    | .physicalClock t => Json.mkObj [("variant", "physicalClock"), ("value", ToJson.toJson t)]
    | .range t => Json.mkObj [("variant", "range"), ("value", ToJson.toJson t)]

instance : FromJson TimeStamp where
  fromJson? j := do
    let variant ← j.getObjValAs? String "variant"
    match variant with
    | "logicalClock" => .logicalClock <$> j.getObjValAs? LogicalTime "value"
    | "orderClock" => .orderClock <$> j.getObjValAs? OrderTime "value"
    | "physicalClock" => .physicalClock <$> j.getObjValAs? PhysicalTime "value"
    | "range" => .range <$> j.getObjValAs? RangeTime "value"
    | _ => throw s!"Unknown TimeStamp variant: {variant}"

/-! ## Utilities -/

/-- Create a physical timestamp from milliseconds. -/
def physicalFromMs (ms : Nat) : TimeStamp :=
  .physicalClock ⟨ms, none⟩

/-- Create an order timestamp from an OrderTime value. -/
def fromOrderTime (ot : OrderTime) : TimeStamp :=
  .orderClock ot

/-- Create a logical timestamp with just a Lamport counter. -/
def logicalFromLamport (lamport : Nat) : TimeStamp :=
  .logicalClock ⟨VectorClock.empty, lamport⟩

end Aura.Types.TimeStamp
