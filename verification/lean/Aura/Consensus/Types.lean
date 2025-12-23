import Aura.Assumptions

/-!
# Consensus Types

Core data structures for Aura Consensus verification. These types model
the protocol state and messages for threshold agreement.

## Quint Correspondence
- File: verification/quint/protocol_consensus.qnt
- Section: TYPES

## Rust Correspondence
- File: crates/aura-protocol/src/consensus/types.rs
- File: crates/aura-protocol/src/consensus/messages.rs

## Expose

The following definitions form the semantic interface for proofs:

**Types** (stable, used in theorem statements):
- `ConsensusId`: Unique identifier for a consensus instance
- `AuthorityId`: Identifier for a witness/authority
- `PrestateHash`: Hash binding operation to prestate
- `ResultId`: Identifier for proposed result
- `ShareData`: Threshold signature share
- `CommitFact`: Committed consensus result with threshold signature
- `Evidence`: CRDT-mergeable evidence for a consensus instance
- `EquivocationProof`: Proof that a witness signed conflicting values
- `WitnessVote`: A witness's vote on a consensus instance
- `ConsensusPhase`: Protocol phase enumeration

**Predicates** (stable, used in theorem statements):
- `detectEquivocation`: Detect conflicting votes from same witness
- `Evidence.isCommitted`: Check if evidence contains a commit
- `Evidence.hasVoteFrom`: Check if witness voted in evidence

**Internal helpers** (may change):
- `Hash32.value`: Raw hash bytes (implementation detail)
- `ConsensusId.mk`, `AuthorityId.mk`: Constructors

Changing exposed definitions requires updating Quint specs and Rust implementation.
-/

namespace Aura.Consensus.Types

/-!
## Utility Functions

Helper functions for list operations (pure Lean 4, no Mathlib).
-/

/-- Remove duplicates from a list using BEq.
    This is a pure Lean 4 replacement for Mathlib's eraseDups. -/
def List.removeDups [BEq α] : List α → List α
  | [] => []
  | x :: xs => if xs.elem x then List.removeDups xs else x :: List.removeDups xs

/-- Membership in removeDups implies membership in original list. -/
theorem List.mem_removeDups_of_mem [BEq α] [LawfulBEq α] {a : α} {l : List α} :
    a ∈ List.removeDups l → a ∈ l := by
  intro h
  induction l with
  | nil => cases h
  | cons x xs ih =>
    simp only [removeDups] at h
    split at h
    · exact List.Mem.tail x (ih h)
    · cases h with
      | head => exact List.Mem.head xs
      | tail _ ht => exact List.Mem.tail x (ih ht)

/-- Membership in original list implies membership in removeDups. -/
theorem List.mem_removeDups_of_mem' [BEq α] [LawfulBEq α] {a : α} {l : List α} :
    a ∈ l → a ∈ List.removeDups l := by
  intro h
  induction l with
  | nil => cases h
  | cons x xs ih =>
    simp only [removeDups]
    cases h with
    | head =>
      -- In head case, a = x
      split
      · -- xs.elem x is true, so x ∈ xs
        rename_i helem
        have hx_in_xs : a ∈ xs := List.elem_iff.mp helem
        exact ih hx_in_xs
      · exact List.Mem.head _
    | tail _ ht =>
      split
      · exact ih ht
      · exact List.Mem.tail _ (ih ht)

/-- Membership equivalence for removeDups. -/
theorem List.mem_removeDups_iff [BEq α] [LawfulBEq α] {a : α} {l : List α} :
    a ∈ List.removeDups l ↔ a ∈ l :=
  ⟨mem_removeDups_of_mem, mem_removeDups_of_mem'⟩

/-- Membership in append. -/
theorem List.mem_append_iff {a : α} {xs ys : List α} :
    a ∈ xs ++ ys ↔ a ∈ xs ∨ a ∈ ys := List.mem_append

/-- Membership in removeDups of append is equivalent to membership in either list. -/
theorem List.mem_removeDups_append [BEq α] [LawfulBEq α] {a : α} {xs ys : List α} :
    a ∈ List.removeDups (xs ++ ys) ↔ a ∈ xs ∨ a ∈ ys := by
  rw [mem_removeDups_iff, mem_append_iff]

/-- removeDups of append is commutative on membership. -/
theorem List.mem_removeDups_append_comm [BEq α] [LawfulBEq α] {a : α} {xs ys : List α} :
    a ∈ List.removeDups (xs ++ ys) ↔ a ∈ List.removeDups (ys ++ xs) := by
  simp only [mem_removeDups_append]
  constructor <;> (intro h; cases h <;> (first | exact Or.inl ‹_› | exact Or.inr ‹_›))

/-- removeDups is idempotent on membership. -/
theorem List.mem_removeDups_removeDups [BEq α] [LawfulBEq α] {a : α} {l : List α} :
    a ∈ List.removeDups (List.removeDups l) ↔ a ∈ List.removeDups l := by
  simp only [mem_removeDups_iff]

/-- Membership preserved from left list in removeDups append. -/
theorem List.mem_removeDups_append_left [BEq α] [LawfulBEq α] {a : α} {xs ys : List α} :
    a ∈ xs → a ∈ List.removeDups (xs ++ ys) := by
  intro h
  rw [mem_removeDups_append]
  exact Or.inl h

/-!
## Core Identifier Types

These correspond to Quint `str` types and Rust newtype wrappers.
-/

/-- Unique identifier for a consensus instance.
    Rust: aura-protocol/src/consensus/types.rs::ConsensusId
    Quint: protocol_consensus.qnt::ConsensusId -/
structure ConsensusId where
  value : String
  deriving BEq, Repr, DecidableEq, Hashable

instance : LawfulBEq ConsensusId where
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

/-- Identifier for a witness (threshold signing participant).
    Rust: aura-core/src/domain/types.rs::AuthorityId
    Quint: protocol_consensus.qnt::AuthorityId -/
structure AuthorityId where
  value : String
  deriving BEq, Repr, DecidableEq, Hashable

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

/-- Hash of prestate for deterministic binding.
    Rust: aura-protocol/src/consensus/types.rs (PrestateHash)
    Quint: protocol_consensus.qnt::PrestateHash -/
structure PrestateHash where
  value : String
  deriving BEq, Repr, DecidableEq, Hashable

instance : LawfulBEq PrestateHash where
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

/-- Identifier for a proposed result value.
    Rust: aura-protocol/src/consensus/types.rs (ResultId)
    Quint: protocol_consensus.qnt::ResultId -/
structure ResultId where
  value : String
  deriving BEq, Repr, DecidableEq, Hashable

instance : LawfulBEq ResultId where
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

/-!
## Protocol Phase

Models the state machine phases from the Quint specification.
-/

/-- Protocol phase enumeration.
    Rust: aura-protocol/src/consensus/messages.rs::ConsensusPhase
    Quint: protocol_consensus.qnt::ConsensusPhase -/
inductive ConsensusPhase where
  | Pending      : ConsensusPhase  -- Waiting to start
  | FastPathActive : ConsensusPhase  -- Fast path with cached nonces
  | FallbackActive : ConsensusPhase  -- Slow path fallback
  | Committed    : ConsensusPhase  -- Successfully committed
  | Failed       : ConsensusPhase  -- Failed to reach consensus
  deriving BEq, Repr, DecidableEq

/-!
## Signature and Vote Types

These model the threshold signature shares and witness votes.
-/

/-- Abstract signature share data.
    Rust: Uses frost-secp256k1 SignatureShare
    Quint: protocol_consensus.qnt::ShareData -/
structure ShareData where
  shareValue : String      -- Abstract share value
  nonceBinding : String    -- Nonce commitment binding
  dataBinding : String     -- Data binding (cid, rid, pHash)
  deriving Repr, DecidableEq

/-- Explicit BEq for ShareData (unfoldable). -/
instance : BEq ShareData where
  beq a b := decide (a = b)

instance : LawfulBEq ShareData where
  eq_of_beq {a b} h := of_decide_eq_true h
  rfl {a} := decide_eq_true rfl

/-- A witness's vote on a consensus instance.
    Rust: aura-protocol/src/consensus/types.rs::WitnessVote
    Quint: protocol_consensus.qnt::ShareProposal -/
structure WitnessVote where
  witness : AuthorityId
  consensusId : ConsensusId
  resultId : ResultId
  prestateHash : PrestateHash
  share : ShareData
  deriving Repr, DecidableEq

/-- Explicit BEq for WitnessVote (unfoldable). -/
instance : BEq WitnessVote where
  beq a b := decide (a = b)

instance : LawfulBEq WitnessVote where
  eq_of_beq {a b} h := of_decide_eq_true h
  rfl {a} := decide_eq_true rfl

/-- Aggregated threshold signature.
    Rust: Uses frost-secp256k1 Signature
    Quint: protocol_consensus.qnt::ThresholdSignature -/
structure ThresholdSignature where
  sigValue : String
  boundCid : ConsensusId
  boundRid : ResultId
  boundPHash : PrestateHash
  signerSet : List AuthorityId
  deriving BEq, Repr, DecidableEq

/-!
## Commit and Evidence Types

These are the key types for agreement proofs.
-/

/-- A committed consensus result with threshold signature.
    This is the output of successful consensus.
    Rust: aura-protocol/src/consensus/types.rs::CommitFact
    Quint: protocol_consensus.qnt::CommitFact -/
structure CommitFact where
  consensusId : ConsensusId
  resultId : ResultId
  prestateHash : PrestateHash
  signature : ThresholdSignature
  deriving BEq, Repr, DecidableEq

/-- Proof that a witness equivocated (signed conflicting values).
    Used for Byzantine fault detection.
    Rust: aura-protocol/src/consensus/types.rs::ConflictFact
    Quint: protocol_consensus_adversary.qnt (equivocators set) -/
structure EquivocationProof where
  witness : AuthorityId
  consensusId : ConsensusId
  vote1 : WitnessVote
  vote2 : WitnessVote
  conflicting : vote1.resultId ≠ vote2.resultId
  deriving Repr

/-- Evidence gathered during consensus (CRDT-mergeable).
    Forms a join-semilattice under merge.
    Rust: Would be part of consensus state
    Quint: protocol_consensus.qnt::ConsensusInstance (proposals, equivocators) -/
structure Evidence where
  consensusId : ConsensusId
  votes : List WitnessVote
  equivocators : List AuthorityId
  commitFact : Option CommitFact
  deriving BEq, Repr

/-!
## Exposed Predicates

These predicates are used in theorem statements and must be stable.
-/

/-- Detect equivocation: two votes from same witness with different results.
    Quint: protocol_consensus.qnt::witnessEquivocated -/
def detectEquivocation (v1 v2 : WitnessVote) : Option EquivocationProof :=
  if h : v1.witness = v2.witness ∧ v1.consensusId = v2.consensusId ∧ v1.resultId ≠ v2.resultId then
    some {
      witness := v1.witness
      consensusId := v1.consensusId
      vote1 := v1
      vote2 := v2
      conflicting := h.2.2
    }
  else
    none

/-- Check if evidence contains a successful commit.
    Quint: Checking `inst.commitFact != None` -/
def Evidence.isCommitted (e : Evidence) : Bool :=
  e.commitFact.isSome

/-- Check if a witness has voted in the evidence.
    Quint: hasProposal helper -/
def Evidence.hasVoteFrom (e : Evidence) (w : AuthorityId) : Bool :=
  e.votes.any (fun v => v.witness == w)

/-- Get all unique witnesses who voted.
    Quint: Derived from proposals set -/
def Evidence.voters (e : Evidence) : List AuthorityId :=
  List.removeDups (e.votes.map (·.witness))

/-- Count votes for a specific result.
    Quint: countProposalsForResult -/
def Evidence.votesForResult (e : Evidence) (rid : ResultId) : Nat :=
  e.votes.filter (fun v => v.resultId == rid) |>.length

/-- Check if a witness is marked as an equivocator.
    Quint: Checking membership in equivocators set -/
def Evidence.isEquivocator (e : Evidence) (w : AuthorityId) : Bool :=
  e.equivocators.any (fun eq => eq == w)

/-!
## Empty/Initial Values

Constructors for initial states.
-/

/-- Empty evidence for a new consensus instance. -/
def Evidence.empty (cid : ConsensusId) : Evidence :=
  { consensusId := cid
  , votes := []
  , equivocators := []
  , commitFact := none }

end Aura.Consensus.Types
