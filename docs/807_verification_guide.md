# Verification and MBT Guide

This guide covers how to use formal verification and model-based testing to validate Aura protocols. It focuses on practical workflows with Quint, Lean, and generative testing.

## When to Verify

Verification suits protocols with complex state machines or security-critical properties. Use Quint model checking for exhaustive state exploration. Use Lean proofs for mathematical guarantees. Use generative testing to validate implementations against models.

Unit tests suffice for simple, well-understood behavior. Do not over-invest in verification for straightforward code.

See [Formal Verification Reference](120_verification.md) for the complete architecture documentation.

## Writing Quint Specifications

### Getting Started

Create a new specification in `verification/quint/`.

```quint
module protocol_example {
    type State = { phase: str, value: int }
    var state: State

    action init = {
        state' = { phase: "setup", value: 0 }
    }

    action increment(amount: int): bool = all {
        amount > 0,
        state' = { ...state, value: state.value + amount }
    }

    val nonNegative = state.value >= 0
}
```

Run `quint typecheck` to validate syntax. Run `quint run` to simulate execution.

### Authority Model

Specifications should use `AuthorityId` for identity, not `DeviceId`. Model relational semantics without device-level details.

```quint
type AuthorityId = str
type Participant = { authority: AuthorityId, role: Role }
```

This aligns specifications with Aura's authority-centric design.

### State Machine Design

Define clear phases with explicit transitions.

```quint
type Phase = Setup | Active | Completed | Failed

action transition(target: Phase): bool = all {
    state.phase != Completed,
    state.phase != Failed,
    validTransition(state.phase, target),
    state' = { ...state, phase: target }
}
```

Disallow transitions from terminal states. Validate transition legality explicitly.

### Invariant Design

Define invariants before actions. Clear invariants guide action design.

```quint
val safetyInvariant = or {
    state.phase != Failed,
    hasRecoveryPath(state)
}

val progressInvariant = state.step < MAX_STEPS
```

Invariants should be checkable at every state. Avoid invariants that require execution history.

### Harness Modules

Create harness modules for simulation and trace generation.

```quint
module harness_example {
    import protocol_example.*

    action register(): bool = init
    action step(amount: int): bool = increment(amount)
    action done(): bool = state.phase == Completed
}
```

Harnesses provide standardized entry points for tooling.

## Model Checking Workflow

### Type Checking

```bash
quint typecheck verification/quint/protocol_example.qnt
```

Type checking validates syntax and catches type errors. Run it before any other operation.

### Simulation

```bash
quint run --main=harness_example verification/quint/protocol_example.qnt
```

Simulation executes random traces. It finds bugs quickly but does not provide exhaustive coverage.

### Invariant Checking

```bash
quint run --invariant=safetyInvariant verification/quint/protocol_example.qnt
```

Invariant checking verifies properties hold across simulated traces.

### Model Checking with Apalache

```bash
quint verify --max-steps=10 --invariant=safetyInvariant verification/quint/protocol_example.qnt
```

Apalache performs exhaustive model checking. It proves invariants hold for all reachable states up to the step bound.

### Interpreting Violations

Violations produce counterexample traces. The trace shows the state sequence leading to the violated invariant.

```
[State 0] phase: Setup, value: 0
[State 1] phase: Active, value: 5
[State 2] phase: Active, value: -3  <- VIOLATION: nonNegative
```

Use counterexamples to identify specification bugs or missing preconditions.

## Generative Testing Workflow

Generative testing validates Rust implementations against Quint models.

### The Trust Chain

```
Quint Specification
       │
       ▼ generates
   ITF Traces
       │
       ▼ replayed through
   Rust Effect Handlers
       │
       ▼ produces
   Property Verdicts
```

Each link adds verification value. Specifications validate design. Traces validate reachability. Replay validates implementation.

### Generating Traces

```bash
quint run --main=harness_example --out-itf=trace.itf.json verification/quint/protocol_example.qnt
```

ITF traces capture state sequences and non-deterministic choices.

### Direct Conformance Testing

The recommended approach compares Rust behavior to Quint expected states.

```rust
use aura_simulator::quint::itf_loader::ITFLoader;

#[test]
fn test_matches_quint() {
    let trace = ITFLoader::load("trace.itf.json").unwrap();

    for states in trace.states.windows(2) {
        let pre = State::from_quint(&states[0]).unwrap();
        let action = states[1].meta.action.as_deref().unwrap();

        let actual = apply_action(&pre, action).unwrap();
        let expected = State::from_quint(&states[1]).unwrap();

        assert_eq!(actual, expected);
    }
}
```

This tests production code directly. Quint serves as the single source of truth.

### Generative Exploration

For state space exploration with Rust-driven non-determinism, use the action registry.

```rust
use aura_simulator::quint::action_registry::ActionRegistry;

let mut registry = ActionRegistry::new();
registry.register("increment", Box::new(IncrementHandler));

let result = registry.execute("increment", &params, &effects).await?;
```

This approach requires handlers that re-implement Quint logic. Prefer direct conformance testing for new protocols.

## ITF Trace Handling

### Loading Traces

```rust
use aura_simulator::quint::itf_loader::ITFLoader;

let trace = ITFLoader::load("trace.itf.json")?;
for state in &trace.states {
    let index = state.meta.index;
    let action = state.meta.action.as_deref();
    let picks = &state.meta.nondet_picks;
}
```

The loader parses ITF JSON into typed Rust structures.

### Non-Deterministic Choices

ITF traces capture non-deterministic choices for reproducible replay.

```json
{
  "#meta": { "index": 3, "nondet_picks": { "leader": "alice" } }
}
```

The simulator injects these choices into `RandomEffects` to ensure deterministic replay.

### State Mapping

Types implementing `QuintMappable` convert between Quint and Rust representations.

```rust
use aura_core::effects::quint::QuintMappable;

let rust_state = State::from_quint(&quint_value)?;
let quint_value = rust_state.to_quint();
```

Bidirectional mapping enables state comparison during replay.

## Feeding Conformance Corpus

MBT traces should feed conformance testing.

### Deriving Seeds

```bash
AURA_CONFORMANCE_ITF_TRACE=trace.itf.json cargo test conformance
```

ITF traces become inputs for native/WASM conformance lanes.

### Coupling Model to Corpus

When Quint models change, regenerate traces and update the conformance corpus. This couples model evolution to test coverage.

```bash
quint run --out-itf=traces/new_trace.itf.json verification/quint/updated_spec.qnt
just ci-conformance
```

See [Conformance and Parity Reference](119_conformance.md) for corpus policy details.

## Lean Proof Development

### Adding Theorems

Create or extend modules in `verification/lean/Aura/Proofs/`.

```lean
theorem new_property : ∀ s : State, isValid s → preservesInvariant s := by
  intro s h
  simp [isValid, preservesInvariant] at *
  exact h
```

Use Lean 4 tactic mode for proofs.

### Using Claims Bundles

Access related theorems through claims bundles.

```lean
import Aura.Proofs.Consensus

#check Aura.Consensus.Validity.validityClaims.commit_has_threshold
```

Bundles organize proofs by domain.

### Working with Axioms

Cryptographic assumptions appear in `Assumptions.lean`.

```lean
axiom frost_threshold_unforgeability : ...
```

Proofs depending on axioms are sound under standard hardness assumptions. Document axiom dependencies clearly.

### Building Proofs

```bash
cd verification/lean
lake build
```

The build succeeds only if all proofs complete without `sorry`.

### Checking Status

```bash
just lean-status
```

This reports per-module proof status including incomplete proofs.

## Running Verification

### Quint Commands

```bash
quint typecheck spec.qnt           # Type check
quint run --main=harness spec.qnt  # Simulate
quint run --invariant=inv spec.qnt # Check invariant
quint verify --max-steps=10 spec.qnt # Model check
```

### Lean Commands

```bash
just lean-build        # Build proofs
just lean-status       # Check status
just test-differential # Rust vs Lean tests
```

### Full Verification

```bash
just verify-all
```

This runs Quint model checking, Lean proof building, and conformance tests.

## Best Practices

Start with invariants. Define properties before implementing actions. Clear invariants guide design.

Use unique variant names. Quint requires globally unique sum type variants. Prefix with domain names.

Test harnesses separately. Verify harness modules parse before integrating with the simulator.

Start with short traces. Debug action mappings with 3-5 step traces before exhaustive exploration.

Isolate properties. Test one property at a time during development. Combine for coverage testing.

## Related Documentation

See [Formal Verification Reference](120_verification.md) for architecture details. See [Simulation Guide](806_simulation_guide.md) for trace replay. See [Conformance and Parity Reference](119_conformance.md) for parity testing.
