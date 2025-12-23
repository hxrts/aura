/-!
# Journal CRDT Proofs

Proves that the journal forms a join-semilattice under merge, ensuring
replicas can merge in any order and converge to the same state.

## Quint Correspondence
- File: verification/quint/protocol_journal.qnt
- Section: INVARIANTS
- Properties: Journal merge forms a CRDT with set-union semantics

## Rust Correspondence
- File: crates/aura-journal/src/lib.rs
- Type: `Journal`, `Fact`, `FactId`
- Function: `merge` - combines journals via set union

## Expose

**Types**:
- `FactId`: Unique identifier for each fact
- `Fact`: Abstract fact representation
- `Journal`: List of facts (set semantics via membership equivalence)

**Operations** (stable):
- `merge`: Combine two journals via list concatenation
- `reduce`: Deterministically derive canonical state from facts

**Properties** (stable, theorem statements):
- `merge_comm`: Merge is commutative (membership-wise)
- `merge_assoc`: Merge is associative
- `merge_idem`: Merge is idempotent
- `equiv_refl`, `equiv_symm`, `equiv_trans`: Equivalence relation properties

**Internal helpers** (may change):
- `JoinSemilatticeEquiv`: Typeclass for semilattice up to equivalence
-/

namespace Aura.Journal

/-!
## Core Types

Fact identifiers and facts for journal membership.
-/

/-- Unique identifier for each fact.
    Rust: aura-journal/src/lib.rs::FactId -/
structure FactId where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Abstract fact representation.
    Real facts contain typed payloads; here we track identity only.
    Rust: aura-journal/src/lib.rs::Fact -/
structure Fact where
  id : FactId
  deriving BEq, Repr, DecidableEq

/-- Journal as a list of facts.
    We use List rather than Finset for pure Lean 4 compatibility.
    Rust: aura-journal/src/lib.rs::Journal -/
abbrev Journal := List Fact

/-!
## Equivalence Relation

Set-membership equivalence: two journals are equal if they contain the same facts.
This is the right notion for CRDTs where order doesn't matter, only presence.
-/

/-- Set-membership equivalence for journals. -/
def Journal.equiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1 ↔ f ∈ j2

/-- Infix notation for journal equivalence. -/
infix:50 " ≃ " => Journal.equiv

/-- Reflexivity: every journal is equivalent to itself. -/
theorem equiv_refl (j : Journal) : j ≃ j := fun _ => Iff.rfl

/-- Symmetry: if j1 ≃ j2 then j2 ≃ j1. -/
theorem equiv_symm {j1 j2 : Journal} (h : j1 ≃ j2) : j2 ≃ j1 :=
  fun f => (h f).symm

/-- Transitivity: if j1 ≃ j2 and j2 ≃ j3 then j1 ≃ j3. -/
theorem equiv_trans {j1 j2 j3 : Journal} (h12 : j1 ≃ j2) (h23 : j2 ≃ j3) : j1 ≃ j3 :=
  fun f => Iff.trans (h12 f) (h23 f)

/-!
## Merge Operation

Merge two journals via list concatenation.
Under membership equivalence, this behaves like set union.
-/

/-- Merge two journals (set union semantics).
    Quint: Journal merge accumulates facts from both sources -/
def merge (j1 j2 : Journal) : Journal := j1 ++ j2

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
def reduce (facts : Journal) : Journal := facts

/-!
## Claims Bundle

CRDT semilattice properties for journal merge.
-/

/-- Claims bundle for Journal CRDT properties. -/
structure JournalClaims where
  /-- Merge is commutative (membership-wise). -/
  merge_comm : ∀ j1 j2 : Journal, merge j1 j2 ≃ merge j2 j1

  /-- Merge is associative (membership-wise). -/
  merge_assoc : ∀ j1 j2 j3 : Journal,
    merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3)

  /-- Merge is idempotent. -/
  merge_idem : ∀ j : Journal, merge j j ≃ j

  /-- Reduce is idempotent: reducing twice equals reducing once. -/
  reduce_idem : ∀ j : Journal, reduce (reduce j) ≃ reduce j

  /-- Reduce preserves membership: facts in reduced are subset of original. -/
  reduce_subset : ∀ j : Journal, ∀ f, f ∈ reduce j → f ∈ j

  /-- Reduce respects equivalence: equivalent journals reduce to equivalent results. -/
  reduce_respects_equiv : ∀ j1 j2 : Journal, j1 ≃ j2 → reduce j1 ≃ reduce j2

  /-- Reduce commutes with merge (for monotone reductions). -/
  reduce_merge_comm : ∀ j1 j2 : Journal,
    reduce (merge j1 j2) ≃ merge (reduce j1) (reduce j2)

/-!
## Proofs

Individual theorem proofs that construct the claims bundle.
-/

/-- CRDT Law 1: Commutativity - merge(j1,j2) ≃ merge(j2,j1).
    Ensures replicas can merge in either order and get equivalent results. -/
theorem merge_comm (j1 j2 : Journal) : merge j1 j2 ≃ merge j2 j1 := by
  intro f
  simp only [merge, mem_append, or_comm]

/-- CRDT Law 2: Associativity - merge(merge(j1,j2),j3) ≃ merge(j1,merge(j2,j3)).
    Ensures three-way merges are order-independent. -/
theorem merge_assoc (j1 j2 j3 : Journal) :
    merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3) := by
  intro f
  simp only [merge, mem_append, or_assoc]

/-- CRDT Law 3: Idempotence - merge(j,j) ≃ j.
    Ensures merging the same state twice doesn't change anything. -/
theorem merge_idem (j : Journal) : merge j j ≃ j := by
  intro f
  simp only [merge, mem_append, or_self]

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

/-- Journal with merge is a join-semilattice. -/
instance : JoinSemilatticeEquiv Journal where
  join := merge
  equiv := Journal.equiv
  join_comm := merge_comm
  join_assoc := merge_assoc
  join_idem := merge_idem

/-!
## Reduction Proofs

Properties of the reduce function (defined above with merge).
-/

/-- Reduction is deterministic. -/
theorem reduce_deterministic (facts : Journal) :
  reduce facts = reduce facts := rfl

/-- Reduce is idempotent: applying reduce twice is the same as once.
    Critical for ensuring convergence to a stable canonical form. -/
theorem reduce_idem (j : Journal) : reduce (reduce j) ≃ reduce j := by
  unfold reduce
  exact equiv_refl j

/-- Reduce preserves membership: every fact in the reduced journal was in the original.
    Ensures reduction only removes or reorganizes, never adds facts. -/
theorem reduce_subset (j : Journal) : ∀ f, f ∈ reduce j → f ∈ j := by
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
theorem reduce_merge_comm (j1 j2 : Journal) :
    reduce (merge j1 j2) ≃ merge (reduce j1) (reduce j2) := by
  unfold reduce merge
  exact equiv_refl (j1 ++ j2)

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving Journal is a CRDT. -/
def journalClaims : JournalClaims where
  merge_comm := merge_comm
  merge_assoc := merge_assoc
  merge_idem := merge_idem
  reduce_idem := reduce_idem
  reduce_subset := reduce_subset
  reduce_respects_equiv := reduce_respects_equiv
  reduce_merge_comm := reduce_merge_comm

end Aura.Journal
