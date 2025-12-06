# Aura Lean Verification

This directory contains Lean 4 formal verification modules for Aura's kernel components.

## Setup

The Lean toolchain is managed through Nix. To enter the development environment:

```bash
nix develop
```

## Building

Build the Lean verification modules:

```bash
just lean-build
```

Or directly with lake:

```bash
cd specs/lean && lake build
```

## Module Structure

| Module | Description | Theorems |
|--------|-------------|----------|
| `Aura/Journal/Core.lean` | CRDT semilattice definitions & instance | `merge_comm`, `merge_assoc`, `merge_idem`, `reduce_deterministic` |
| `Aura/KeyDerivation/Core.lean` | Contextual key derivation | `derive_unique` (axiomatic) |
| `Aura/GuardChain/Core.lean` | Guard chain evaluation | `cost_sum` |
| `Aura/FlowBudget/Core.lean` | Flow budget mathematics | `charge_decreases`, `charge_exact` |
| `Aura/Frost/Core.lean` | FROST protocol state machine | `aggregate_same_session_round` |
| `Aura/TimeSystem/Core.lean` | TimeStamp ordering & privacy | `compare_refl`, `compare_trans`, `physical_hidden` |
| `Aura/Runner.lean` | CLI runner for differential testing | (executable) |

## Proof Status

All core proofs are complete:

| Module | Status |
|--------|--------|
| `GuardChain/Core` | ● Complete |
| `Journal/Core` | ● Complete |
| `KeyDerivation/Core` | ● Complete |
| `FlowBudget/Core` | ● Complete |
| `Frost/Core` | ● Complete |
| `TimeSystem/Core` | ● Complete |
| `Runner` | ● Complete |

## Key Properties Proven

### Journal CRDT (Semilattice Laws)
- **Commutativity**: `merge j1 j2 ≃ merge j2 j1`
- **Associativity**: `merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3)`
- **Idempotence**: `merge j j ≃ j`

### Flow Budget
- **Monotonic decrease**: Charging never increases available budget
- **Exact charge**: Charging exact amount results in zero budget

### FROST Protocol
- **Session/round consistency**: Successful aggregation implies all shares have the same session ID and round

### TimeSystem
- **Reflexivity**: `compare policy t t = .eq`
- **Transitivity**: `compare policy a b = .lt → compare policy b c = .lt → compare policy a c = .lt`
- **Privacy**: Physical time is hidden when `ignorePhysical = true`

## Justfile Commands

```bash
just lean-check    # Build and check proofs
just lean-status   # Show proof status summary
just lean-full     # Full workflow (clean + build + verify)
just lean-clean    # Clean build artifacts
```

## Implementation Notes

### Manual BEq Instances (Frost/Core.lean)

The FROST module uses manually defined `BEq` instances instead of `deriving BEq`:

```lean
instance : BEq SessionId where
  beq a b := a.id == b.id
```

This is necessary because Lean 4's derived `BEq` instances create opaque internal functions (like `beqSessionId✝`) that cannot be unfolded by `simp`. Manual instances allow the proofs to reduce `(a == b)` to `decide (a = b)`, which can then be converted to equality via `of_decide_eq_true`.

### Membership-Based Set Equivalence (Journal/Core.lean)

Journal equivalence is defined via membership rather than structural equality:

```lean
def Journal.equiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1 ↔ f ∈ j2
```

This matches CRDT semantics where we care about the set of facts, not their list ordering.

## CI Integration

- CI job: `nix develop -c (cd specs/lean && lake build)`
- Failure policy: Block PRs touching kernel modules when proofs break
- Warnings about macOS version mismatches can be ignored (Nix cross-compilation)

## Next Steps

1. Add JSON serialization for Rust↔Lean differential testing
2. Integrate CLI runner with `aura-testkit` fixtures
3. Add more comprehensive proof coverage for edge cases
