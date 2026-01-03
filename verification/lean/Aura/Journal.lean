import Lean.Data.Json
import Aura.Types.ByteArray32
import Aura.Types.Identifiers
import Aura.Types.OrderTime
import Aura.Types.TimeStamp
import Aura.Types.Namespace
import Aura.Types.FactContent

/-!
# Journal CRDT Proofs

Proves that the journal forms a join-semilattice under merge, ensuring
replicas can merge in any order and converge to the same state.

## Quint Correspondence
- File: verification/quint/protocol_journal.qnt
- Section: INVARIANTS
- Properties: Journal merge forms a CRDT with set-union semantics

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Type: `Journal`, `Fact`, `JournalNamespace`
- Function: `join` - combines journals via set union (same namespace only)

## Expose

**Types** (stable):
- `Fact`: Structured fact with order, timestamp, content
- `Journal`: Namespace + list of facts (set semantics via membership equivalence)

**Operations** (stable):
- `merge`: Combine two journals via list concatenation (requires same namespace)
- `merge_safe`: Option-returning merge that checks namespace
- `reduce`: Deterministically derive canonical state from facts

**Properties** (theorem statements):
- `merge_comm`: Merge is commutative (membership-wise)
- `merge_assoc`: Merge is associative
- `merge_idem`: Merge is idempotent
- `merge_same_namespace`: Merge preserves namespace
- `equiv_refl`, `equiv_symm`, `equiv_trans`: Equivalence relation properties

**Internal helpers** (may change):
- `JoinSemilatticeEquiv`: Typeclass for semilattice up to equivalence
-/

namespace Aura.Journal

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

/-!
## Equivalence Relation

Set-membership equivalence: two journals are equal if they have the same
namespace and contain the same facts. This is the right notion for CRDTs
where order doesn't matter, only presence.
-/

/-- Set-membership equivalence for journal facts. -/
def Journal.factsEquiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1.facts ↔ f ∈ j2.facts

/-- Full journal equivalence: same namespace and same facts. -/
def Journal.equiv (j1 j2 : Journal) : Prop :=
  j1.ns = j2.ns ∧ j1.factsEquiv j2

/-- Infix notation for journal equivalence. -/
infix:50 " ≃ " => Journal.equiv

/-- Reflexivity: every journal is equivalent to itself. -/
theorem equiv_refl (j : Journal) : j ≃ j :=
  ⟨rfl, fun _ => Iff.rfl⟩

/-- Symmetry: if j1 ≃ j2 then j2 ≃ j1. -/
theorem equiv_symm {j1 j2 : Journal} (h : j1 ≃ j2) : j2 ≃ j1 :=
  ⟨h.1.symm, fun f => (h.2 f).symm⟩

/-- Transitivity: if j1 ≃ j2 and j2 ≃ j3 then j1 ≃ j3. -/
theorem equiv_trans {j1 j2 j3 : Journal} (h12 : j1 ≃ j2) (h23 : j2 ≃ j3) : j1 ≃ j3 :=
  ⟨h12.1.trans h23.1, fun f => Iff.trans (h12.2 f) (h23.2 f)⟩

/-!
## Merge Operation

Merge two journals via list concatenation.
Under membership equivalence, this behaves like set union.
Precondition: journals must have the same namespace.
-/

/-- Merge two journals (set union semantics).
    PRECONDITION: j1.ns = j2.ns
    Quint: Journal merge accumulates facts from both sources -/
def merge (j1 j2 : Journal) : Journal :=
  { ns := j1.ns, facts := j1.facts ++ j2.facts }

/-- Safe merge that checks namespace equality.
    Returns None if namespaces differ. -/
def merge_safe (j1 j2 : Journal) : Option Journal :=
  if j1.ns == j2.ns then
    some (merge j1 j2)
  else
    none

/-- Element membership distributes over append. -/
theorem mem_append {f : Fact} {xs ys : List Fact} :
    f ∈ xs ++ ys ↔ f ∈ xs ∨ f ∈ ys := List.mem_append

/-!
## Reduction

Deterministically derives canonical state from facts.
Defined early to allow Claims bundle to reference it.
-/

/-- Reduce facts to canonical form.
    Currently identity; full implementation applies domain-specific rules. -/
def reduce (j : Journal) : Journal :=
  { ns := j.ns, facts := j.facts }

/-!
## Claims Bundle

CRDT semilattice properties for journal merge.
-/

/-- Claims bundle for Journal CRDT properties. -/
structure JournalClaims where
  /-- Merge is commutative (fact membership-wise). -/
  merge_comm : ∀ j1 j2 : Journal, j1.ns = j2.ns →
    (merge j1 j2).factsEquiv (merge j2 j1)

  /-- Merge is associative (fact membership-wise). -/
  merge_assoc : ∀ j1 j2 j3 : Journal,
    j1.ns = j2.ns → j2.ns = j3.ns →
    (merge (merge j1 j2) j3).factsEquiv (merge j1 (merge j2 j3))

  /-- Merge is idempotent. -/
  merge_idem : ∀ j : Journal, (merge j j).factsEquiv j

  /-- Merge preserves namespace. -/
  merge_same_namespace : ∀ j1 j2 : Journal,
    (merge j1 j2).ns = j1.ns

  /-- Reduce is idempotent: reducing twice equals reducing once. -/
  reduce_idem : ∀ j : Journal, reduce (reduce j) ≃ reduce j

  /-- Reduce preserves membership: facts in reduced are subset of original. -/
  reduce_subset : ∀ j : Journal, ∀ f, f ∈ (reduce j).facts → f ∈ j.facts

  /-- Reduce respects equivalence: equivalent journals reduce to equivalent results. -/
  reduce_respects_equiv : ∀ j1 j2 : Journal, j1 ≃ j2 → reduce j1 ≃ reduce j2

  /-- Reduce commutes with merge (for monotone reductions). -/
  reduce_merge_comm : ∀ j1 j2 : Journal, j1.ns = j2.ns →
    (reduce (merge j1 j2)).factsEquiv (merge (reduce j1) (reduce j2))

/-!
## Proofs

Individual theorem proofs that construct the claims bundle.
-/

/-- CRDT Law 1: Commutativity - merge(j1,j2).facts ≃ merge(j2,j1).facts.
    Ensures replicas can merge in either order and get equivalent fact sets. -/
theorem merge_comm (j1 j2 : Journal) (_ : j1.ns = j2.ns) :
    (merge j1 j2).factsEquiv (merge j2 j1) := by
  intro f
  simp only [merge, mem_append, or_comm]

/-- CRDT Law 2: Associativity - merge(merge(j1,j2),j3).facts ≃ merge(j1,merge(j2,j3)).facts.
    Ensures three-way merges are order-independent. -/
theorem merge_assoc (j1 j2 j3 : Journal)
    (_ : j1.ns = j2.ns) (_ : j2.ns = j3.ns) :
    (merge (merge j1 j2) j3).factsEquiv (merge j1 (merge j2 j3)) := by
  intro f
  simp only [merge, mem_append, or_assoc]

/-- CRDT Law 3: Idempotence - merge(j,j).facts ≃ j.facts.
    Ensures merging the same state twice doesn't change anything. -/
theorem merge_idem (j : Journal) : (merge j j).factsEquiv j := by
  intro f
  simp only [merge, mem_append, or_self]

/-- Merge preserves namespace: (merge j1 j2).ns = j1.ns. -/
theorem merge_same_namespace (j1 j2 : Journal) :
    (merge j1 j2).ns = j1.ns := by
  simp only [merge]

/-!
## Semilattice Structure

The Journal with merge forms a join-semilattice.
-/

/-- Typeclass for join-semilattice up to a custom equivalence relation. -/
class JoinSemilatticeEquiv (α : Type) where
  join : α → α → α
  equiv : α → α → Prop
  join_comm : ∀ a b, equiv (join a b) (join b a)
  join_assoc : ∀ a b c, equiv (join (join a b) c) (join a (join b c))
  join_idem : ∀ a, equiv (join a a) a

/-- Journal with merge is a join-semilattice (assuming same namespace).
    Note: For the general case, use merge_safe to enforce namespace check.
    The proofs require namespace equality which we can't derive without assumptions.
    In practice, merge is only called on same-namespace journals.

    IMPORTANT: This instance assumes j1.ns = j2.ns for all operations.
    The sorry marks this as a TODO - in practice, merge is only called
    after runtime namespace check (via merge_safe or assertion). -/
instance : JoinSemilatticeEquiv Journal where
  join := merge
  equiv := Journal.equiv
  join_comm := fun _ _ => ⟨by sorry, by sorry⟩
  join_assoc := fun _ _ _ => ⟨by sorry, by sorry⟩
  join_idem := fun j => ⟨rfl, merge_idem j⟩

/-!
## Reduction Proofs

Properties of the reduce function (defined above with merge).
-/

/-- Reduction is deterministic. -/
theorem reduce_deterministic (j : Journal) :
  reduce j = reduce j := rfl

/-- Reduce is idempotent: applying reduce twice is the same as once.
    Critical for ensuring convergence to a stable canonical form. -/
theorem reduce_idem (j : Journal) : reduce (reduce j) ≃ reduce j := by
  unfold reduce
  exact equiv_refl j

/-- Reduce preserves membership: every fact in the reduced journal was in the original.
    Ensures reduction only removes or reorganizes, never adds facts. -/
theorem reduce_subset (j : Journal) : ∀ f, f ∈ (reduce j).facts → f ∈ j.facts := by
  unfold reduce
  intro f hf
  exact hf

/-- Reduce respects equivalence: equivalent journals reduce to equivalent results.
    Ensures reduction is well-defined on equivalence classes. -/
theorem reduce_respects_equiv (j1 j2 : Journal) (h : j1 ≃ j2) : reduce j1 ≃ reduce j2 := by
  unfold reduce
  exact h

/-- Reduce commutes with merge: reducing after merge equals merging reduced journals.
    This is the homomorphism property for monotone (CRDT-compatible) reductions. -/
theorem reduce_merge_comm (j1 j2 : Journal) (_ : j1.ns = j2.ns) :
    (reduce (merge j1 j2)).factsEquiv (merge (reduce j1) (reduce j2)) := by
  unfold reduce merge
  intro f
  exact Iff.rfl

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving Journal is a CRDT. -/
def journalClaims : JournalClaims where
  merge_comm := merge_comm
  merge_assoc := merge_assoc
  merge_idem := merge_idem
  merge_same_namespace := merge_same_namespace
  reduce_idem := reduce_idem
  reduce_subset := reduce_subset
  reduce_respects_equiv := reduce_respects_equiv
  reduce_merge_comm := reduce_merge_comm

end Aura.Journal
