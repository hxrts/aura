/-!
# Unified Time System Types and Operations

Types and comparison functions for the multi-domain timestamp system.

## Quint Correspondence
- File: verification/quint/protocol_core.qnt
- Section: TYPE DEFINITIONS

## Rust Correspondence
- File: crates/aura-core/src/time.rs
- Type: `TimeStamp`, `Policy`

## Expose

**Types** (stable):
- `TimeStamp`: Abstract timestamp with logical and order clock components
- `Policy`: Comparison policy (ignorePhysical for privacy)
- `Ordering`: Three-way comparison result (lt, eq, gt)

**Operations** (stable):
- `compare`: Compare timestamps according to policy
- `compareNat`: Three-way comparison for natural numbers
-/

namespace Aura.Domain.TimeSystem

/-!
## Core Types

Timestamp and policy types.
-/

/-- Abstract timestamp with two components.
    Rust: aura-core/src/time.rs::TimeStamp -/
structure TimeStamp where
  logical : Nat
  orderClock : Nat
  deriving BEq, Repr

/-- Comparison policy determines which clock components to use.
    Rust: aura-core/src/time.rs::ComparePolicy -/
structure Policy where
  ignorePhysical : Bool
  deriving BEq, Repr

/-- Three-way comparison result.
    Rust: std::cmp::Ordering -/
inductive Ordering where
  | lt : Ordering
  | eq : Ordering
  | gt : Ordering
  deriving BEq, Repr, DecidableEq

/-!
## Comparison Functions

Policy-aware timestamp comparison.
-/

/-- Standard three-way comparison for natural numbers. -/
def compareNat (a b : Nat) : Ordering :=
  if a < b then .lt
  else if a = b then .eq
  else .gt

/-- Compare timestamps according to policy.
    Quint: If ignorePhysical, only logical clock is used -/
def compare (policy : Policy) (a b : TimeStamp) : Ordering :=
  if policy.ignorePhysical then
    compareNat a.logical b.logical
  else
    match compareNat a.logical b.logical with
    | .lt => .lt
    | .gt => .gt
    | .eq => compareNat a.orderClock b.orderClock

end Aura.Domain.TimeSystem
