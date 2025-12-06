-- Core definitions for Journal CRDT verification
-- Models the journal as a semilattice with deterministic reduction

namespace Aura.Journal

/-!
# Journal CRDT Core

This module models the core journal structure and reduction logic from aura-journal.
The journal forms a join-semilattice under set union, and reduction is deterministic.

Key properties to prove:
- merge is associative, commutative, and idempotent
- reduce is deterministic and pure

We use a simple membership-based model where journals are finite sets.
For the semilattice proofs, we work with set-membership equivalence.
-/

/-- A fact identifier -/
structure FactId where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Abstract representation of a Fact in the journal -/
structure Fact where
  id : FactId
  deriving BEq, Repr, DecidableEq

/-- Journal is represented as a list of facts (set semantics via membership) -/
abbrev Journal := List Fact

/-- Two journals are equivalent if they contain the same elements -/
def Journal.equiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1 ↔ f ∈ j2

/-- Notation for journal equivalence -/
infix:50 " ≃ " => Journal.equiv

/-- Equivalence is reflexive -/
theorem equiv_refl (j : Journal) : j ≃ j := fun _ => Iff.rfl

/-- Equivalence is symmetric -/
theorem equiv_symm {j1 j2 : Journal} (h : j1 ≃ j2) : j2 ≃ j1 :=
  fun f => (h f).symm

/-- Equivalence is transitive -/
theorem equiv_trans {j1 j2 j3 : Journal} (h12 : j1 ≃ j2) (h23 : j2 ≃ j3) : j1 ≃ j3 :=
  fun f => Iff.trans (h12 f) (h23 f)

/-- Merge two journals (concatenation - set union semantics) -/
def merge (j1 j2 : Journal) : Journal := j1 ++ j2

/-- Helper: membership in concatenation -/
theorem mem_append {f : Fact} {xs ys : List Fact} :
    f ∈ xs ++ ys ↔ f ∈ xs ∨ f ∈ ys := List.mem_append

/-- merge is commutative (up to equivalence) -/
theorem merge_comm (j1 j2 : Journal) : merge j1 j2 ≃ merge j2 j1 := by
  intro f
  simp only [merge, mem_append, or_comm]

/-- merge is associative (up to equivalence) -/
theorem merge_assoc (j1 j2 j3 : Journal) :
    merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3) := by
  intro f
  simp only [merge, mem_append, or_assoc]

/-- merge is idempotent (up to equivalence) -/
theorem merge_idem (j : Journal) : merge j j ≃ j := by
  intro f
  simp only [merge, mem_append, or_self]

/-!
## Semilattice Structure

The Journal with merge forms a join-semilattice, which is the foundation
of the CRDT convergence guarantee.

Note: The semilattice properties hold up to set-membership equivalence (≃),
which is the appropriate notion for CRDT semantics where we care about
the set of facts, not their order in the list representation.
-/

/-- Journal forms a join-semilattice under merge (up to equivalence) -/
class JoinSemilatticeEquiv (α : Type) where
  join : α → α → α
  equiv : α → α → Prop
  join_comm : ∀ a b, equiv (join a b) (join b a)
  join_assoc : ∀ a b c, equiv (join (join a b) c) (join a (join b c))
  join_idem : ∀ a, equiv (join a a) a

/-- Prove that Journal with merge is a JoinSemilatticeEquiv -/
instance : JoinSemilatticeEquiv Journal where
  join := merge
  equiv := Journal.equiv
  join_comm := merge_comm
  join_assoc := merge_assoc
  join_idem := merge_idem

/-- Deterministic reduction - placeholder for now -/
def reduce (facts : Journal) : Journal :=
  facts

theorem reduce_deterministic (facts : Journal) :
  reduce facts = reduce facts := rfl

end Aura.Journal
