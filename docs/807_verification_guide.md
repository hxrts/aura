# Verification Guide

This guide explains Aura's formal verification system using Lean 4. The verification system provides mathematical proofs of correctness for kernel components that are critical to security and distributed consensus.

## Purpose

Aura uses Lean 4 to formally verify properties that cannot be adequately tested. Properties like CRDT merge associativity, key derivation isolation, and FROST protocol state machine correctness require mathematical proof rather than empirical testing.

The verification system runs alongside Rust code. Lean modules model kernel behavior and prove invariants hold under all possible inputs.

## Module Structure

The verification modules are organized in the `lean/` directory. Each module corresponds to a kernel component from the Rust implementation.

### Journal Verification

```
lean/Aura/Journal/
├── Core.lean          # Journal definitions and merge operation
└── Semilattice.lean   # Semilattice proofs
```

The journal module proves CRDT properties. The `merge` operation must be associative, commutative, and idempotent for correct distributed state reconciliation.

The `Core.lean` module defines the journal type and merge operation. The `Semilattice.lean` module contains proofs that merge forms a valid join-semilattice.

### Key Derivation Verification

```
lean/Aura/KeyDerivation/Core.lean
```

The key derivation module proves contextual isolation. Keys derived for different contexts must be cryptographically independent to prevent cross-context attacks.

### Guard Chain Verification

```
lean/Aura/GuardChain/Core.lean
```

The guard chain module proves evaluation correctness. Cost calculations must be monotonic and correctly track resource consumption across guard evaluation steps.

### Flow Budget Verification

```
lean/Aura/FlowBudget/Core.lean
```

The flow budget module proves charging properties. Budget operations must never increase available budget and charging exact amounts must result in zero budget.

### FROST Protocol Verification

```
lean/Aura/Frost/Core.lean
```

The FROST module proves state machine correctness. The aggregate function must never be called with mixed sessions or rounds to prevent signature forgery.

### TimeStamp Verification

```
lean/Aura/TimeSystem/Core.lean
```

The timestamp module proves ordering and privacy properties. Timestamp comparisons must be transitive and reflexive while preserving privacy guarantees.

## Building and Running

The verification system integrates with the Nix development environment. Build commands are available through the Justfile.

### Initial Setup

```bash
nix develop
just lean-init
```

The `lean-init` command initializes the Lake project and downloads dependencies. This command needs to run once after cloning the repository or after cleaning build artifacts.

### Building Modules

```bash
just lean-build
```

The `lean-build` command compiles all Lean modules and verifies proofs. Build output shows which theorems are proven and which use `sorry` placeholders.

Lake compiles modules in parallel. Build times depend on proof complexity and available CPU cores.

### Full Workflow

```bash
just lean-full
```

The `lean-full` command runs a complete verification cycle. It cleans previous builds, rebuilds all modules, and runs verification checks.

Use this command before committing changes to ensure all proofs still verify.

## Development Workflow

Verification development follows a two-phase approach. Phase one creates theorem statements with `sorry` placeholders. Phase two completes the proofs.

### Creating Theorem Statements

```lean
theorem merge_comm (j1 j2 : Journal) :
  merge j1 j2 = merge j2 j1 := by
  sorry
```

Theorem statements define what must be proven. The statement includes the property name, parameters, type signature, and proof placeholder.

Start with `sorry` placeholders to validate the theorem statement compiles. This ensures the property is correctly specified before investing time in proof development.

### Completing Proofs

```lean
theorem charge_decreases (budget : Budget) (cost : Nat) (result : Budget) :
  charge budget cost = some result →
  result.available ≤ budget.available := by
  intro h
  unfold charge at h
  split at h
  · simp [h]
  · contradiction
```

Proof tactics transform the goal into simpler subgoals. Common tactics include `intro` for introducing hypotheses, `unfold` for expanding definitions, `split` for case analysis, and `simp` for simplification.

Lean provides interactive proof development. The LSP server shows the current goal state after each tactic application.

### Proof Structure

Proofs follow a structured approach. Introduce assumptions with `intro`. Unfold definitions with `unfold`. Apply case analysis with `split` or `cases`. Simplify goals with `simp` or `omega`.

Complex proofs benefit from lemmas. Break large proofs into smaller lemmas that build toward the main theorem.

## Verification Priorities

The verification roadmap prioritizes components by security impact. High priority components receive proofs first.

### High Priority

The CRDT journal merge operation is highest priority. Incorrect merge semantics break distributed state reconciliation.

Key derivation contextual isolation is critical for privacy. Context leakage enables cross-context correlation attacks.

Guard chain evaluation correctness ensures resource limits are enforced. Incorrect cost calculation enables denial-of-service attacks.

Flow budget charging properties prevent resource exhaustion. Budget operations must maintain accounting invariants.

### Medium Priority

FROST protocol state machine verification prevents signature forgery. The aggregate function must validate session and round consistency.

TimeStamp ordering properties ensure causality tracking. Transitivity and reflexivity are required for correct ordering semantics.

## Future Integration

The verification system will integrate with Rust through differential testing. The Lean runner will provide a CLI that Rust tests can invoke.

### Planned Runner Interface

```bash
# Verify journal merge operation
aura_verifier journal-merge --input merge_test_case.json

# Verify guard evaluation
aura_verifier guard-evaluate --input guard_test_case.json
```

The runner will accept JSON-serialized inputs from Rust tests. It will execute the Lean model and return results for comparison.

Differential testing validates the Rust implementation matches the verified Lean model. Mismatches indicate implementation bugs or model errors.

### Current Limitations

The executable runner is not yet functional. The `lean_exe` target in `lakefile.lean` is currently disabled due to linking issues.

Focus is on completing library proofs. The runner implementation will follow once core theorems are proven.

## Working with Lean

Lean development requires understanding dependent type theory and tactics. The Lean 4 manual and mathlib4 documentation provide comprehensive references.

### IDE Setup

Lean development requires the Lean Language Server. Use Visual Studio Code with the lean4 extension or Emacs with lean4-mode.

The LSP provides interactive proof development. Hover over tactics to see goal state. Use the Lean Infoview panel to track proof progress.

### Common Patterns

Structural induction proves properties about recursive types. Use the `induction` tactic to generate base and inductive cases.

Definitional equality simplifies goals automatically. Use `rfl` when both sides of an equality are definitionally equal.

Case analysis handles conditional logic. Use `split` to generate subgoals for each branch of an if-then-else or match expression.

### Debugging Proofs

Failed proofs show unsolved goals. Read the goal state carefully to understand what remains to be proven.

The `trace` tactic prints intermediate state. Use it to inspect variable values during proof development.

Simplification tactics may loop or simplify incorrectly. Use `simp?` to see which simplification lemmas were applied.

## Integration with CI

Verification will integrate with continuous integration once proofs are complete. The CI workflow will run `just lean-build` and fail if any proofs use `sorry`.

This ensures new changes do not break existing proofs. Proof breakage indicates either a bug in the change or a proof that needs updating.

The CI workflow will also run differential tests comparing Rust implementations against Lean models. This provides continuous validation that implementations match specifications.

## Current Status

All theorem statements are defined with `sorry` placeholders. The module structure is complete and builds successfully.

The next phase is completing proofs for high-priority components. Journal merge properties are the first target.

Documentation will expand as proofs are completed. Specific proof techniques and patterns will be documented for each verification domain.

## References

Lean 4 resources:
- [Lean 4 Manual](https://lean-lang.org/lean4/doc/)
- [Theorem Proving in Lean 4](https://lean-lang.org/theorem_proving_in_lean4/)
- [Mathlib4 Documentation](https://leanprover-community.github.io/mathlib4_docs/)

Related guides:
- [Testing Guide](805_testing_guide.md) - Rust testing infrastructure
- [Effect System Guide](106_effect_system_and_runtime.md) - Kernel effect architecture

The verification system complements but does not replace testing. Use verification for mathematical properties and testing for integration and behavior validation.
