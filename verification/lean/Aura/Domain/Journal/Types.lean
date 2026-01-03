import Lean.Data.Json
import Aura.Types.ByteArray32
import Aura.Types.Identifiers
import Aura.Types.OrderTime
import Aura.Types.TimeStamp
import Aura.Types.Namespace
import Aura.Types.FactContent

/-!
# Journal Domain Types

Core type definitions for the journal CRDT system.

## Quint Correspondence
- File: verification/quint/protocol_journal.qnt
- Types: Fact, Journal

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Type: `Journal`, `Fact`, `JournalNamespace`

## Expose

**Types** (stable):
- `Fact`: Structured fact with order, timestamp, content
- `Journal`: Namespace + list of facts (set semantics via membership equivalence)

**Instances**:
- `ToJson Fact`, `FromJson Fact`: JSON serialization
- `ToJson Journal`, `FromJson Journal`: JSON serialization
- `Ord Fact`: Comparison by order key
-/

namespace Aura.Domain.Journal

open Lean (Json ToJson FromJson)
open Aura.Types.ByteArray32 (ByteArray32)
open Aura.Types.Identifiers (Hash32 AuthorityId ContextId)
open Aura.Types.OrderTime (OrderTime)
open Aura.Types.TimeStamp (TimeStamp)
open Aura.Types.Namespace (JournalNamespace)
open Aura.Types.FactContent (FactContent)

/-!
## Core Types

Structured fact with order key, semantic timestamp, and typed content.
-/

/-- Structured fact with ordering, timestamp, and content.
    Rust: aura-journal/src/fact.rs::Fact -/
structure Fact where
  /-- Opaque total order for deterministic merges. -/
  order : OrderTime
  /-- Semantic timestamp (not for ordering). -/
  timestamp : TimeStamp
  /-- Content payload (4 variants). -/
  content : FactContent
  deriving Repr, BEq

/-- Compare facts by their order key. -/
def Fact.compare (a b : Fact) : Ordering :=
  Aura.Types.OrderTime.compare a.order b.order

instance : Ord Fact where
  compare := Fact.compare

/-! ## JSON Serialization for Fact -/

instance : ToJson Fact where
  toJson f := Json.mkObj [
    ("order", ToJson.toJson f.order),
    ("timestamp", ToJson.toJson f.timestamp),
    ("content", ToJson.toJson f.content)
  ]

instance : FromJson Fact where
  fromJson? j := do
    let order ← j.getObjValAs? OrderTime "order"
    let timestamp ← j.getObjValAs? TimeStamp "timestamp"
    let content ← j.getObjValAs? FactContent "content"
    pure ⟨order, timestamp, content⟩

/-!
## Journal Structure

Journal with namespace and list of facts (set semantics via membership equivalence).
-/

/-- Journal as a namespace plus list of facts.
    We use List rather than Finset for pure Lean 4 compatibility.
    Rust: aura-journal/src/fact.rs::Journal -/
structure Journal where
  /-- Namespace this journal belongs to. -/
  ns : JournalNamespace
  /-- Facts in this journal (set semantics via membership equivalence). -/
  facts : List Fact
  deriving Repr, BEq

/-! ## JSON Serialization for Journal -/

instance : ToJson Journal where
  toJson j := Json.mkObj [
    ("namespace", ToJson.toJson j.ns),
    ("facts", Json.arr (j.facts.map ToJson.toJson).toArray)
  ]

instance : FromJson Journal where
  fromJson? j := do
    let ns ← j.getObjValAs? JournalNamespace "namespace"
    let factsArr ← j.getObjValAs? (Array Json) "facts"
    let facts ← factsArr.toList.mapM fun fj => FromJson.fromJson? fj
    pure ⟨ns, facts⟩

end Aura.Domain.Journal
