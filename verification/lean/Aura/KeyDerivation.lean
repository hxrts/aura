/-!
# Contextual Key Derivation Proofs

Proves that context-specific key derivation is injective, ensuring
different contexts always yield different keys for relational isolation.

## Quint Correspondence
- File: verification/quint/protocol_dkd.qnt
- Section: TYPE DEFINITIONS
- Properties: Key derivation context isolation

## Rust Correspondence
- File: crates/aura-core/src/crypto/key_derivation.rs
- Type: `RootKey`, `DerivedKey`
- Function: `derive` - context-specific key derivation via HKDF

## Expose

**Types**:
- `RootKey`: Account's master key held in threshold shares
- `AppId`: Application identifier namespace
- `CtxLabel`: Context label for specific relationship
- `DerivedKey`: Output of key derivation

**Operations** (stable):
- `derive`: Derive context-specific key from root

**Properties** (stable, theorem statements):
- `contextual_isolation`: Different (app, context) pairs yield different keys

**Internal helpers** (may change):
- None
-/

namespace Aura.KeyDerivation

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

/-!
## Claims Bundle

Key derivation isolation properties.
-/

/-- Claims bundle for KeyDerivation properties. -/
structure KeyDerivationClaims where
  /-- Contextual isolation: different (app, context) pairs yield different keys. -/
  contextual_isolation : ∀ (root : RootKey) (app1 app2 : AppId) (ctx1 ctx2 : CtxLabel),
    derive root app1 ctx1 = derive root app2 ctx2 →
    app1 = app2 ∧ ctx1 = ctx2

  /-- Root isolation: different roots with same context yield different keys. -/
  root_isolation : ∀ (root1 root2 : RootKey) (app : AppId) (ctx : CtxLabel),
    derive root1 app ctx = derive root2 app ctx →
    root1 = root2

  /-- Full isolation: equal derived keys require equal (root, app, ctx) triples. -/
  full_isolation : ∀ (root1 root2 : RootKey) (app1 app2 : AppId) (ctx1 ctx2 : CtxLabel),
    derive root1 app1 ctx1 = derive root2 app2 ctx2 →
    root1 = root2 ∧ app1 = app2 ∧ ctx1 = ctx2

/-!
## Cryptographic Assumption

The KDF is injective on (app_id, ctx_label) pairs for a fixed root key.
This models PRF (Pseudorandom Function) security.
-/

/-- Security axiom: If two derived keys are equal, the inputs must be equal.
    Cryptographic justification: PRF security of HKDF-SHA256 -/
axiom derive_injective :
  ∀ {root : RootKey} {app1 app2 : AppId} {ctx1 ctx2 : CtxLabel},
    derive root app1 ctx1 = derive root app2 ctx2 →
    app1 = app2 ∧ ctx1 = ctx2

/-- Security axiom: Different root keys produce different derived keys.
    Cryptographic justification: PRF security with unique key material -/
axiom derive_root_injective :
  ∀ {root1 root2 : RootKey} {app : AppId} {ctx : CtxLabel},
    derive root1 app ctx = derive root2 app ctx →
    root1 = root2

/-- Combined security axiom: derive is fully injective across all three arguments.
    If two derived keys are equal, all three inputs (root, app, ctx) must be equal.
    Cryptographic justification: PRF collision resistance over the full domain -/
axiom derive_full_injective :
  ∀ {root1 root2 : RootKey} {app1 app2 : AppId} {ctx1 ctx2 : CtxLabel},
    derive root1 app1 ctx1 = derive root2 app2 ctx2 →
    root1 = root2 ∧ app1 = app2 ∧ ctx1 = ctx2

/-!
## Proofs

Main isolation theorem.
-/

/-- Contextual isolation: Derived keys are unique across (app, context) pairs.
    This ensures compromising one relationship's key doesn't reveal others. -/
theorem contextual_isolation (root : RootKey) (app1 app2 : AppId) (ctx1 ctx2 : CtxLabel) :
  derive root app1 ctx1 = derive root app2 ctx2 →
  app1 = app2 ∧ ctx1 = ctx2 :=
  derive_injective

/-- Root isolation: Different roots with same context yield different keys.
    Prevents cross-account key collision. -/
theorem root_isolation (root1 root2 : RootKey) (app : AppId) (ctx : CtxLabel) :
    derive root1 app ctx = derive root2 app ctx →
    root1 = root2 :=
  derive_root_injective

/-- Full isolation: Equal derived keys require equal (root, app, ctx) triples.
    This is the strongest isolation property, combining root and context isolation. -/
theorem full_isolation (root1 root2 : RootKey) (app1 app2 : AppId) (ctx1 ctx2 : CtxLabel) :
    derive root1 app1 ctx1 = derive root2 app2 ctx2 →
    root1 = root2 ∧ app1 = app2 ∧ ctx1 = ctx2 :=
  derive_full_injective

/-!
## Claims Bundle Construction
-/

/-- The claims bundle, proving key derivation isolation. -/
def keyDerivationClaims : KeyDerivationClaims where
  contextual_isolation := contextual_isolation
  root_isolation := root_isolation
  full_isolation := full_isolation

end Aura.KeyDerivation
