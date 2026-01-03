import Aura.Domain.Journal.Types

/-!
# Journal Operations

Operations on the journal CRDT: merge, reduce, and equivalence relations.

## Quint Correspondence
- File: verification/quint/protocol_journal.qnt
- Section: OPERATIONS
- Properties: Journal merge accumulates facts from both sources

## Rust Correspondence
- File: crates/aura-journal/src/fact.rs
- Function: `join` - combines journals via set union (same namespace only)

## Expose

**Operations** (stable):
- `merge`: Combine two journals via list concatenation (requires same namespace)
- `merge_safe`: Option-returning merge that checks namespace
- `reduce`: Deterministically derive canonical state from facts

**Predicates** (stable):
- `Journal.factsEquiv`: Set-membership equivalence for facts
- `Journal.equiv`: Full journal equivalence (namespace + facts)

**Typeclasses**:
- `JoinSemilatticeEquiv`: Typeclass for semilattice up to equivalence
-/

namespace Aura.Domain.Journal

open Aura.Domain.Journal (Fact Journal)

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
-/

/-- Reduce facts to canonical form.
    Currently identity; full implementation applies domain-specific rules. -/
def reduce (j : Journal) : Journal :=
  { ns := j.ns, facts := j.facts }

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

end Aura.Domain.Journal
