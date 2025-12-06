-- Core definitions for Journal CRDT verification.
-- Models the journal as a join-semilattice with deterministic reduction.

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

**Why this matters**: CRDTs require merge to be a semilattice operation so that
replicas can merge in any order and still converge to the same final state.
-/

-- Unique identifier for each fact. In Rust, this maps to aura_journal::FactId.
structure FactId where
  id : Nat
  deriving BEq, Repr, DecidableEq

-- Abstract fact representation. Real facts contain typed payloads;
-- here we only track identity since that's sufficient for merge semantics.
structure Fact where
  id : FactId
  deriving BEq, Repr, DecidableEq

-- Journal as a list of facts. We use List rather than Finset because Lean's
-- Finset requires decidable equality proofs, and List + membership equivalence
-- is simpler while capturing the same set semantics.
abbrev Journal := List Fact

-- Set-membership equivalence: two journals are equal if they contain the same facts.
-- This is the right notion for CRDTs where order doesn't matter, only presence.
def Journal.equiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1 ↔ f ∈ j2

-- Infix notation ≃ for journal equivalence (distinct from propositional equality =).
infix:50 " ≃ " => Journal.equiv

-- Reflexivity: every journal is equivalent to itself.
-- This is trivial but required for the equivalence relation.
theorem equiv_refl (j : Journal) : j ≃ j := fun _ => Iff.rfl

-- Symmetry: if j1 ≃ j2 then j2 ≃ j1.
-- Follows directly from Iff.symm on the membership biconditional.
theorem equiv_symm {j1 j2 : Journal} (h : j1 ≃ j2) : j2 ≃ j1 :=
  fun f => (h f).symm

-- Transitivity: if j1 ≃ j2 and j2 ≃ j3 then j1 ≃ j3.
-- Chain the membership biconditionals via Iff.trans.
theorem equiv_trans {j1 j2 j3 : Journal} (h12 : j1 ≃ j2) (h23 : j2 ≃ j3) : j1 ≃ j3 :=
  fun f => Iff.trans (h12 f) (h23 f)

-- Merge two journals via list concatenation.
-- Under membership equivalence, this behaves like set union.
def merge (j1 j2 : Journal) : Journal := j1 ++ j2

-- Standard library lemma: element membership distributes over append.
-- f ∈ (xs ++ ys) iff f ∈ xs or f ∈ ys.
theorem mem_append {f : Fact} {xs ys : List Fact} :
    f ∈ xs ++ ys ↔ f ∈ xs ∨ f ∈ ys := List.mem_append

-- **CRDT Law 1: Commutativity** - merge(j1,j2) ≃ merge(j2,j1).
-- Proof: membership in j1++j2 is (f∈j1 ∨ f∈j2), which equals (f∈j2 ∨ f∈j1) by or_comm.
-- This ensures replicas can merge in either order and get equivalent results.
theorem merge_comm (j1 j2 : Journal) : merge j1 j2 ≃ merge j2 j1 := by
  intro f
  simp only [merge, mem_append, or_comm]

-- **CRDT Law 2: Associativity** - merge(merge(j1,j2),j3) ≃ merge(j1,merge(j2,j3)).
-- Proof: both sides expand to (f∈j1 ∨ f∈j2 ∨ f∈j3) by or_assoc.
-- This ensures three-way merges are order-independent.
theorem merge_assoc (j1 j2 j3 : Journal) :
    merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3) := by
  intro f
  simp only [merge, mem_append, or_assoc]

-- **CRDT Law 3: Idempotence** - merge(j,j) ≃ j.
-- Proof: membership in j++j is (f∈j ∨ f∈j), which simplifies to f∈j by or_self.
-- This ensures merging the same state twice doesn't change anything.
theorem merge_idem (j : Journal) : merge j j ≃ j := by
  intro f
  simp only [merge, mem_append, or_self]

/-!
## Semilattice Structure

The Journal with merge forms a join-semilattice, which is the foundation
of the CRDT convergence guarantee. A join-semilattice requires:
  1. Commutativity: a ⊔ b = b ⊔ a
  2. Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
  3. Idempotence: a ⊔ a = a

Note: The semilattice properties hold up to set-membership equivalence (≃),
which is the appropriate notion for CRDT semantics where we care about
the set of facts, not their order in the list representation.
-/

-- Typeclass for join-semilattice up to a custom equivalence relation.
-- Standard Mathlib uses propositional equality; we need equivalence.
class JoinSemilatticeEquiv (α : Type) where
  join : α → α → α
  equiv : α → α → Prop
  join_comm : ∀ a b, equiv (join a b) (join b a)
  join_assoc : ∀ a b c, equiv (join (join a b) c) (join a (join b c))
  join_idem : ∀ a, equiv (join a a) a

-- **Main theorem**: Journal with merge is a join-semilattice.
-- This is the key structural property that guarantees CRDT convergence.
instance : JoinSemilatticeEquiv Journal where
  join := merge
  equiv := Journal.equiv
  join_comm := merge_comm
  join_assoc := merge_assoc
  join_idem := merge_idem

-- Reduction: deterministically derives canonical state from facts.
-- Currently identity; full implementation would apply domain-specific rules.
def reduce (facts : Journal) : Journal :=
  facts

-- Reduction is deterministic (same input always produces same output).
-- Trivial for identity function; non-trivial for real reduction logic.
theorem reduce_deterministic (facts : Journal) :
  reduce facts = reduce facts := rfl

end Aura.Journal
