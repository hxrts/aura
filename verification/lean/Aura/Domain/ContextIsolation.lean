/-!
# Context Isolation Types and Operations

Types and operations for context isolation in Aura's privacy model.
Contexts are isolated namespaces that prevent information flow between
unrelated parties.

## Quint Correspondence
- File: verification/quint/authorization.qnt
- Section: Context isolation checks

## Rust Correspondence
- File: crates/aura-core/src/context.rs
- Types: `ContextId`, `Message`
- Trait: `ContextScope`

## Expose

**Types** (stable):
- `ContextId`: Opaque context identifier
- `ContextMessage`: Context-scoped message
- `Bridge`: Explicit cross-context bridge

**Operations** (stable):
- `isSameContext`: Check if two messages are in the same context
- `canBridge`: Check if a bridge allows cross-context flow
-/

namespace Aura.Domain.ContextIsolation

/-!
## Core Types

Context identifiers and scoped messages.
-/

/-- Opaque context identifier.
    Represents either an AuthorityId namespace or a RelationalContext.
    Rust: ContextId(Uuid) -/
structure ContextId where
  id : String
  deriving BEq, Repr, DecidableEq

instance : LawfulBEq ContextId where
  eq_of_beq {a b} h := by
    cases a with | mk va =>
    cases b with | mk vb =>
    have hstr : (va == vb) = true := h
    have heq : va = vb := eq_of_beq hstr
    simp only [heq]
  rfl {a} := by
    cases a with | mk v =>
    show (v == v) = true
    exact beq_self_eq_true v

/-- Authority identifier (journal namespace owner). -/
structure AuthorityId where
  id : String
  deriving BEq, Repr, DecidableEq

instance : LawfulBEq AuthorityId where
  eq_of_beq {a b} h := by
    cases a with | mk va =>
    cases b with | mk vb =>
    have hstr : (va == vb) = true := h
    have heq : va = vb := eq_of_beq hstr
    simp only [heq]
  rfl {a} := by
    cases a with | mk v =>
    show (v == v) = true
    exact beq_self_eq_true v

/-- Message scoped to a context.
    Messages cannot flow between contexts without explicit bridging.
    Rust: Msg<Ctx, Payload, Version> -/
structure ContextMessage where
  contextId : ContextId
  payload : String  -- Simplified; actual payload is typed
  deriving BEq, Repr

/-- Bridge between two contexts.
    Bridges must be explicitly typed and authorized.
    See docs/002_theoretical_model.md for bridge protocols. -/
structure Bridge where
  source : ContextId
  target : ContextId
  authorized : Bool
  deriving BEq, Repr

/-!
## Context Scoping Operations

Operations for checking and enforcing context isolation.
-/

/-- Check if two messages are in the same context.
    Quint: Message extraction validates context before processing -/
def isSameContext (m1 m2 : ContextMessage) : Bool :=
  m1.contextId == m2.contextId

/-- Check if a bridge allows flow from source to target context. -/
def canBridge (bridge : Bridge) (source target : ContextId) : Bool :=
  bridge.source == source && bridge.target == target && bridge.authorized

/-- Check if a message can be processed in a given context.
    Returns true only if the message's context matches. -/
def canProcess (msg : ContextMessage) (ctx : ContextId) : Bool :=
  msg.contextId == ctx

/-!
## Journal Namespace Types

Journals are namespaced by context for isolation.
-/

/-- Journal namespace: either an authority or a relational context.
    Rust: JournalNamespace enum in aura-journal -/
inductive JournalNamespace where
  | authority : AuthorityId → JournalNamespace
  | context : ContextId → JournalNamespace
  deriving BEq, Repr

/-- Extract the context ID from a namespace (if applicable). -/
def namespaceContext : JournalNamespace → Option ContextId
  | JournalNamespace.context ctx => some ctx
  | JournalNamespace.authority _ => none

/-- Check if two namespaces are compatible for merging.
    Only namespaces of the same type and ID can merge.
    Rust: Journal merge assertion -/
def namespacesCompatible (n1 n2 : JournalNamespace) : Bool :=
  match n1, n2 with
  | JournalNamespace.authority a1, JournalNamespace.authority a2 => a1 == a2
  | JournalNamespace.context c1, JournalNamespace.context c2 => c1 == c2
  | _, _ => false

end Aura.Domain.ContextIsolation
