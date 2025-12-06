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
cd verification/lean && lake build
```

## Module Structure

| Module | Description | Theorems |
|--------|-------------|----------|
| `Aura/Journal.lean` | CRDT semilattice definitions & instance | `merge_comm`, `merge_assoc`, `merge_idem`, `reduce_deterministic` |
| `Aura/KeyDerivation.lean` | Contextual key derivation | `derive_unique` (axiomatic) |
| `Aura/GuardChain.lean` | Guard chain evaluation | `cost_sum` |
| `Aura/FlowBudget.lean` | Flow budget mathematics | `charge_decreases`, `charge_exact` |
| `Aura/Frost.lean` | FROST protocol state machine | `aggregate_same_session_round` |
| `Aura/TimeSystem.lean` | TimeStamp ordering & privacy | `compare_refl`, `compare_trans`, `physical_hidden` |
| `Aura/Runner.lean` | CLI runner for differential testing | (executable) |

## Proof Status

All core proofs are complete:

| Module | Status |
|--------|--------|
| `GuardChain` | ● Complete |
| `Journal` | ● Complete |
| `KeyDerivation` | ● Complete |
| `FlowBudget` | ● Complete |
| `Frost` | ● Complete |
| `TimeSystem` | ● Complete |
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

### Manual BEq Instances (Frost.lean)

The FROST module uses manually defined `BEq` instances instead of `deriving BEq`:

```lean
instance : BEq SessionId where
  beq a b := a.id == b.id
```

This is necessary because Lean 4's derived `BEq` instances create opaque internal functions (like `beqSessionId✝`) that cannot be unfolded by `simp`. Manual instances allow the proofs to reduce `(a == b)` to `decide (a = b)`, which can then be converted to equality via `of_decide_eq_true`.

### Membership-Based Set Equivalence (Journal.lean)

Journal equivalence is defined via membership rather than structural equality:

```lean
def Journal.equiv (j1 j2 : Journal) : Prop :=
  ∀ f, f ∈ j1 ↔ f ∈ j2
```

This matches CRDT semantics where we care about the set of facts, not their list ordering.

## Differential Testing

The Lean oracle can be used for differential testing against Rust implementations:

```bash
# Build the Lean oracle CLI
just lean-oracle-build

# Run differential tests
just test-differential
```

The oracle exposes these commands via JSON stdin/stdout:
- `aura_verifier journal-merge` - Test journal merge operations
- `aura_verifier journal-reduce` - Test journal reduction
- `aura_verifier flow-charge` - Test flow budget charging
- `aura_verifier timestamp-compare` - Test timestamp comparison

See `crates/aura-testkit/tests/lean_differential.rs` for property-based tests.

## CI Integration

Two CI jobs verify the formal proofs:

1. **lean-proofs**: Builds all Lean modules, checks for `sorry` usage
2. **differential-testing**: Runs Rust vs Lean oracle tests

Both jobs only run when relevant files change:
- `verification/lean/**`
- `crates/aura-journal/**`
- `crates/aura-core/src/time/**`
- `crates/aura-testkit/**`

## Verification Failure Triage

When CI fails, use this guide to diagnose:

| Failure Type | Symptom | Action |
|--------------|---------|--------|
| **Lean build failure** | `lake build` fails | Check Lean syntax errors in modified files |
| **Proof regression** | `sorry` added or theorem fails | Investigate why the property no longer holds |
| **Differential test mismatch** | Rust ≠ Lean output | Either Rust has a bug, or Lean spec needs updating |
| **Oracle version mismatch** | Version check fails | Update `LeanOracle::expected_version` after Lean changes |

### Common Issues

1. **Rust implementation bug**: If differential test fails, the Rust code may not match the proven specification. Compare the algorithm step-by-step.

2. **Spec drift**: If the Lean model was updated but Rust wasn't (or vice versa), ensure both stay in sync.

3. **Semantic mismatch**: The Lean model uses list-based Journal with membership equivalence (`≃`). Rust implementations should compare using set semantics.

## Next Steps

1. ~~Add JSON serialization for Rust↔Lean differential testing~~ ✅
2. ~~Integrate CLI runner with `aura-testkit` fixtures~~ ✅
3. Add Aeneas translation for critical Rust functions (Phase 2)
4. Add more comprehensive proof coverage for edge cases
