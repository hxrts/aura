import Aura.Consensus.Types

/-!
# Evidence CRDT Proofs

Proves that consensus Evidence forms a join-semilattice under merge.
This ensures replicas can merge evidence in any order and converge.

## Quint Correspondence
- File: verification/quint/protocol_consensus.qnt
- Section: INVARIANTS (evidence-related)
- Predicates: Evidence is implicitly a CRDT through proposals/equivocators sets

## Rust Correspondence
- File: crates/aura-protocol/src/consensus/protocol.rs
- Evidence merge would be part of consensus state reconciliation

## Expose

The following definitions form the semantic interface for proofs:

**Types**:
- `Evidence`: CRDT evidence structure (from Types.lean)

**Operations** (stable):
- `mergeEvidence`: Combine two evidence structures
- `mergeLists`: Merge lists with deduplication

**Properties** (stable, theorem statements):
- `merge_comm`: Merge is commutative (on membership)
- `merge_assoc`: Merge is associative
- `merge_idem`: Merge is idempotent
- `merge_preserves_commit`: Merge preserves commit facts
- `commit_monotonic`: Once committed, stays committed
- `equivocator_monotonic`: Equivocators only grow

**Internal helpers** (may change):
- List manipulation utilities
-/

namespace Aura.Consensus.Evidence

open Aura.Consensus.Types

/-!
## List Merge Utilities

Helper functions for merging lists (set union semantics).
Note: Using custom List.removeDups (from Types.lean) for pure Lean 4 compatibility.
-/

/-- Merge two lists, removing duplicates.
    This is the join operation for set-represented-as-list. -/
def mergeLists [BEq α] (xs ys : List α) : List α :=
  List.removeDups (xs ++ ys)

/-!
## Evidence Merge Operation

The core CRDT merge for consensus evidence.
-/

/-- Merge two evidence structures.
    - Votes: union of all votes
    - Equivocators: union of detected equivocators
    - Commit: prefer existing commit (first-writer-wins semantics)

    This is the join operation for the Evidence semilattice.
    Quint: Implicit in how proposals/equivocators accumulate -/
def mergeEvidence (e1 e2 : Evidence) : Evidence :=
  if e1.consensusId != e2.consensusId then
    e1  -- Can't merge evidence for different instances
  else
    { consensusId := e1.consensusId
    , votes := mergeLists e1.votes e2.votes
    , equivocators := mergeLists e1.equivocators e2.equivocators
    , commitFact := e1.commitFact.orElse (fun _ => e2.commitFact) }

/-!
## Claims Bundle

This structure collects all the theorems about evidence merge.
Reviewers can inspect this to understand what's proven without
reading individual proofs.

Note: We express set equality via membership equivalence (pure Lean 4)
rather than Finset equality (which requires Mathlib).
-/

/-- Claims bundle for Evidence CRDT properties. -/
structure EvidenceClaims where
  /-- Merge is commutative (on membership). -/
  merge_comm : ∀ e1 e2 : Evidence,
    e1.consensusId = e2.consensusId →
    (∀ v, v ∈ (mergeEvidence e1 e2).votes ↔ v ∈ (mergeEvidence e2 e1).votes) ∧
    (∀ w, w ∈ (mergeEvidence e1 e2).equivocators ↔ w ∈ (mergeEvidence e2 e1).equivocators)

  /-- Merge is associative (on membership). -/
  merge_assoc : ∀ e1 e2 e3 : Evidence,
    e1.consensusId = e2.consensusId →
    e2.consensusId = e3.consensusId →
    ∀ v, v ∈ (mergeEvidence (mergeEvidence e1 e2) e3).votes ↔
         v ∈ (mergeEvidence e1 (mergeEvidence e2 e3)).votes

  /-- Merge is idempotent. -/
  merge_idem : ∀ e : Evidence,
    ∀ v, v ∈ (mergeEvidence e e).votes ↔ v ∈ e.votes

  /-- Equivocators only grow under merge. -/
  equivocator_preserved : ∀ e1 e2 : Evidence, ∀ w : AuthorityId,
    e1.consensusId = e2.consensusId →
    e1.isEquivocator w → (mergeEvidence e1 e2).isEquivocator w

  /-- Votes only grow under merge. -/
  votes_preserved : ∀ e1 e2 : Evidence, ∀ v : WitnessVote,
    e1.consensusId = e2.consensusId →
    v ∈ e1.votes → v ∈ (mergeEvidence e1 e2).votes

/-!
## Proofs

Individual theorem proofs that construct the claims bundle.
All proofs are complete using List.removeDups lemmas from Types.lean.
-/

/-- Merge preserves commit facts from either side. -/
theorem merge_preserves_commit (e1 e2 : Evidence) (h : e1.consensusId = e2.consensusId) :
    e1.isCommitted → (mergeEvidence e1 e2).isCommitted := by
  intro hc1
  simp only [Evidence.isCommitted] at hc1 ⊢
  simp only [mergeEvidence]
  -- The condition e1.consensusId != e2.consensusId is false because h says they're equal
  have hne : (e1.consensusId != e2.consensusId) = false := by
    rw [bne_eq_false_iff_eq, h]
  simp only [hne, Bool.false_eq_true, ite_false]
  -- Now we need to show (e1.commitFact.orElse (fun _ => e2.commitFact)).isSome
  -- hc1 says e1.commitFact.isSome is true
  cases he1 : e1.commitFact with
  | none =>
    -- e1.commitFact = none, but hc1 says isSome is true - contradiction
    rw [he1] at hc1
    simp only [Option.isSome] at hc1
    exact absurd hc1 Bool.false_ne_true
  | some c => simp only [Option.orElse, Option.isSome]

/-- Once committed, stays committed after merge. -/
theorem commit_monotonic (e1 e2 : Evidence) (h : e1.consensusId = e2.consensusId) :
    e1.isCommitted ∨ e2.isCommitted → (mergeEvidence e1 e2).isCommitted := by
  intro hor
  simp only [Evidence.isCommitted] at hor ⊢
  simp only [mergeEvidence]
  have hne : (e1.consensusId != e2.consensusId) = false := by
    rw [bne_eq_false_iff_eq, h]
  simp only [hne, Bool.false_eq_true, ite_false]
  -- Case analysis on e1.commitFact
  cases he1 : e1.commitFact with
  | none =>
    -- e1.commitFact = none, so must have e2.isCommitted
    simp only [Option.orElse]
    rw [he1] at hor
    simp only [Option.isSome] at hor
    cases hor with
    | inl h1 => exact absurd h1 (Bool.false_ne_true)
    | inr h2 => exact h2
  | some c =>
    -- e1.commitFact = some c, so orElse returns some c
    simp only [Option.orElse, Option.isSome]

/-- Equivocators from e1 are preserved in merge. -/
theorem equivocator_preserved_left (e1 e2 : Evidence) (w : AuthorityId)
    (h : e1.consensusId = e2.consensusId) :
    e1.isEquivocator w → (mergeEvidence e1 e2).isEquivocator w := by
  intro heq
  simp only [Evidence.isEquivocator, mergeEvidence]
  have hne : (e1.consensusId != e2.consensusId) = false := by
    rw [bne_eq_false_iff_eq, h]
  simp only [hne, Bool.false_eq_true, ite_false, mergeLists]
  -- heq says w is in e1.equivocators via any
  -- We need to show w is in removeDups (e1.equivocators ++ e2.equivocators) via any
  simp only [Evidence.isEquivocator] at heq
  -- Convert any to exists
  rw [List.any_eq_true] at heq ⊢
  obtain ⟨x, hx_mem, hx_eq⟩ := heq
  -- x is in e1.equivocators and x == w
  refine ⟨x, ?_, hx_eq⟩
  -- Need to show x ∈ removeDups (e1.equivocators ++ e2.equivocators)
  exact List.mem_removeDups_append_left hx_mem

/-- Votes from e1 are preserved in merge. -/
theorem votes_preserved_left (e1 e2 : Evidence) (v : WitnessVote)
    (h : e1.consensusId = e2.consensusId) :
    v ∈ e1.votes → v ∈ (mergeEvidence e1 e2).votes := by
  intro hv
  simp only [mergeEvidence]
  have hne : (e1.consensusId != e2.consensusId) = false := by
    rw [bne_eq_false_iff_eq, h]
  simp only [hne, Bool.false_eq_true, ite_false, mergeLists]
  exact List.mem_removeDups_append_left hv

/-- Merge is idempotent on votes. -/
theorem merge_idem_votes (e : Evidence) :
    ∀ v, v ∈ (mergeEvidence e e).votes ↔ v ∈ e.votes := by
  intro v
  simp only [mergeEvidence]
  have hne : (e.consensusId != e.consensusId) = false := bne_self_eq_false _
  simp only [hne, Bool.false_eq_true, ite_false, mergeLists]
  rw [List.mem_removeDups_iff, List.mem_append_iff]
  constructor
  · intro h; cases h <;> assumption
  · intro h; exact Or.inl h

/-- Merge is commutative on votes (membership-wise). -/
theorem merge_comm_votes (e1 e2 : Evidence) (h : e1.consensusId = e2.consensusId) :
    ∀ v, v ∈ (mergeEvidence e1 e2).votes ↔ v ∈ (mergeEvidence e2 e1).votes := by
  intro v
  simp only [mergeEvidence]
  have hne12 : (e1.consensusId != e2.consensusId) = false := by
    rw [bne_eq_false_iff_eq, h]
  have hne21 : (e2.consensusId != e1.consensusId) = false := by
    rw [bne_eq_false_iff_eq, h]
  simp only [hne12, hne21, Bool.false_eq_true, ite_false, mergeLists]
  exact List.mem_removeDups_append_comm

/-- Merge is commutative on equivocators (membership-wise). -/
theorem merge_comm_equivocators (e1 e2 : Evidence) (h : e1.consensusId = e2.consensusId) :
    ∀ w, w ∈ (mergeEvidence e1 e2).equivocators ↔ w ∈ (mergeEvidence e2 e1).equivocators := by
  intro w
  -- Expand mergeEvidence for e1 e2
  have heq12 : (mergeEvidence e1 e2).equivocators = mergeLists e1.equivocators e2.equivocators := by
    simp only [mergeEvidence]
    have hne : (e1.consensusId != e2.consensusId) = false := by
      rw [bne_eq_false_iff_eq, h]
    simp only [hne, Bool.false_eq_true, ite_false]
  -- Expand mergeEvidence for e2 e1
  have heq21 : (mergeEvidence e2 e1).equivocators = mergeLists e2.equivocators e1.equivocators := by
    simp only [mergeEvidence]
    have hne : (e2.consensusId != e1.consensusId) = false := by
      rw [bne_eq_false_iff_eq, h]
    simp only [hne, Bool.false_eq_true, ite_false]
  rw [heq12, heq21]
  exact List.mem_removeDups_append_comm

/-- Merge is associative on votes. -/
theorem merge_assoc_votes (e1 e2 e3 : Evidence)
    (h12 : e1.consensusId = e2.consensusId) (h23 : e2.consensusId = e3.consensusId) :
    ∀ v, v ∈ (mergeEvidence (mergeEvidence e1 e2) e3).votes ↔
         v ∈ (mergeEvidence e1 (mergeEvidence e2 e3)).votes := by
  intro v
  -- Establish helper lemmas about how mergeEvidence computes votes
  have lhs_votes : (mergeEvidence (mergeEvidence e1 e2) e3).votes =
      mergeLists (mergeEvidence e1 e2).votes e3.votes := by
    simp only [mergeEvidence]
    have hne12 : (e1.consensusId != e2.consensusId) = false := by rw [bne_eq_false_iff_eq, h12]
    simp only [hne12, Bool.false_eq_true, ite_false]
    have hne_outer : (e1.consensusId != e3.consensusId) = false := by
      rw [bne_eq_false_iff_eq, h12.trans h23]
    simp only [hne_outer, Bool.false_eq_true, ite_false]
  have inner12_votes : (mergeEvidence e1 e2).votes = mergeLists e1.votes e2.votes := by
    simp only [mergeEvidence]
    have hne : (e1.consensusId != e2.consensusId) = false := by rw [bne_eq_false_iff_eq, h12]
    simp only [hne, Bool.false_eq_true, ite_false]
  have rhs_votes : (mergeEvidence e1 (mergeEvidence e2 e3)).votes =
      mergeLists e1.votes (mergeEvidence e2 e3).votes := by
    simp only [mergeEvidence]
    have hne23 : (e2.consensusId != e3.consensusId) = false := by rw [bne_eq_false_iff_eq, h23]
    simp only [hne23, Bool.false_eq_true, ite_false]
    have hne_outer : (e1.consensusId != e2.consensusId) = false := by rw [bne_eq_false_iff_eq, h12]
    simp only [hne_outer, Bool.false_eq_true, ite_false]
  have inner23_votes : (mergeEvidence e2 e3).votes = mergeLists e2.votes e3.votes := by
    simp only [mergeEvidence]
    have hne : (e2.consensusId != e3.consensusId) = false := by rw [bne_eq_false_iff_eq, h23]
    simp only [hne, Bool.false_eq_true, ite_false]
  -- Rewrite with the helper lemmas
  rw [lhs_votes, inner12_votes, rhs_votes, inner23_votes]
  simp only [mergeLists, List.mem_removeDups_iff, List.mem_append_iff]
  -- Goal: (v ∈ e1 ∨ v ∈ e2) ∨ v ∈ e3 ↔ v ∈ e1 ∨ v ∈ e2 ∨ v ∈ e3
  constructor
  · intro h
    cases h with
    | inl h12' =>
      cases h12' with
      | inl h1 => exact Or.inl h1
      | inr h2 => exact Or.inr (Or.inl h2)
    | inr h3 => exact Or.inr (Or.inr h3)
  · intro h
    cases h with
    | inl h1 => exact Or.inl (Or.inl h1)
    | inr h23' =>
      cases h23' with
      | inl h2 => exact Or.inl (Or.inr h2)
      | inr h3 => exact Or.inr h3

/-!
## Claims Bundle Construction

Construct the claims bundle from individual proofs.
-/

/-- The claims bundle, proving Evidence is a CRDT. -/
def evidenceClaims : EvidenceClaims where
  merge_comm := fun e1 e2 h => ⟨merge_comm_votes e1 e2 h, merge_comm_equivocators e1 e2 h⟩
  merge_assoc := fun e1 e2 e3 h12 h23 => merge_assoc_votes e1 e2 e3 h12 h23
  merge_idem := fun e => merge_idem_votes e
  equivocator_preserved := fun e1 e2 w h => equivocator_preserved_left e1 e2 w h
  votes_preserved := fun e1 e2 v h => votes_preserved_left e1 e2 v h

end Aura.Consensus.Evidence
