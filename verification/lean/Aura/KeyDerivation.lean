-- Core definitions for contextual key derivation verification

namespace Aura.KeyDerivation

/-!
# Contextual Key Derivation

This module models the context-specific key derivation function and proves
its isolation properties.

Key property: derive(root, app_id, ctx) is unique across (app_id, ctx) pairs.
-/

/-- Abstract root key -/
structure RootKey where
  id : Nat
  deriving BEq, Repr, DecidableEq

/-- Application identifier -/
structure AppId where
  id : String
  deriving BEq, Repr, DecidableEq

/-- Context label -/
structure CtxLabel where
  label : String
  deriving BEq, Repr, DecidableEq

/-- Derived key (abstract) -/
structure DerivedKey where
  value : Nat  -- Abstract representation
  deriving BEq, Repr, DecidableEq

/-- Key derivation function (abstract, axiomatized for now) -/
axiom derive : RootKey → AppId → CtxLabel → DerivedKey

/-!
Cryptographic assumption: The KDF is injective on (app_id, ctx_label) pairs.
This models the PRF security assumption.
-/
axiom derive_injective :
  ∀ {root : RootKey} {app1 app2 : AppId} {ctx1 ctx2 : CtxLabel},
    derive root app1 ctx1 = derive root app2 ctx2 →
    app1 = app2 ∧ ctx1 = ctx2

/-- Main theorem: Derived keys are unique across contexts -/
theorem contextual_isolation (root : RootKey) (app1 app2 : AppId) (ctx1 ctx2 : CtxLabel) :
  derive root app1 ctx1 = derive root app2 ctx2 →
  app1 = app2 ∧ ctx1 = ctx2 :=
  derive_injective

end Aura.KeyDerivation
