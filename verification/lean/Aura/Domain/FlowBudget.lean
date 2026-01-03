/-!
# Flow Budget Types and Operations

Types and operations for the flow budget rate limiting system.

## Quint Correspondence
- File: verification/quint/protocol_capability_properties.qnt
- Section: TYPES, OPERATIONS

## Rust Correspondence
- File: crates/aura-core/src/domain/flow_budget.rs
- Type: `FlowBudget`
- Function: `charge` - deduct cost from available budget

## Expose

**Types** (stable):
- `Budget`: Available units remaining for rate limiting

**Operations** (stable):
- `charge`: Deduct cost from budget, returning None if insufficient
-/

namespace Aura.Domain.FlowBudget

/-!
## Core Types

Budget state for rate limiting.
-/

/-- Budget state: tracks available units remaining.
    Rust: FlowBudget { limit, spent } where available = limit - spent -/
structure Budget where
  available : Nat
  deriving BEq, Repr

/-!
## Charge Operation

Deduct cost from budget with underflow protection.
-/

/-- Charge a cost against a budget. Returns None if insufficient.
    Quint: Models FlowGuard check: if spent + cost > limit, block -/
def charge (budget : Budget) (cost : Nat) : Option Budget :=
  if budget.available >= cost then
    some { available := budget.available - cost }
  else
    none

end Aura.Domain.FlowBudget
