import Aura.Domain.Journal.Types
import Aura.Domain.Journal.Operations

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
- Function: `join` - combines journals via set union (same namespace only)

## Expose

**Properties** (theorem statements):
- `merge_comm`: Merge is commutative (membership-wise)
- `merge_assoc`: Merge is associative
- `merge_idem`: Merge is idempotent
- `merge_same_namespace`: Merge preserves namespace
- `equiv_refl`, `equiv_symm`, `equiv_trans`: Equivalence relation properties
- `reduce_idem`, `reduce_subset`, `reduce_respects_equiv`, `reduce_merge_comm`

**Claims Bundle**:
- `JournalClaims`: All CRDT properties bundled
- `journalClaims`: The constructed claims bundle
-/

namespace Aura.Proofs.Journal

open Aura.Domain.Journal

/-!
## Equivalence Relation Proofs
-/

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
## Merge Proofs
-/

/-- CRDT Law 1: Commutativity - merge(j1,j2).facts ≃ merge(j2,j1).facts.
    Ensures replicas can merge in either order and get equivalent fact sets.
    Note: Namespace equality is required for full journal equivalence, but
    factsEquiv only compares fact membership which is namespace-independent. -/
theorem merge_comm (j1 j2 : Journal) (_ : j1.ns = j2.ns) :
    (merge j1 j2).factsEquiv (merge j2 j1) := by
  intro f
  simp only [merge, mem_append, or_comm]

/-- Unconditional merge_comm for factsEquiv.
    Since factsEquiv only compares facts (not namespace), namespace equality
    is not required for this property to hold. -/
theorem merge_comm' (j1 j2 : Journal) :
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

/-- Unconditional merge_assoc for factsEquiv. -/
theorem merge_assoc' (j1 j2 j3 : Journal) :
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
## Semilattice Instance
-/

/-- For the semilattice instance, we use facts-only equivalence.
    This is the appropriate equivalence for CRDT semantics where we care
    about fact membership, not namespace metadata. Namespace consistency
    is enforced by the runtime (via merge_safe or assertions). -/
instance : JoinSemilatticeEquiv Journal where
  join := merge
  equiv := Journal.factsEquiv
  join_comm := merge_comm'
  join_assoc := merge_assoc'
  join_idem := merge_idem

/-!
## Reduction Proofs
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

end Aura.Proofs.Journal
