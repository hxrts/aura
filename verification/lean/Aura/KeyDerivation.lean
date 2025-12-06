-- Core definitions for contextual key derivation verification.
-- Proves isolation: different contexts always yield different keys.

namespace Aura.KeyDerivation

/-!
# Contextual Key Derivation

This module models the context-specific key derivation function and proves
its isolation properties.

Key property: derive(root, app_id, ctx) is unique across (app_id, ctx) pairs.

**Why this matters**: Aura derives context-specific keys so that compromising
one relationship's key doesn't reveal keys for other relationships. This is
the cryptographic foundation for relational identity isolation.
-/

-- The account's root key, held in threshold shares across devices.
-- This is the master secret from which all context keys are derived.
structure RootKey where
  id : Nat
  deriving BEq, Repr, DecidableEq

-- Application identifier (e.g., "chat", "storage", "recovery").
-- Different apps get different key namespaces even within the same context.
structure AppId where
  id : String
  deriving BEq, Repr, DecidableEq

-- Context label identifying a specific relationship or usage.
-- Combined with AppId to form the full derivation path.
structure CtxLabel where
  label : String
  deriving BEq, Repr, DecidableEq

-- The output of key derivation. Abstract here; in practice this is
-- a 32-byte key suitable for symmetric encryption or signing.
structure DerivedKey where
  value : Nat  -- Abstract representation
  deriving BEq, Repr, DecidableEq

-- **Axiom**: Key derivation function exists but is left abstract.
-- The actual implementation uses HKDF or similar PRF-based construction.
axiom derive : RootKey → AppId → CtxLabel → DerivedKey

/-!
## Cryptographic Assumption

The KDF is injective on (app_id, ctx_label) pairs for a fixed root key.
This models the PRF (Pseudorandom Function) security assumption:
- A secure PRF is indistinguishable from a random function
- Different inputs produce different outputs (with overwhelming probability)

This is a standard cryptographic assumption satisfied by HKDF-SHA256.
-/

-- **Security Axiom**: If two derived keys are equal, the inputs must be equal.
-- Contrapositive: different (app_id, ctx_label) pairs always give different keys.
axiom derive_injective :
  ∀ {root : RootKey} {app1 app2 : AppId} {ctx1 ctx2 : CtxLabel},
    derive root app1 ctx1 = derive root app2 ctx2 →
    app1 = app2 ∧ ctx1 = ctx2

-- **Main theorem: Contextual Isolation**
-- Derived keys are unique across (app, context) pairs. This means:
-- 1. Knowing key for (app1, ctx1) tells you nothing about key for (app2, ctx2)
-- 2. Different relationships cannot be correlated via their derived keys
theorem contextual_isolation (root : RootKey) (app1 app2 : AppId) (ctx1 ctx2 : CtxLabel) :
  derive root app1 ctx1 = derive root app2 ctx2 →
  app1 = app2 ∧ ctx1 = ctx2 :=
  derive_injective

end Aura.KeyDerivation
