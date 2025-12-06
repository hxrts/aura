# Verification Guide

Aura uses two complementary formal verification systems. Lean 4 provides mathematical proofs of correctness for kernel invariants. Quint provides state machine specifications and model checking for distributed protocols.

## Purpose

Some properties cannot be adequately tested through empirical methods. CRDT merge associativity, key derivation isolation, and FROST protocol state machine correctness require mathematical proof. Distributed protocol liveness and Byzantine fault tolerance require exhaustive state space exploration.

Lean 4 modules model kernel behavior and prove invariants hold under all possible inputs. Quint specifications model protocol state machines and verify safety and liveness properties through simulation and model checking.

## Verification System Architecture

The verification system spans three architectural layers.

```
aura-core::effects::quint   (Layer 1: Effect traits)
        │
aura-quint                  (Layer 3: Implementation)
        │
aura-simulator::quint       (Layer 6: Runtime integration)
```

The `aura-core` layer defines effect traits for property evaluation and verification. The `aura-quint` crate implements these traits using the native Quint evaluator. The `aura-simulator` module integrates property evaluation during simulation runs.

## Lean 4 Theorem Proving

Lean 4 verification modules are organized in the `lean/` directory. Each module corresponds to a kernel component from the Rust implementation.

### Module Structure

```
lean/Aura/
├── Journal/
│   ├── Core.lean          # Journal definitions and merge operation
│   └── Semilattice.lean   # Semilattice proofs
├── KeyDerivation/Core.lean
├── GuardChain/Core.lean
├── FlowBudget/Core.lean
├── Frost/Core.lean
└── TimeSystem/Core.lean
```

The `Journal` module proves CRDT properties including merge associativity, commutativity, and idempotence. The `KeyDerivation` module proves contextual isolation for derived keys. The `GuardChain` module proves cost calculation monotonicity.

The `FlowBudget` module proves charging properties that prevent resource exhaustion. The `Frost` module proves state machine correctness for threshold signatures. The `TimeSystem` module proves ordering transitivity and reflexivity.

### Building Lean Modules

```bash
nix develop
just lean-init
```

The `lean-init` command initializes the Lake project and downloads dependencies. This command runs once after cloning the repository.

```bash
just lean-build
```

The `lean-build` command compiles all Lean modules and verifies proofs. Build output shows which theorems are proven and which use `sorry` placeholders.

```bash
just lean-full
```

The `lean-full` command runs a complete verification cycle. It cleans previous builds, rebuilds all modules, and runs verification checks. Use this command before committing changes.

### Writing Theorems

```lean
theorem merge_comm (j1 j2 : Journal) :
  merge j1 j2 = merge j2 j1 := by
  sorry
```

Theorem statements define what must be proven. Start with `sorry` placeholders to validate the statement compiles before investing time in proof development.

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

### Common Tactics

| Tactic | Purpose |
|--------|---------|
| `intro` | Introduce hypotheses |
| `unfold` | Expand definitions |
| `split` | Case analysis on conditionals |
| `cases` | Case analysis on inductive types |
| `induction` | Structural induction |
| `simp` | Simplification |
| `rfl` | Reflexivity for definitional equality |
| `omega` | Linear arithmetic |

Structural induction proves properties about recursive types. Use the `induction` tactic to generate base and inductive cases. Definitional equality simplifies goals automatically when both sides are equal by definition.

## Quint Model Checking

Quint specifications model distributed protocol state machines. The `aura-quint` crate provides a native Rust interface to the Quint evaluator. Property evaluation integrates with the simulator for real-time validation.

### Crate Structure

```
crates/aura-quint/
├── lib.rs           # Public API exports
├── evaluator.rs     # Native Quint subprocess interface
├── handler.rs       # Effect trait implementations
├── runner.rs        # Verification runner with caching
├── properties.rs    # Property specification management
└── types.rs         # Core type definitions
```

The `evaluator` module provides subprocess-based parsing using `quint parse --output=json`. The `handler` module implements `QuintEvaluationEffects` and `QuintVerificationEffects` traits. The `runner` module provides caching, counterexample generation, and parallel execution.

### Quint Specifications

Formal specifications are organized in `verification/quint/` and `crates/aura-simulator/tests/quint_specs/`. Each protocol has a core specification and a harness for simulator integration.

| Specification | Description |
|---------------|-------------|
| `protocol_core.qnt` | Runtime utilities and state machine definitions |
| `protocol_dkg.qnt` | Distributed Key Generation |
| `protocol_resharing.qnt` | Threshold key resharing |
| `protocol_recovery.qnt` | Guardian recovery flows |
| `protocol_counter.qnt` | Counter reservation with Lamport clocks |
| `protocol_sessions.qnt` | Session management |
| `protocol_journal.qnt` | Ledger event tracking |
| `protocol_capability_properties.qnt` | Guard chain, budget, and integrity verification |
| `session_types.qnt` | Session type state machine properties |
| `journal_effect_api.qnt` | Journal CRDT and event authorization |

Harness specifications expose standard action entry points. The `register()` action initializes protocols. The `complete()` action handles successful completion. The `abort()` action handles failure with reason codes.

### Building Quint Specifications

```bash
just quint-parse verification/quint/protocol_dkg.qnt output.json
```

The parse command converts Quint specifications to JSON IR format. The simulator consumes this format for property evaluation.

```bash
just quint-compile verification/quint/protocol_dkg.qnt output.json
```

The compile command includes full type checking. Use this for validation before integration testing.

```bash
just verify-quint
just test-quint-pipeline
```

The verify command runs all Quint specifications through the parser. The pipeline test validates end-to-end integration with the simulator.

### Property Types

Quint specifications define several property types for verification.

| Property Type | Purpose |
|---------------|---------|
| Safety | Bad states are never reached |
| Liveness | Good states are eventually reached |
| Invariant | Property holds in all reachable states |
| Temporal | Property holds across state sequences |

Safety properties verify that invalid states cannot occur. Liveness properties verify that the system makes progress. Invariants define conditions that must hold in every reachable state.

### Capability Property Verification

The `protocol_capability_properties.qnt` specification verifies Aura's core security properties. These properties correspond to the guard chain architecture documented in `docs/003_information_flow_contract.md`.

#### Property Categories

| Category | Guard | Purpose |
|----------|-------|---------|
| Authorization | CapGuard | Verify capability grants follow meet-semilattice rules |
| Budget | FlowGuard | Verify charge-before-send and resource accounting |
| Integrity | JournalCoupler | Verify attenuation-only and receipt chain correctness |

The `aura-quint` runner automatically classifies properties by detecting keywords in property names. Authorization properties contain keywords like `grant`, `permit`, `guard`, or `authorization`. Budget properties contain `budget`, `charge`, `spent`, or `flowguard`. Integrity properties contain `attenuation`, `signature`, or `chain`.

#### Authorization Invariants

```
guardChainOrder: All completed operations follow CapGuard → FlowGuard → JournalCoupler → TransportSend
noCapabilityWidening: Attenuation count only increases (capabilities never widen)
authorizationSoundness: Temporal property ensuring all operations pass full guard chain
```

Authorization verification ensures that every transport operation passes through the complete guard chain in the correct order. The meet-semilattice property guarantees capabilities can only be narrowed, never expanded.

#### Budget Invariants

```
chargeBeforeSend: Every TransportSend operation has charged=true
spentWithinLimit: spent ≤ limit for all flow budgets
noTransportWithoutFlowGuard: TransportSend implies FlowGuard in guard steps
budgetMonotonicity: Spent counters are always non-negative
flowBudgetFairness: All budget limits are positive
```

Budget verification ensures the charge-before-send invariant holds. No message can be sent without first charging the flow budget. The `spent + cost ≤ limit` constraint prevents resource exhaustion attacks.

#### Integrity Invariants

```
attenuationOnlyNarrows: Capability levels remain within valid bounds after attenuation
receiptChainIntegrity: Receipt chain hashes are preserved (verified via hash consistency)
receiptIntegrity: Temporal property for receipt chain correctness
```

Integrity verification ensures Biscuit tokens can only be attenuated (narrowed), never forged or expanded. Receipt chains provide cryptographic accountability for all transport operations.

#### Running Capability Verification

```bash
just quint-parse verification/quint/protocol_capability_properties.qnt output.json
```

The specification models the guard chain state machine with actions for context initialization, authority creation, transport operations, and capability attenuation. The `step` relation explores the state space through non-deterministic choices.

### Quint Syntax Patterns (v0.25.x)

Quint 0.25.x has specific syntax requirements. The following patterns avoid common errors.

#### Type Definitions

Type imports don't work in Quint. Define types locally in each module:

```quint
// ❌ WRONG: Type imports fail
import protocol_core as core from "protocol_core"
type MyId = core.AuthorityId  // Error: types can't be imported

// ✅ CORRECT: Define types locally
type AuthorityId = str
type ContextId = str
type ProtocolId = str
```

#### Reserved Keywords

These identifiers are reserved and cannot be used as variable or parameter names:

| Reserved | Use Instead |
|----------|-------------|
| `from` | `sender`, `source`, `origin` |
| `to` | `recipient`, `target`, `destination` |
| `next` | `targetState`, `successor`, `nextState` |
| `fail` | `failProtocol`, `failX`, `abort` |

#### Conditional Expressions

Quint uses `if (cond) expr1 else expr2` without `then`:

```quint
// ❌ WRONG: Using 'then'
val result = if x > 0 then "positive" else "non-positive"

// ✅ CORRECT: No 'then' keyword
val result = if (x > 0) "positive" else "non-positive"
```

#### Val Bindings in Actions

Val bindings must be declared before `all {}` blocks:

```quint
// ❌ WRONG: Val inside all block
action myAction(pid: ProtocolId): bool = all {
  val phase = phases.get(pid)  // Error
  phase == Active,
  // ...
}

// ✅ CORRECT: Val before all block
action myAction(pid: ProtocolId): bool = {
  val phase = if (pid.in(phases.keys())) phases.get(pid) else Idle
  all {
    phase == Active,
    // ...
  }
}
```

#### Sum Type Variants

Sum type variant names must be globally unique within a module:

```quint
// ❌ WRONG: Conflicting variant names
type Phase = Active | Pending | Completed | Failed
type Lifecycle = Pending | Completed | Aborted  // Error: Pending, Completed conflict

// ✅ CORRECT: Unique variant names
type Phase = PhaseActive | PhasePending | PhaseCompleted | PhaseFailed
type Lifecycle = LifecyclePending | LifecycleCompleted | LifecycleAborted
```

#### Map Operations

Maps don't have `values()`. Use `keys()` iteration:

```quint
// ❌ WRONG: values() doesn't exist
val allValid = myMap.values().forall(v => v > 0)

// ✅ CORRECT: Iterate via keys
val allValid = myMap.keys().forall(k => myMap.get(k) > 0)
```

#### Set Operations with forall/exists

`forall` and `exists` only work on Sets, not Lists:

```quint
// ❌ WRONG: forall on List
val myList: List[int] = [1, 2, 3]
val allPositive = myList.forall(x => x > 0)  // Error

// ✅ CORRECT: Use foldl for Lists
val allPositive = myList.foldl(true, (acc, x) => acc and x > 0)

// ✅ CORRECT: forall on Sets
val mySet: Set[int] = Set(1, 2, 3)
val allPositive = mySet.forall(x => x > 0)
```

#### String Operations

Strings don't have `length()`. Use comparison for non-empty checks:

```quint
// ❌ WRONG: No length method
val isValid = label.length() > 0

// ✅ CORRECT: Compare with empty string
val isValid = label != ""
```

#### Module Imports for Actions

Import modules and use `::` for action calls:

```quint
// Import module
import protocol_dkg from "./protocol_dkg"

// Call actions with module prefix
action register(pid: ProtocolId, members: Set[AuthorityId]): bool = all {
  protocol_dkg::configureDkg(pid, members, 2),
  // ...
}
```

### Simulator Integration

The `aura-simulator::quint` module provides property evaluation during simulation.

```
crates/aura-simulator/src/quint/
├── properties.rs           # Property extraction and monitoring
├── itf_fuzzer.rs          # ITF-based fuzz testing
├── trace_converter.rs     # ITF trace conversion
├── simulation_evaluator.rs # Property evaluation engine
├── chaos_generator.rs     # Byzantine scenario generation
└── byzantine_mapper.rs    # Byzantine role mapping
```

The property evaluator validates properties in real-time during simulation runs. The ITF fuzzer generates test cases from formal specifications. The chaos generator injects Byzantine scenarios for fault tolerance testing.

### ITF Trace Format

The simulator uses Informal Trace Format for trace exchange with external tools. ITF provides bidirectional conversion between Aura simulation traces and Quint model traces.

Traces include metadata, variable definitions, and state sequences. Type preservation ensures semantic correctness across conversions. Variable consistency validation prevents invalid trace exchange.

## Verification Priorities

The verification roadmap prioritizes components by security impact.

### High Priority

CRDT journal merge correctness is highest priority. Incorrect merge semantics break distributed state reconciliation. Key derivation contextual isolation prevents cross-context correlation attacks.

Guard chain cost calculation must be monotonic. Incorrect cost calculation enables denial-of-service attacks. Flow budget charging properties must maintain accounting invariants.

### Medium Priority

FROST protocol state machine verification prevents signature forgery. The aggregate function must validate session and round consistency. TimeStamp ordering properties ensure correct causality tracking.

## IDE Setup

Lean development requires the Lean Language Server. Use Visual Studio Code with the lean4 extension or Emacs with lean4-mode. The LSP provides interactive proof development with goal state visualization.

Quint development uses standard text editors. The Quint CLI provides parsing and type checking feedback. Apalache provides model checking through the `apalache-mc` command.

## Integration with CI

Verification integrates with continuous integration. The CI workflow runs `just lean-build` and fails if any proofs use `sorry`. The workflow also runs Quint parsing to validate specification syntax.

Proof breakage indicates either a bug in the change or a proof that needs updating. Quint parse failures indicate specification syntax errors that must be fixed before merging.

## Current Status

Lean theorem statements are defined with `sorry` placeholders. The module structure is complete and builds successfully. The next phase is completing proofs for high-priority components.

Quint integration is production-ready. The `aura-quint` crate compiles with zero errors. All 18 protocol specifications parse successfully. Property evaluation integrates with the simulator runtime.

## References

Lean 4 resources:
- [Lean 4 Manual](https://lean-lang.org/lean4/doc/)
- [Theorem Proving in Lean 4](https://lean-lang.org/theorem_proving_in_lean4/)
- [Mathlib4 Documentation](https://leanprover-community.github.io/mathlib4_docs/)
- [Lean 4 DeepWiki](https://deepwiki.com/leanprover/lean4)

Quint resources:
- [Quint Language Documentation](https://quint-lang.org/)
- [Quint DeepWiki](https://deepwiki.com/informalsystems/quint)

Related guides:
- [Testing Guide](805_testing_guide.md)
- [Effect System Guide](106_effect_system_and_runtime.md)
- [Simulation Guide](806_simulation_guide.md)

Verification complements but does not replace testing. Use verification for mathematical properties and protocol correctness. Use testing for integration and behavior validation.
