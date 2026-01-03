import Aura.Domain.Consensus.Types
import Aura.Assumptions

/-!
# Consensus Liveness Properties

States liveness claims for Aura consensus termination and progress.
Liveness properties are verified via Quint model checking; this file
documents the assumptions and provides Lean-side type definitions.

## Quint Correspondence
- File: verification/quint/protocol_consensus_liveness.qnt
- Properties: isSynchronous, canMakeProgress, isTerminal, hasQuorumOnline
- Temporal: FastPathProgressCheck, SlowPathProgressCheck, ActiveMakesProgress

## Rust Correspondence
- File: crates/aura-consensus/src/consensus/coordinator.rs
- File: crates/aura-consensus/src/consensus/fallback.rs

## Expose

**Types**:
- `Time`: Logical time for synchrony model
- `SynchronyState`: GST-based partial synchrony model
- `ProgressCondition`: Conditions required for progress

**Properties** (stated as axioms, verified in Quint):
- `terminationUnderSynchrony`: If synchronized with quorum, eventually commits
- `fastPathBound`: Fast path completes within bounded time
- `fallbackBound`: Fallback completes within bounded time

**Internal helpers** (may change):
- Timing calculations
-/

namespace Aura.Proofs.Consensus.Liveness

open Aura.Domain.Consensus.Types
open Aura.Assumptions

/-!
## Synchrony Model Types

Partial synchrony with Global Stabilization Time (GST).
Before GST: messages may be arbitrarily delayed (but not lost).
After GST: all messages delivered within bound Δ.
-/

/-- Logical time units (abstract, not wall-clock).
    Quint: protocol_consensus_liveness.qnt Time -/
abbrev Time := Nat

/-- Message delay bound after GST.
    Quint: DELTA constant -/
def delta : Time := 3

/-- Fallback trigger timeout (2-3× Δ).
    Quint: T_FALLBACK constant -/
def fallbackTimeout : Time := delta * 2

/-- Synchrony state for partial synchrony model.
    Quint: gst, gstReached state variables -/
structure SynchronyState where
  /-- Current logical time. -/
  currentTime : Time
  /-- Global Stabilization Time (when network stabilizes). -/
  gst : Time
  /-- Whether GST has been reached. -/
  gstReached : Bool
  deriving BEq, Repr

/-- Check if network is currently synchronous.
    Quint: isSynchronous predicate -/
def isSynchronous (s : SynchronyState) : Bool :=
  s.gstReached && s.currentTime >= s.gst

/-!
## Progress Conditions

Conditions under which consensus can make progress toward termination.
-/

/-- Witness participation state for liveness analysis.
    Quint: WitnessParticipation type -/
structure WitnessParticipation where
  /-- Witness follows protocol correctly. -/
  isHonest : Bool
  /-- Witness is currently reachable. -/
  isOnline : Bool
  deriving BEq, Repr

/-- Progress condition for consensus instance.
    Combines synchrony, quorum, and threshold requirements. -/
structure ProgressCondition where
  /-- Network is past GST. -/
  isSynchronous : Bool
  /-- Threshold honest witnesses are online. -/
  hasQuorum : Bool
  /-- Byzantine count is below threshold. -/
  byzantineBelowThreshold : Bool
  deriving BEq, Repr

/-- Check if all progress conditions are met.
    When true, consensus should eventually terminate. -/
def canMakeProgress (pc : ProgressCondition) : Bool :=
  pc.isSynchronous && pc.hasQuorum && pc.byzantineBelowThreshold

/-!
## Liveness Claims Bundle

These properties are stated as axioms since Lean cannot directly prove
temporal properties. They are verified via Quint model checking.

The claims express: "Under these conditions, consensus eventually terminates."
-/

/-- Claims bundle for consensus liveness properties. -/
structure LivenessClaims where
  /-- Termination under synchrony: If synchronized with honest quorum,
      active instances eventually commit or fail cleanly.
      Quint: InvariantProgressUnderSynchrony -/
  terminationUnderSynchrony : ∀ (sync : SynchronyState) (pc : ProgressCondition),
    isSynchronous sync →
    canMakeProgress pc →
    True  -- Temporal property: eventually(phase == Committed ∨ phase == Failed)

  /-- Fast path bound: Fast path completes within Δ time after GST
      when all witnesses have valid cached nonces.
      Quint: FastPathProgressCheck -/
  fastPathBound : ∀ (sync : SynchronyState),
    isSynchronous sync →
    True  -- Temporal property: fast path completes within delta

  /-- Fallback bound: Fallback completes within T_FALLBACK time after GST
      when quorum of honest witnesses are online.
      Quint: SlowPathProgressCheck -/
  fallbackBound : ∀ (sync : SynchronyState),
    isSynchronous sync →
    True  -- Temporal property: fallback completes within fallbackTimeout

  /-- No deadlock: Active instances always have an enabled action.
      Quint: NoDeadlock -/
  noDeadlock : ∀ (pc : ProgressCondition),
    canMakeProgress pc →
    True  -- State property: some action is enabled

/-!
## Axioms

Liveness properties are axiomatized since they require temporal reasoning
that Lean cannot directly express. Verification is done in Quint.
-/

/-- Axiom: Under partial synchrony with honest quorum, consensus terminates.
    This is the fundamental liveness assumption for distributed consensus.
    Verified in: protocol_consensus_liveness.qnt -/
axiom liveness_under_synchrony :
  ∀ (sync : SynchronyState) (threshold byzantineCount : Nat),
    isSynchronous sync →
    byzantineCount < threshold →
    True  -- Temporal: eventually terminates

/-- Axiom: Fast path is faster than fallback when nonces are cached.
    This justifies the pipelining optimization.
    Verified in: protocol_consensus_liveness.qnt -/
axiom fast_path_faster :
  delta < fallbackTimeout

/-!
## Claims Bundle Construction
-/

/-- The liveness claims bundle.
    Note: These are trivially satisfied in Lean since temporal properties
    are represented as True. The actual verification is in Quint. -/
def livenessClaims : LivenessClaims where
  terminationUnderSynchrony := fun _ _ _ _ => trivial
  fastPathBound := fun _ _ => trivial
  fallbackBound := fun _ _ => trivial
  noDeadlock := fun _ _ => trivial

/-!
## Theorems

These theorems establish basic properties that support liveness reasoning.
-/

/-- Fast path is strictly faster than fallback timeout. -/
theorem fast_path_bound_correct : delta < fallbackTimeout := by
  unfold delta fallbackTimeout
  omega

/-- Synchrony requires GST to be reached. -/
theorem synchrony_requires_gst (s : SynchronyState) :
    isSynchronous s → s.gstReached := by
  intro h
  unfold isSynchronous at h
  exact Bool.and_eq_true.mp h |>.1

/-- Progress requires all conditions. -/
theorem progress_requires_all (pc : ProgressCondition) :
    canMakeProgress pc →
    pc.isSynchronous ∧ pc.hasQuorum ∧ pc.byzantineBelowThreshold := by
  intro h
  unfold canMakeProgress at h
  simp only [Bool.and_eq_true] at h
  exact h

end Aura.Proofs.Consensus.Liveness
