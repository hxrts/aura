# Lean 4 Style Guide for Aura Verification

This guide establishes conventions for Lean 4 proof development in the Aura verification suite.

## Module Structure

### File Organization

```
Aura/
├── Assumptions.lean        # Centralized axioms and assumptions
├── Consensus/
│   ├── Types.lean          # Domain types
│   ├── Agreement.lean      # Agreement proofs
│   ├── Validity.lean       # Validity proofs
│   ├── Evidence.lean       # Evidence CRDT proofs
│   ├── Equivocation.lean   # Equivocation detection proofs
│   ├── Frost.lean          # FROST integration proofs
│   └── Proofs.lean         # Claims bundle aggregation
├── Journal.lean            # Journal CRDT proofs
├── KeyDerivation.lean      # Key derivation isolation
├── GuardChain.lean         # Guard chain cost proofs
├── FlowBudget.lean         # Budget charging proofs
├── Frost.lean              # FROST state machine proofs
├── TimeSystem.lean         # Timestamp ordering proofs
└── Runner.lean             # CLI for differential testing
```

### Module Template

Each proof module should follow this structure:

```lean
import Aura.Consensus.Types
import Aura.Assumptions

/-!
# Module Title

Brief description of what this module proves.

## Quint Correspondence
- File: verification/quint/protocol_*.qnt
- Section: INVARIANTS
- Invariant: `InvariantName`

## Rust Correspondence
- File: crates/aura-*/src/*.rs
- Type/Function: `TypeName`

## Expose

**Properties** (stable, theorem statements):
- `property_name`: Description

**Internal helpers** (may change):
- Helper utilities
-/

namespace Aura.Module.Name

-- Imports and opens
open Aura.Consensus.Types
open Aura.Assumptions

/-!
## Section: Predicates
-/

/-- Docstring explaining the predicate. -/
def predicate (x : Type) : Prop := ...

/-!
## Section: Claims Bundle
-/

/-- Claims bundle description. -/
structure ModuleClaims where
  /-- Property description. -/
  property : ∀ x : Type, condition x → result x

/-!
## Section: Proofs
-/

/-- Theorem description. -/
theorem property (x : Type) (h : condition x) : result x := by
  -- proof

/-!
## Section: Claims Bundle Construction
-/

/-- The claims bundle. -/
def moduleClaims : ModuleClaims where
  property := property

end Aura.Module.Name
```

## Naming Conventions

### Types and Structures
- PascalCase for types: `CommitFact`, `WitnessVote`, `Evidence`
- Suffix with purpose: `*Claims` for claim bundles, `*Proof` for proof witnesses

### Functions and Theorems
- snake_case for definitions: `valid_commit`, `has_equivocated`
- Descriptive names reflecting the property: `agreement`, `unique_commit`
- Prefix with action for operations: `merge_evidence`, `detect_equivocation`

### Variables
- Single letters for bound variables in quantifiers: `c`, `e`, `v`, `w`
- Descriptive names in complex proofs: `commit1`, `commit2`
- Hypothesis naming: `h` prefix with description (`hvalid`, `hcid`, `hne`)

## Documentation

### Docstrings
Every public definition requires a docstring:

```lean
/-- A CommitFact is valid if its signature verifies correctly.
    Quint: Valid commits have threshold signatures bound to (cid, rid, pHash). -/
def validCommit (c : CommitFact) : Prop := ...
```

### Section Headers
Use `/-! -/` blocks to organize logical sections:

```lean
/-!
## Agreement Predicates

Predicates expressing agreement properties.
-/
```

### Expose Blocks
Document the module's API contract in the header:

```lean
## Expose

**Properties** (stable, theorem statements):
- `agreement`: If two commits exist for same consensus, they have same result

**Internal helpers** (may change):
- Auxiliary lemmas about signature verification
```

## Claims Bundle Pattern

### Purpose
Claims bundles separate theorem statements from proofs, enabling:
1. Quick auditing of what's proven
2. Type-level verification of completeness
3. Clear distinction between stable API and implementation

### Structure

```lean
/-- Claims bundle for Module properties. -/
structure ModuleClaims where
  /-- Property 1 description. -/
  prop1 : ∀ x, condition1 x → result1 x

  /-- Property 2 description. -/
  prop2 : ∀ x y, condition2 x y → result2 x y

/-- Individual theorem proof. -/
theorem prop1 (x : Type) (h : condition1 x) : result1 x := by
  -- proof body

/-- The claims bundle, constructed from individual proofs. -/
def moduleClaims : ModuleClaims where
  prop1 := prop1
  prop2 := prop2
```

### Placeholder Proofs
Use `sorry` with explanatory comments for incomplete proofs:

```lean
theorem complex_property : ... := by
  sorry  -- Requires FROST uniqueness axiom; see Assumptions.lean
```

## Proof Style

### Tactic Proofs
Prefer tactic mode for non-trivial proofs:

```lean
theorem property (h1 : P) (h2 : Q) : R := by
  unfold definition at h1
  cases h2 with
  | case1 => exact ...
  | case2 => ...
```

### Term Proofs
Use term mode for simple proofs:

```lean
theorem trivial_property (h : P) : P := h
```

### Common Tactics

| Tactic | Use Case |
|--------|----------|
| `unfold` | Expand definitions |
| `exact` | Provide exact term |
| `cases` | Case analysis |
| `obtain` | Destructure existentials |
| `omega` | Linear arithmetic |
| `trivial` | Solve trivial goals |
| `sorry` | Placeholder (document reason) |

### Proof Organization
- One tactic per line for readability
- Comment complex steps
- Group related hypotheses

## Correspondence Comments

### Quint Correspondence
Link to Quint model for model-proof consistency:

```lean
## Quint Correspondence
- File: verification/quint/protocol_consensus.qnt
- Section: INVARIANTS
- Invariant: `InvariantUniqueCommitPerInstance`
```

### Rust Correspondence
Link to Rust implementation for spec-impl traceability:

```lean
## Rust Correspondence
- File: crates/aura-protocol/src/consensus/types.rs
- Type: `CommitFact`
- Function: `verify_commit`
```

## Pure Lean 4

### No Mathlib Dependency
This project uses pure Lean 4 without Mathlib. Implement needed utilities locally:

```lean
/-- Remove duplicates from a list using BEq. -/
def List.removeDups [BEq α] : List α → List α
  | [] => []
  | x :: xs => if xs.elem x then List.removeDups xs else x :: List.removeDups xs
```

### Character Encoding
Prefer ASCII when Unicode causes issues:

| Unicode | ASCII Alternative |
|---------|-------------------|
| `→` | `->` |
| `∀` | `forall` |
| `∧` | `/\` |
| `∨` | `\/` |
| `≥` | `>=` |
| `≤` | `<=` |
| `≠` | `!=` |

## Axiom Management

### Centralized Assumptions
All axioms live in `Aura/Assumptions.lean`:

```lean
-- FROST threshold unforgeability
axiom frost_threshold_unforgeability : ...

-- Hash collision resistance
axiom hash_collision_resistance : ...

-- Byzantine threshold
axiom byzantine_threshold : threshold > maxByzantine
```

### Using Axioms
Reference axioms explicitly in proofs:

```lean
theorem uses_axiom : ... := by
  have h := frost_threshold_unforgeability
  ...
```

## Proof Status Tracking

### In Proofs.lean
Document proof completion status:

```lean
## Proof Status

**Completed Proofs** (no sorry):
- Evidence CRDT merge preserves commit
- Validity threshold reflexivity

**Placeholder Proofs** (sorry):
- Agreement between valid commits (requires FROST axioms)
```

### Sorry Documentation
Always document why `sorry` is used:

```lean
theorem incomplete : ... := by
  sorry  -- Requires list dedup commutativity lemma
```

## Build Verification

Before committing:

```bash
# Build all proofs
cd verification/lean
lake build

# Expected: warnings about sorry, no errors
```

## File Checklist

For each new proof module:

- [ ] Header with module description
- [ ] Quint correspondence section
- [ ] Rust correspondence section
- [ ] Expose block documenting API
- [ ] Claims bundle structure
- [ ] Individual theorem proofs
- [ ] Claims bundle construction
- [ ] All definitions have docstrings
- [ ] Sorry usage documented with reason
- [ ] Added to Proofs.lean re-exports
