import Lean.Data.Json
import Aura.Types.ByteArray32
import Aura.Types.Identifiers
import Aura.Types.TimeStamp

/-! # Aura.Types.ProtocolFacts

Protocol-level relational facts (12 variants).

## Rust Correspondence
- File: crates/aura-journal/src/protocol_facts.rs
- Enum: `ProtocolRelationalFact` with 12 variants

## Expose

**Types** (stable):
- `ChannelCheckpoint`: AMP channel ratchet window anchor
- `ProposedChannelEpochBump`: Optimistic epoch bump proposal
- `CommittedChannelEpochBump`: Finalized epoch bump
- `ChannelPolicy`: Channel policy overrides
- `LeakageFact`: Privacy budget accounting
- `DkgTranscriptCommit`: Finalized DKG transcript
- `ConvergenceCert`: Coordinator convergence certificate
- `ReversionFact`: Explicit reversion marker
- `RotateFact`: Lifecycle transition marker
- `ProtocolRelationalFact`: Main enum with all 12 variants
-/

namespace Aura.Types.ProtocolFacts

open Lean (Json ToJson FromJson)
open Aura.Types.ByteArray32 (ByteArray32)
open Aura.Types.Identifiers (Hash32 AuthorityId ChannelId)
open Aura.Types.TimeStamp (PhysicalTime)

/-! ## Supporting Types -/

/-- AMP channel checkpoint anchoring ratchet windows.
    Rust: aura-journal/src/fact.rs::ChannelCheckpoint -/
structure ChannelCheckpoint where
  channel : ChannelId
  chan_epoch : Nat
  ck_commitment : Hash32
  deriving Repr, BEq

/-- Reason for channel epoch bump.
    Rust: aura-journal/src/fact.rs::ChannelBumpReason -/
inductive ChannelBumpReason where
  | regularRotation
  | keyCompromise
  | membershipChange
  deriving Repr, BEq, DecidableEq

/-- Proposed channel epoch bump (optimistic).
    Rust: aura-journal/src/fact.rs::ProposedChannelEpochBump -/
structure ProposedChannelEpochBump where
  channel : ChannelId
  parent_epoch : Nat
  new_epoch : Nat
  bump_id : Hash32
  reason : ChannelBumpReason
  deriving Repr, BEq

/-- Committed channel epoch bump (final).
    Rust: aura-journal/src/fact.rs::CommittedChannelEpochBump -/
structure CommittedChannelEpochBump where
  channel : ChannelId
  parent_epoch : Nat
  new_epoch : Nat
  chosen_bump_id : Hash32
  deriving Repr, BEq

/-- Channel policy overrides.
    Rust: aura-journal/src/fact.rs::ChannelPolicy -/
structure ChannelPolicy where
  channel : ChannelId
  max_message_size : Option Nat
  rate_limit_per_minute : Option Nat
  deriving Repr, BEq

/-- Leakage tracking event (privacy budget).
    Rust: aura-journal/src/fact.rs::LeakageFact -/
structure LeakageFact where
  source : AuthorityId
  destination : AuthorityId
  timestamp : PhysicalTime
  budget_consumed : Nat
  deriving Repr, BEq

/-- Finalized DKG transcript commit.
    Rust: aura-journal/src/fact.rs::DkgTranscriptCommit -/
structure DkgTranscriptCommit where
  transcript_hash : Hash32
  epoch : Nat
  deriving Repr, BEq

/-- Coordinator convergence certificate (soft-safe).
    Rust: aura-journal/src/fact.rs::ConvergenceCert -/
structure ConvergenceCert where
  op_id : Hash32
  coordinator : AuthorityId
  deriving Repr, BEq

/-- Lifecycle state for rotation facts.
    Rust: aura-journal/src/fact.rs::LifecycleState -/
inductive LifecycleState where
  | active
  | rotating
  | deprecated
  | revoked
  deriving Repr, BEq, DecidableEq

/-- Explicit reversion fact (soft-safe).
    Rust: aura-journal/src/fact.rs::ReversionFact -/
structure ReversionFact where
  op_id : Hash32
  reason : String
  deriving Repr, BEq

/-- Rotation/upgrade marker for lifecycle transitions.
    Rust: aura-journal/src/fact.rs::RotateFact -/
structure RotateFact where
  to_state : LifecycleState
  deriving Repr, BEq

/-! ## Main Protocol Fact Enum -/

/-- Protocol-level relational facts (12 variants).
    These participate in reduction semantics and cross-domain invariants.
    Rust: aura-journal/src/protocol_facts.rs::ProtocolRelationalFact -/
inductive ProtocolRelationalFact where
  /-- Guardian binding established between two authorities. -/
  | guardianBinding (account_id guardian_id : AuthorityId) (binding_hash : Hash32)
  /-- Recovery grant issued by a guardian. -/
  | recoveryGrant (account_id guardian_id : AuthorityId) (grant_hash : Hash32)
  /-- Consensus result from Aura Consensus. -/
  | consensus (consensus_id operation_hash : Hash32) (threshold_met : Bool) (participant_count : Nat)
  /-- AMP channel checkpoint. -/
  | ampChannelCheckpoint (checkpoint : ChannelCheckpoint)
  /-- Proposed channel epoch bump (optimistic). -/
  | ampProposedChannelEpochBump (bump : ProposedChannelEpochBump)
  /-- Committed channel epoch bump (final). -/
  | ampCommittedChannelEpochBump (bump : CommittedChannelEpochBump)
  /-- Channel policy overrides. -/
  | ampChannelPolicy (policy : ChannelPolicy)
  /-- Leakage tracking event. -/
  | leakageEvent (event : LeakageFact)
  /-- Finalized DKG transcript commit. -/
  | dkgTranscriptCommit (commit : DkgTranscriptCommit)
  /-- Coordinator convergence certificate. -/
  | convergenceCert (cert : ConvergenceCert)
  /-- Explicit reversion fact. -/
  | reversionFact (reversion : ReversionFact)
  /-- Rotation/upgrade marker. -/
  | rotateFact (rotate : RotateFact)
  deriving Repr, BEq

/-! ## JSON Serialization -/

-- Supporting type JSON instances
instance : ToJson ChannelBumpReason where
  toJson
    | .regularRotation => Json.str "regularRotation"
    | .keyCompromise => Json.str "keyCompromise"
    | .membershipChange => Json.str "membershipChange"

instance : FromJson ChannelBumpReason where
  fromJson? j := do
    let s ← j.getStr?
    match s with
    | "regularRotation" => pure .regularRotation
    | "keyCompromise" => pure .keyCompromise
    | "membershipChange" => pure .membershipChange
    | _ => throw s!"Unknown ChannelBumpReason: {s}"

instance : ToJson LifecycleState where
  toJson
    | .active => Json.str "active"
    | .rotating => Json.str "rotating"
    | .deprecated => Json.str "deprecated"
    | .revoked => Json.str "revoked"

instance : FromJson LifecycleState where
  fromJson? j := do
    let s ← j.getStr?
    match s with
    | "active" => pure .active
    | "rotating" => pure .rotating
    | "deprecated" => pure .deprecated
    | "revoked" => pure .revoked
    | _ => throw s!"Unknown LifecycleState: {s}"

instance : ToJson ChannelCheckpoint where
  toJson c := Json.mkObj [
    ("channel", ToJson.toJson c.channel),
    ("chan_epoch", Json.num c.chan_epoch),
    ("ck_commitment", ToJson.toJson c.ck_commitment)
  ]

instance : FromJson ChannelCheckpoint where
  fromJson? j := do
    let channel ← j.getObjValAs? ChannelId "channel"
    let chan_epoch ← j.getObjValAs? Nat "chan_epoch"
    let ck_commitment ← j.getObjValAs? Hash32 "ck_commitment"
    pure ⟨channel, chan_epoch, ck_commitment⟩

instance : ToJson ProposedChannelEpochBump where
  toJson b := Json.mkObj [
    ("channel", ToJson.toJson b.channel),
    ("parent_epoch", Json.num b.parent_epoch),
    ("new_epoch", Json.num b.new_epoch),
    ("bump_id", ToJson.toJson b.bump_id),
    ("reason", ToJson.toJson b.reason)
  ]

instance : FromJson ProposedChannelEpochBump where
  fromJson? j := do
    let channel ← j.getObjValAs? ChannelId "channel"
    let parent_epoch ← j.getObjValAs? Nat "parent_epoch"
    let new_epoch ← j.getObjValAs? Nat "new_epoch"
    let bump_id ← j.getObjValAs? Hash32 "bump_id"
    let reason ← j.getObjValAs? ChannelBumpReason "reason"
    pure ⟨channel, parent_epoch, new_epoch, bump_id, reason⟩

instance : ToJson CommittedChannelEpochBump where
  toJson b := Json.mkObj [
    ("channel", ToJson.toJson b.channel),
    ("parent_epoch", Json.num b.parent_epoch),
    ("new_epoch", Json.num b.new_epoch),
    ("chosen_bump_id", ToJson.toJson b.chosen_bump_id)
  ]

instance : FromJson CommittedChannelEpochBump where
  fromJson? j := do
    let channel ← j.getObjValAs? ChannelId "channel"
    let parent_epoch ← j.getObjValAs? Nat "parent_epoch"
    let new_epoch ← j.getObjValAs? Nat "new_epoch"
    let chosen_bump_id ← j.getObjValAs? Hash32 "chosen_bump_id"
    pure ⟨channel, parent_epoch, new_epoch, chosen_bump_id⟩

instance : ToJson ChannelPolicy where
  toJson p := Json.mkObj [
    ("channel", ToJson.toJson p.channel),
    ("max_message_size", match p.max_message_size with | some n => Json.num n | none => Json.null),
    ("rate_limit_per_minute", match p.rate_limit_per_minute with | some n => Json.num n | none => Json.null)
  ]

instance : FromJson ChannelPolicy where
  fromJson? j := do
    let channel ← j.getObjValAs? ChannelId "channel"
    let max_size ← match j.getObjVal? "max_message_size" with
      | .ok Json.null => pure none
      | .ok v => some <$> FromJson.fromJson? v
      | .error _ => pure none
    let rate_limit ← match j.getObjVal? "rate_limit_per_minute" with
      | .ok Json.null => pure none
      | .ok v => some <$> FromJson.fromJson? v
      | .error _ => pure none
    pure ⟨channel, max_size, rate_limit⟩

instance : ToJson LeakageFact where
  toJson l := Json.mkObj [
    ("source", ToJson.toJson l.source),
    ("destination", ToJson.toJson l.destination),
    ("timestamp", ToJson.toJson l.timestamp),
    ("budget_consumed", Json.num l.budget_consumed)
  ]

instance : FromJson LeakageFact where
  fromJson? j := do
    let source ← j.getObjValAs? AuthorityId "source"
    let destination ← j.getObjValAs? AuthorityId "destination"
    let timestamp ← j.getObjValAs? PhysicalTime "timestamp"
    let budget_consumed ← j.getObjValAs? Nat "budget_consumed"
    pure ⟨source, destination, timestamp, budget_consumed⟩

instance : ToJson DkgTranscriptCommit where
  toJson d := Json.mkObj [
    ("transcript_hash", ToJson.toJson d.transcript_hash),
    ("epoch", Json.num d.epoch)
  ]

instance : FromJson DkgTranscriptCommit where
  fromJson? j := do
    let transcript_hash ← j.getObjValAs? Hash32 "transcript_hash"
    let epoch ← j.getObjValAs? Nat "epoch"
    pure ⟨transcript_hash, epoch⟩

instance : ToJson ConvergenceCert where
  toJson c := Json.mkObj [
    ("op_id", ToJson.toJson c.op_id),
    ("coordinator", ToJson.toJson c.coordinator)
  ]

instance : FromJson ConvergenceCert where
  fromJson? j := do
    let op_id ← j.getObjValAs? Hash32 "op_id"
    let coordinator ← j.getObjValAs? AuthorityId "coordinator"
    pure ⟨op_id, coordinator⟩

instance : ToJson ReversionFact where
  toJson r := Json.mkObj [
    ("op_id", ToJson.toJson r.op_id),
    ("reason", Json.str r.reason)
  ]

instance : FromJson ReversionFact where
  fromJson? j := do
    let op_id ← j.getObjValAs? Hash32 "op_id"
    let reason ← j.getObjValAs? String "reason"
    pure ⟨op_id, reason⟩

instance : ToJson RotateFact where
  toJson r := Json.mkObj [("to_state", ToJson.toJson r.to_state)]

instance : FromJson RotateFact where
  fromJson? j := do
    let to_state ← j.getObjValAs? LifecycleState "to_state"
    pure ⟨to_state⟩

-- Main enum JSON instances
instance : ToJson ProtocolRelationalFact where
  toJson
    | .guardianBinding acc gua hash => Json.mkObj [
        ("variant", "guardianBinding"),
        ("account_id", ToJson.toJson acc),
        ("guardian_id", ToJson.toJson gua),
        ("binding_hash", ToJson.toJson hash)
      ]
    | .recoveryGrant acc gua hash => Json.mkObj [
        ("variant", "recoveryGrant"),
        ("account_id", ToJson.toJson acc),
        ("guardian_id", ToJson.toJson gua),
        ("grant_hash", ToJson.toJson hash)
      ]
    | .consensus cid oph met cnt => Json.mkObj [
        ("variant", "consensus"),
        ("consensus_id", ToJson.toJson cid),
        ("operation_hash", ToJson.toJson oph),
        ("threshold_met", Json.bool met),
        ("participant_count", Json.num cnt)
      ]
    | .ampChannelCheckpoint ck => Json.mkObj [
        ("variant", "ampChannelCheckpoint"),
        ("data", ToJson.toJson ck)
      ]
    | .ampProposedChannelEpochBump bump => Json.mkObj [
        ("variant", "ampProposedChannelEpochBump"),
        ("data", ToJson.toJson bump)
      ]
    | .ampCommittedChannelEpochBump bump => Json.mkObj [
        ("variant", "ampCommittedChannelEpochBump"),
        ("data", ToJson.toJson bump)
      ]
    | .ampChannelPolicy policy => Json.mkObj [
        ("variant", "ampChannelPolicy"),
        ("data", ToJson.toJson policy)
      ]
    | .leakageEvent event => Json.mkObj [
        ("variant", "leakageEvent"),
        ("data", ToJson.toJson event)
      ]
    | .dkgTranscriptCommit commit => Json.mkObj [
        ("variant", "dkgTranscriptCommit"),
        ("data", ToJson.toJson commit)
      ]
    | .convergenceCert cert => Json.mkObj [
        ("variant", "convergenceCert"),
        ("data", ToJson.toJson cert)
      ]
    | .reversionFact rev => Json.mkObj [
        ("variant", "reversionFact"),
        ("data", ToJson.toJson rev)
      ]
    | .rotateFact rot => Json.mkObj [
        ("variant", "rotateFact"),
        ("data", ToJson.toJson rot)
      ]

instance : FromJson ProtocolRelationalFact where
  fromJson? j := do
    let variant ← j.getObjValAs? String "variant"
    match variant with
    | "guardianBinding" => do
      let acc ← j.getObjValAs? AuthorityId "account_id"
      let gua ← j.getObjValAs? AuthorityId "guardian_id"
      let hash ← j.getObjValAs? Hash32 "binding_hash"
      pure (.guardianBinding acc gua hash)
    | "recoveryGrant" => do
      let acc ← j.getObjValAs? AuthorityId "account_id"
      let gua ← j.getObjValAs? AuthorityId "guardian_id"
      let hash ← j.getObjValAs? Hash32 "grant_hash"
      pure (.recoveryGrant acc gua hash)
    | "consensus" => do
      let cid ← j.getObjValAs? Hash32 "consensus_id"
      let oph ← j.getObjValAs? Hash32 "operation_hash"
      let met ← j.getObjValAs? Bool "threshold_met"
      let cnt ← j.getObjValAs? Nat "participant_count"
      pure (.consensus cid oph met cnt)
    | "ampChannelCheckpoint" =>
      .ampChannelCheckpoint <$> j.getObjValAs? ChannelCheckpoint "data"
    | "ampProposedChannelEpochBump" =>
      .ampProposedChannelEpochBump <$> j.getObjValAs? ProposedChannelEpochBump "data"
    | "ampCommittedChannelEpochBump" =>
      .ampCommittedChannelEpochBump <$> j.getObjValAs? CommittedChannelEpochBump "data"
    | "ampChannelPolicy" =>
      .ampChannelPolicy <$> j.getObjValAs? ChannelPolicy "data"
    | "leakageEvent" =>
      .leakageEvent <$> j.getObjValAs? LeakageFact "data"
    | "dkgTranscriptCommit" =>
      .dkgTranscriptCommit <$> j.getObjValAs? DkgTranscriptCommit "data"
    | "convergenceCert" =>
      .convergenceCert <$> j.getObjValAs? ConvergenceCert "data"
    | "reversionFact" =>
      .reversionFact <$> j.getObjValAs? ReversionFact "data"
    | "rotateFact" =>
      .rotateFact <$> j.getObjValAs? RotateFact "data"
    | _ => throw s!"Unknown ProtocolRelationalFact variant: {variant}"

end Aura.Types.ProtocolFacts
