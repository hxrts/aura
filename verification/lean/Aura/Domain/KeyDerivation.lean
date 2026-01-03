/-!
# Contextual Key Derivation Types and Operations

Types for context-specific key derivation ensuring relational isolation.

## Quint Correspondence
- File: verification/quint/protocol_dkd.qnt
- Section: TYPE DEFINITIONS

## Rust Correspondence
- File: crates/aura-core/src/crypto/key_derivation.rs
- Type: `RootKey`, `DerivedKey`

## Expose

**Types** (stable):
- `RootKey`: Account's master key held in threshold shares
- `AppId`: Application identifier namespace
- `CtxLabel`: Context label for specific relationship
- `DerivedKey`: Output of key derivation

**Operations** (stable):
- `derive`: Derive context-specific key from root (axiom)
-/

namespace Aura.Domain.KeyDerivation

/-!
## Core Types

Key types for hierarchical derivation.
-/

/-- The account's root key, held in threshold shares across devices.
    Rust: aura-core/src/crypto/key_derivation.rs -/
structure RootKey where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Application identifier (e.g., "chat", "storage", "recovery").
    Rust: Corresponds to derivation path component -/
structure AppId where
  id : String
  deriving BEq, Repr, DecidableEq

/-- Context label identifying a specific relationship or usage.
    Rust: Combined with AppId to form full derivation path -/
structure CtxLabel where
  label : String
  deriving BEq, Repr, DecidableEq

/-- The output of key derivation.
    Rust: 32-byte key for symmetric encryption or signing -/
structure DerivedKey where
  value : Nat
  deriving BEq, Repr, DecidableEq

/-!
## Key Derivation Function

Abstract key derivation with cryptographic assumptions.
-/

/-- Key derivation function (abstract).
    Rust: HKDF-based construction in aura-core -/
axiom derive : RootKey → AppId → CtxLabel → DerivedKey

end Aura.Domain.KeyDerivation
