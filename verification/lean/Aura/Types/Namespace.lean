import Lean.Data.Json
import Aura.Types.Identifiers

/-! # Aura.Types.Namespace

Journal namespace for scoping facts to authorities or contexts.

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Enum: `JournalNamespace` with Authority and Context variants

## Expose

**Types** (stable):
- `JournalNamespace`: Enum with Authority(AuthorityId) or Context(ContextId)

**Properties**:
- Merge precondition: journals must have same namespace to merge
-/

namespace Aura.Types.Namespace

open Lean (Json ToJson FromJson)
open Aura.Types.Identifiers (AuthorityId ContextId)

/-! ## Core Type -/

/-- Journal namespace for scoping facts.
    Authority journals hold commitment tree operations.
    Context journals hold relational facts between authorities.
    Rust: aura-journal/src/fact.rs::JournalNamespace -/
inductive JournalNamespace where
  /-- Facts belonging to a specific authority. -/
  | authority (id : AuthorityId)
  /-- Facts belonging to a relational context. -/
  | context (id : ContextId)
  deriving Repr, BEq, DecidableEq

/-! ## JSON Serialization -/

instance : ToJson JournalNamespace where
  toJson
    | .authority id => Json.mkObj [("variant", "authority"), ("id", ToJson.toJson id)]
    | .context id => Json.mkObj [("variant", "context"), ("id", ToJson.toJson id)]

instance : FromJson JournalNamespace where
  fromJson? j := do
    let variant ← j.getObjValAs? String "variant"
    match variant with
    | "authority" => .authority <$> j.getObjValAs? AuthorityId "id"
    | "context" => .context <$> j.getObjValAs? ContextId "id"
    | _ => throw s!"Unknown JournalNamespace variant: {variant}"

end Aura.Types.Namespace
