import Lean.Data.Json
import Aura.Types.ByteArray32

/-! # Aura.Types.Identifiers

Cryptographic identifiers wrapping ByteArray32 with domain-specific semantics.

## Rust Correspondence
- File: crates/aura-core/src/types/identifiers.rs
- Hash32-backed: ChannelId, HomeId (wrap [u8; 32])
- UUID-backed: AuthorityId, ContextId (wrap Uuid = 16 bytes, padded to 32 for Lean model)

Note: For verification purposes, all identifiers use ByteArray32 (32 bytes).
The Rust implementation uses both 16-byte (UUID) and 32-byte (Hash32) backing,
but the CRDT properties we verify are independent of the representation size.

## Expose

**Types** (stable):
- `Hash32`: Content-addressed hash identifier
- `AuthorityId`: Opaque authority identifier
- `ContextId`: Relational context identifier
- `ChannelId`: AMP messaging channel identifier

**Operations** (stable):
- `BEq`: Equality derived from ByteArray32
- `Ord`: Comparison derived from ByteArray32
- JSON (de)serialization via hex encoding
-/

namespace Aura.Types.Identifiers

open Lean (Json ToJson FromJson)
open Aura.Types.ByteArray32 (ByteArray32)

/-! ## Hash32 -/

/-- Content-addressed 32-byte hash.
    Rust: aura-core/src/domain/content.rs::Hash32([u8; 32]) -/
structure Hash32 where
  /-- The underlying 32-byte value. -/
  value : ByteArray32
  deriving Repr

instance : BEq Hash32 where
  beq a b := a.value == b.value

instance : DecidableEq Hash32 := fun a b =>
  if h : a.value = b.value then
    isTrue (by cases a; cases b; simp_all)
  else
    isFalse (by intro heq; cases heq; exact h rfl)

instance : Ord Hash32 where
  compare a b := Aura.Types.ByteArray32.compare a.value b.value

instance : ToJson Hash32 where
  toJson h := ToJson.toJson h.value

instance : FromJson Hash32 where
  fromJson? j := do
    let bytes ← FromJson.fromJson? j
    pure ⟨bytes⟩

/-- Create a Hash32 filled with zeros. -/
def Hash32.zero : Hash32 := ⟨ByteArray32.zero⟩

/-! ## AuthorityId -/

/-- Opaque authority identifier for the authority-centric model.
    Represents a cryptographic authority with internal structure (not exposed).
    Rust: aura-core/src/types/identifiers.rs::AuthorityId(Uuid)
    Note: Lean model uses 32 bytes for uniformity; Rust uses 16-byte UUID. -/
structure AuthorityId where
  /-- The underlying identifier value. -/
  value : ByteArray32
  deriving Repr

instance : BEq AuthorityId where
  beq a b := a.value == b.value

instance : DecidableEq AuthorityId := fun a b =>
  if h : a.value = b.value then
    isTrue (by cases a; cases b; simp_all)
  else
    isFalse (by intro heq; cases heq; exact h rfl)

instance : Ord AuthorityId where
  compare a b := Aura.Types.ByteArray32.compare a.value b.value

instance : ToJson AuthorityId where
  toJson a := ToJson.toJson a.value

instance : FromJson AuthorityId where
  fromJson? j := do
    let bytes ← FromJson.fromJson? j
    pure ⟨bytes⟩

/-- Create an AuthorityId for testing. -/
def AuthorityId.zero : AuthorityId := ⟨ByteArray32.zero⟩

/-! ## ContextId -/

/-- Relational context identifier for cross-authority relationships.
    ContextIds are opaque and never encode participant data.
    Rust: aura-core/src/types/identifiers.rs::ContextId(Uuid)
    Note: Lean model uses 32 bytes for uniformity; Rust uses 16-byte UUID. -/
structure ContextId where
  /-- The underlying identifier value. -/
  value : ByteArray32
  deriving Repr

instance : BEq ContextId where
  beq a b := a.value == b.value

instance : DecidableEq ContextId := fun a b =>
  if h : a.value = b.value then
    isTrue (by cases a; cases b; simp_all)
  else
    isFalse (by intro heq; cases heq; exact h rfl)

instance : Ord ContextId where
  compare a b := Aura.Types.ByteArray32.compare a.value b.value

instance : ToJson ContextId where
  toJson c := ToJson.toJson c.value

instance : FromJson ContextId where
  fromJson? j := do
    let bytes ← FromJson.fromJson? j
    pure ⟨bytes⟩

/-- Create a ContextId for testing. -/
def ContextId.zero : ContextId := ⟨ByteArray32.zero⟩

/-! ## ChannelId -/

/-- AMP messaging channel identifier.
    Channels are scoped under a RelationalContext.
    Rust: aura-core/src/types/identifiers.rs::ChannelId(Hash32) -/
structure ChannelId where
  /-- The underlying Hash32 value. -/
  value : ByteArray32
  deriving Repr

instance : BEq ChannelId where
  beq a b := a.value == b.value

instance : DecidableEq ChannelId := fun a b =>
  if h : a.value = b.value then
    isTrue (by cases a; cases b; simp_all)
  else
    isFalse (by intro heq; cases heq; exact h rfl)

instance : Ord ChannelId where
  compare a b := Aura.Types.ByteArray32.compare a.value b.value

instance : ToJson ChannelId where
  toJson c := ToJson.toJson c.value

instance : FromJson ChannelId where
  fromJson? j := do
    let bytes ← FromJson.fromJson? j
    pure ⟨bytes⟩

/-- Create a ChannelId for testing. -/
def ChannelId.zero : ChannelId := ⟨ByteArray32.zero⟩

end Aura.Types.Identifiers
