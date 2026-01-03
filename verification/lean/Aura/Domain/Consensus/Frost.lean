/-!
# FROST Protocol Types and Operations

Types and operations for FROST threshold signature orchestration.

## Quint Correspondence
- File: verification/quint/protocol_frost.qnt
- Section: TYPE DEFINITIONS
- Types: SessionId, Round, Share, AggregatorState

## Rust Correspondence
- File: crates/aura-core/src/crypto/tree_signing.rs
- Types: `SigningSession`, `Share`
- Function: `aggregate` - combines threshold shares into signature

## Expose

**Types** (stable):
- `SessionId`: Groups shares from one signing request
- `Round`: Round within a session (commitment, signing)
- `WitnessId`: Threshold participant identifier
- `ShareData`: Abstract share value
- `Share`: Signature share with session/round/witness tagging
- `AggregatorState`: Collects shares until threshold
- `Signature`: Abstract signature result

**Operations** (stable):
- `canAggregate`: Check if shares are from same session/round
- `aggregate`: Combine shares into signature (if valid)
-/

namespace Aura.Domain.Consensus.Frost

/-!
## Core Types

FROST protocol data structures.
-/

/-- Session identifier: groups shares from one signing request.
    Rust: aura-core/src/crypto/tree_signing.rs -/
structure SessionId where
  id : Nat
  deriving Repr, DecidableEq

/-- Manual BEq for SessionId (unfoldable for proofs). -/
instance : BEq SessionId where
  beq a b := a.id == b.id

/-- Round within a session (commitment, signing, etc.).
    Rust: Corresponds to FROST protocol rounds -/
structure Round where
  idx : Nat
  deriving Repr, DecidableEq

/-- Manual BEq for Round. -/
instance : BEq Round where
  beq a b := a.idx == b.idx

/-- Witness identifier: which threshold participant produced this share.
    Rust: Maps to devices in commitment tree -/
structure WitnessId where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Abstract share data (actual cryptographic share value).
    Rust: Schnorr signature share -/
structure ShareData where
  value : Nat
  deriving BEq, Repr

/-- A signature share: one participant's contribution.
    Rust: aura-core/src/crypto/tree_signing.rs::Share -/
structure Share where
  sid : SessionId
  round : Round
  witness : WitnessId
  data : ShareData
  deriving BEq, Repr

/-- Aggregator state: collects shares until threshold.
    Rust: Part of FrostOrchestrator state -/
structure AggregatorState where
  pending : List Share
  deriving BEq, Repr

/-- Abstract signature result.
    Rust: Schnorr signature verifying against group public key -/
structure Signature where
  value : Nat
  deriving BEq, Repr

/-!
## Aggregation Operations

Check and perform share aggregation.
-/

/-- Check if aggregation is safe: all shares from same session/round.
    Quint: Precondition for aggregateShares action -/
def canAggregate (state : AggregatorState) : Bool :=
  match state.pending with
  | [] => false
  | sh :: tl =>
    tl.all (fun sh' => sh'.sid == sh.sid && sh'.round == sh.round)

/-- Aggregate shares into a signature (if valid).
    Quint: Only succeeds when canAggregate returns true -/
def aggregate (state : AggregatorState) : Option Signature :=
  if canAggregate state then
    some { value := 0 }
  else
    none

end Aura.Domain.Consensus.Frost
