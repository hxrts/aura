import Aura.Domain.ContextIsolation

/-!
# Context Isolation Proofs

Proves that context isolation prevents information flow between unrelated
contexts. This is a core privacy guarantee from docs/002_theoretical_model.md.

## Key Properties

1. **No Cross-Context Merge**: Messages from different contexts cannot be
   combined without an explicit bridge.
2. **Namespace Isolation**: Journal namespaces prevent fact mixing.
3. **Bridge Authorization**: Cross-context flow requires explicit authorization.

## Quint Correspondence
- File: verification/quint/authorization.qnt
- File: verification/quint/leakage.qnt
- Invariants: Context isolation checks, InvariantObserverHierarchy

## Rust Correspondence
- File: crates/aura-core/src/context.rs
- File: crates/aura-journal/src/lib.rs (namespace assertions)

## Expose

**Properties** (stable, theorem statements):
- `context_isolation`: Messages from different contexts are blocked
- `namespace_isolation`: Incompatible namespaces cannot merge
- `bridge_required`: Cross-context flow requires authorized bridge

**Internal helpers** (may change):
- Auxiliary lemmas for context equality
-/

namespace Aura.Proofs.ContextIsolation

open Aura.Domain.ContextIsolation

/-!
## Helper Lemmas

BEq lemmas for our custom types using LawfulBEq instances.
-/

/-- BEq on ContextId is reflexive (via LawfulBEq). -/
theorem ContextId.beq_refl (c : ContextId) : (c == c) = true :=
  beq_self_eq_true c

/-- BEq on ContextId reflects equality (via LawfulBEq). -/
theorem ContextId.beq_eq_true_iff (c1 c2 : ContextId) : (c1 == c2) = true ↔ c1 = c2 :=
  ⟨eq_of_beq, fun h => h ▸ beq_self_eq_true c1⟩

/-- BEq on AuthorityId is reflexive (via LawfulBEq). -/
theorem AuthorityId.beq_refl (a : AuthorityId) : (a == a) = true :=
  beq_self_eq_true a

/-- BEq on AuthorityId reflects equality (via LawfulBEq). -/
theorem AuthorityId.beq_eq_true_iff (a1 a2 : AuthorityId) : (a1 == a2) = true ↔ a1 = a2 :=
  ⟨eq_of_beq, fun h => h ▸ beq_self_eq_true a1⟩

/-!
## Context Isolation Theorems

Core properties ensuring context separation.
-/

/-- Messages from different contexts are not processable together.
    This is the fundamental isolation property from docs/002_theoretical_model.md §1.3:
    "No reduction may combine messages of distinct contexts" -/
theorem context_isolation (m1 m2 : ContextMessage) (ctx : ContextId) :
    m1.contextId ≠ m2.contextId →
    ¬(canProcess m1 ctx ∧ canProcess m2 ctx) := by
  intro hneq ⟨h1, h2⟩
  unfold canProcess at h1 h2
  have eq1 : m1.contextId = ctx := eq_of_beq h1
  have eq2 : m2.contextId = ctx := eq_of_beq h2
  rw [eq1, eq2] at hneq
  exact hneq rfl

/-- Same context messages can be processed together. -/
theorem same_context_processable (m1 m2 : ContextMessage) :
    m1.contextId = m2.contextId →
    canProcess m1 m1.contextId ∧ canProcess m2 m1.contextId := by
  intro heq
  constructor
  · unfold canProcess
    exact beq_self_eq_true m1.contextId
  · unfold canProcess
    rw [← heq]
    exact beq_self_eq_true m1.contextId

/-- isSameContext is reflexive. -/
theorem same_context_refl (m : ContextMessage) : isSameContext m m = true := by
  unfold isSameContext
  exact beq_self_eq_true m.contextId

/-- isSameContext is symmetric. -/
theorem same_context_symm (m1 m2 : ContextMessage) :
    isSameContext m1 m2 = isSameContext m2 m1 := by
  unfold isSameContext
  cases hcmp : (m1.contextId == m2.contextId) with
  | false =>
    cases hrev : (m2.contextId == m1.contextId) with
    | false => rfl
    | true =>
      have heq : m2.contextId = m1.contextId := eq_of_beq hrev
      rw [heq, beq_self_eq_true] at hcmp
      simp at hcmp
  | true =>
    have heq : m1.contextId = m2.contextId := eq_of_beq hcmp
    rw [heq]
    exact (beq_self_eq_true m2.contextId).symm

/-- isSameContext is transitive. -/
theorem same_context_trans (m1 m2 m3 : ContextMessage) :
    isSameContext m1 m2 = true →
    isSameContext m2 m3 = true →
    isSameContext m1 m3 = true := by
  intro h12 h23
  unfold isSameContext at *
  have eq12 : m1.contextId = m2.contextId := eq_of_beq h12
  have eq23 : m2.contextId = m3.contextId := eq_of_beq h23
  rw [eq12, eq23]
  exact beq_self_eq_true m3.contextId

/-!
## Namespace Isolation Theorems

Properties ensuring journal namespace separation.
-/

/-- Incompatible namespaces cannot merge.
    This ensures authority journals don't mix with context journals,
    and different authorities/contexts stay separate. -/
theorem namespace_isolation (n1 n2 : JournalNamespace) :
    ¬namespacesCompatible n1 n2 →
    n1 ≠ n2 := by
  intro hincompat heq
  rw [heq] at hincompat
  unfold namespacesCompatible at hincompat
  cases n2 with
  | authority a =>
    have : (a == a) = true := beq_self_eq_true a
    exact hincompat this
  | context c =>
    have : (c == c) = true := beq_self_eq_true c
    exact hincompat this

/-- Compatible namespaces must be of the same type. -/
theorem compatible_same_type (n1 n2 : JournalNamespace) :
    namespacesCompatible n1 n2 = true →
    (∃ a1 a2, n1 = JournalNamespace.authority a1 ∧ n2 = JournalNamespace.authority a2) ∨
    (∃ c1 c2, n1 = JournalNamespace.context c1 ∧ n2 = JournalNamespace.context c2) := by
  intro hcompat
  unfold namespacesCompatible at hcompat
  cases n1 with
  | authority a1 =>
    cases n2 with
    | authority a2 =>
      left
      exact ⟨a1, a2, rfl, rfl⟩
    | context _ =>
      simp at hcompat
  | context c1 =>
    cases n2 with
    | authority _ =>
      simp at hcompat
    | context c2 =>
      right
      exact ⟨c1, c2, rfl, rfl⟩

/-- Authority namespaces are compatible iff they have the same authority. -/
theorem authority_namespace_compat (a1 a2 : AuthorityId) :
    namespacesCompatible (JournalNamespace.authority a1) (JournalNamespace.authority a2) =
    (a1 == a2) := by
  unfold namespacesCompatible
  rfl

/-- Context namespaces are compatible iff they have the same context. -/
theorem context_namespace_compat (c1 c2 : ContextId) :
    namespacesCompatible (JournalNamespace.context c1) (JournalNamespace.context c2) =
    (c1 == c2) := by
  unfold namespacesCompatible
  rfl

/-!
## Bridge Authorization Theorems

Properties ensuring cross-context flow requires explicit bridges.
-/

/-- Cross-context flow requires an authorized bridge. -/
theorem bridge_required (src tgt : ContextId) (bridge : Bridge) :
    src ≠ tgt →
    canBridge bridge src tgt = true →
    bridge.authorized = true := by
  intro _ hcan
  unfold canBridge at hcan
  simp only [Bool.and_eq_true] at hcan
  obtain ⟨⟨_, _⟩, hauth⟩ := hcan
  exact hauth

/-- Unauthorized bridges cannot enable cross-context flow. -/
theorem unauthorized_bridge_blocks (src tgt : ContextId) (bridge : Bridge) :
    bridge.authorized = false →
    canBridge bridge src tgt = false := by
  intro hunauth
  unfold canBridge
  simp [hunauth]

/-- Bridges are directional: source and target matter. -/
theorem bridge_directional (bridge : Bridge) :
    bridge.source ≠ bridge.target →
    canBridge bridge bridge.source bridge.target = true →
    canBridge bridge bridge.target bridge.source = false := by
  intro hneq _
  unfold canBridge
  have hne : (bridge.target == bridge.source) = false := by
    cases h : bridge.target == bridge.source with
    | false => rfl
    | true =>
      have heq : bridge.target = bridge.source := eq_of_beq h
      exact absurd heq.symm hneq
  simp [hne]

/-!
## Claims Bundle
-/

/-- Claims bundle for context isolation properties. -/
structure ContextIsolationClaims where
  /-- No cross-context processing without same context. -/
  no_cross_context : ∀ (m1 m2 : ContextMessage) (ctx : ContextId),
    m1.contextId ≠ m2.contextId →
    ¬(canProcess m1 ctx ∧ canProcess m2 ctx)

  /-- Namespace compatibility requires same type and ID. -/
  namespace_type_match : ∀ (n1 n2 : JournalNamespace),
    namespacesCompatible n1 n2 = true →
    (∃ a1 a2, n1 = JournalNamespace.authority a1 ∧ n2 = JournalNamespace.authority a2) ∨
    (∃ c1 c2, n1 = JournalNamespace.context c1 ∧ n2 = JournalNamespace.context c2)

  /-- Cross-context flow needs authorized bridge. -/
  bridge_auth_required : ∀ (src tgt : ContextId) (bridge : Bridge),
    src ≠ tgt →
    canBridge bridge src tgt = true →
    bridge.authorized = true

/-- The context isolation claims bundle. -/
def contextIsolationClaims : ContextIsolationClaims where
  no_cross_context := context_isolation
  namespace_type_match := compatible_same_type
  bridge_auth_required := bridge_required

end Aura.Proofs.ContextIsolation
