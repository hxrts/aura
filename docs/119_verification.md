# Formal Verification Reference

This document describes the formal verification infrastructure that provides mathematical guarantees for Aura protocols through Quint model checking, Lean theorem proving, and Telltale session type verification.

## Overview

Aura uses three complementary verification systems. Quint provides executable state machine specifications with model checking. Lean provides mathematical theorem proofs. Telltale provides session type guarantees for choreographic protocols.

The systems form a trust chain. Quint specifications define correct behavior. Lean proofs verify mathematical properties. Telltale ensures protocol implementations match session type specifications.

## Verification Boundary

Aura separates domain proof ownership from runtime parity checks.

| Verification Surface | Primary Tools | Guarantee Class | Ownership |
|----------------------|--------------|-----------------|-----------|
| Consensus and CRDT domain properties | Quint + Lean | model and theorem correctness | `verification/quint/` and `verification/lean/` |
| Runtime execution conformance | Telltale parity + conformance artifacts | implementation parity under declared envelopes | `aura-agent`, `aura-simulator`, `aura-testkit` |
| Bridge consistency | `aura-quint` bridge pipeline | cross-validation between model checks and certificates | `aura-quint` |

Telltale runtime parity does not replace domain theorem work. It validates runtime behavior against admitted profiles and artifact envelopes.

## Assurance Summary

This architecture provides five assurance classes.

1. Boundary assurance.
Domain theorem claims and runtime parity claims are separated.
This reduces proof-surface ambiguity.

2. Runtime parity assurance.
Telltale parity lanes compare runtime artifacts with deterministic profiles.
This provides replayable evidence for conformance under declared envelopes.

3. Bridge consistency assurance.
Bridge pipelines check model-check outcomes against certificate outcomes.
This detects drift between proof artifacts and executable checks.

4. CI gate assurance.
Parity and bridge lanes run as CI gates.
This prevents silent regression of conformance checks.

5. Coverage drift assurance.
Coverage documentation is validated against repository state by script checks.
This prevents long-term drift between claims and implementation.

Limits remain explicit.
Parity success is not a replacement for new Quint or Lean domain proofs.
Parity checks are coverage-bounded by scenarios, seeds, and artifact surfaces.

## Quint Architecture

Quint specifications live in `verification/quint/`. They define protocol state machines and verify properties through model checking with Apalache.

### Directory Structure

```
verification/quint/
├── core.qnt               # Shared runtime utilities
├── authorization.qnt      # Guard chain security
├── recovery.qnt           # Guardian recovery
├── consensus/             # Consensus protocol specs
│   ├── core.qnt
│   ├── liveness.qnt
│   └── adversary.qnt
├── journal/               # Journal CRDT specs
│   ├── core.qnt
│   ├── counter.qnt
│   └── anti_entropy.qnt
├── keys/                  # Key management specs
│   └── dkg.qnt
├── sessions/              # Session management specs
│   ├── core.qnt
│   └── groups.qnt
├── harness/               # Simulator harnesses
├── tui/                   # TUI state machine
└── traces/                # Generated ITF traces
```

Each specification focuses on a single protocol or subsystem.

### Specification Pattern

Specifications follow a consistent structure.

```quint
module protocol_example {
    // Type definitions
    type Phase = Setup | Active | Completed | Failed
    type State = { phase: Phase, data: Data }

    // State variables
    var state: State

    // Initial state
    action init = {
        state' = { phase: Setup, data: emptyData }
    }

    // State transitions
    action transition(input: Input): bool = all {
        state.phase != Completed,
        state.phase != Failed,
        state' = computeNextState(state, input)
    }

    // Invariants
    val safetyInvariant = state.phase != Failed or hasRecoveryPath(state)
}
```

Actions define state transitions. Invariants define properties that must hold in all reachable states.

### Harness Modules

Harness modules provide standardized entry points for simulation.

```quint
module harness_example {
    import protocol_example.*

    action register(id: Id): bool = init
    action step(input: Input): bool = transition(input)
    action complete(): bool = state.phase == Completed
}
```

Harnesses enable Quint simulation and ITF trace generation.

### Available Specifications

| Specification | Purpose | Key Invariants |
|---------------|---------|----------------|
| `consensus/core.qnt` | Fast-path consensus | `UniqueCommitPerInstance`, `CommitRequiresThreshold` |
| `consensus/liveness.qnt` | Liveness properties | `ProgressUnderSynchrony`, `RetryBound` |
| `consensus/adversary.qnt` | Byzantine tolerance | `ByzantineThreshold`, `EquivocationDetected` |
| `journal/core.qnt` | Journal CRDT | `NonceUnique`, `FactsOrdered` |
| `journal/anti_entropy.qnt` | Sync protocol | `FactsMonotonic`, `EventualConvergence` |
| `authorization.qnt` | Guard chain | `NoCapabilityWidening`, `ChargeBeforeSend` |

## Lean Architecture

Lean proofs live in `verification/lean/`. They provide mathematical verification of safety properties.

### Directory Structure

```
verification/lean/
├── lakefile.lean          # Build configuration
├── Aura/
│   ├── Assumptions.lean   # Cryptographic axioms
│   ├── Types.lean         # Core type definitions
│   ├── Types/
│   │   ├── ByteArray32.lean
│   │   └── OrderTime.lean
│   ├── Proofs/
│   │   ├── Consensus/
│   │   │   ├── Agreement.lean
│   │   │   ├── Validity.lean
│   │   │   ├── Equivocation.lean
│   │   │   ├── Liveness.lean
│   │   │   ├── Evidence.lean
│   │   │   ├── Adversary.lean
│   │   │   └── Frost.lean
│   │   ├── Journal.lean
│   │   ├── FlowBudget.lean
│   │   ├── GuardChain.lean
│   │   ├── KeyDerivation.lean
│   │   ├── TimeSystem.lean
│   │   └── ContextIsolation.lean
│   └── Runner.lean        # CLI for differential testing
```

### Axioms

Cryptographic assumptions appear as axioms in `Assumptions.lean`.

```lean
axiom frost_threshold_unforgeability :
  ∀ (k n : Nat) (shares : List Share),
    k ≤ shares.length →
    shares.length ≤ n →
    validShares shares →
    unforgeable (aggregate shares)

axiom hash_collision_resistance :
  ∀ (a b : ByteArray), hash a = hash b → a = b
```

Proofs that depend on these assumptions are sound under standard cryptographic hardness assumptions.

The consensus proofs also depend on domain-level axioms for signature binding. These axioms establish that valid signatures bind to unique results. See `verification/lean/Aura/Assumptions.lean` for the full axiom reduction analysis.

### Claims Bundles

Related theorems group into claims bundles.

```lean
structure ValidityClaims where
  commit_has_threshold : ∀ c, isCommit c → hasThreshold c
  validity : ∀ c, isCommit c → validPrestate c
  distinct_signers : ∀ c, isCommit c → distinctSigners c.shares

def validityClaims : ValidityClaims := {
  commit_has_threshold := Validity.commit_has_threshold
  validity := Validity.validity
  distinct_signers := Validity.distinct_signers
}
```

Bundles provide easy access to related proofs.

### Proof Status

| Module | Status | Notes |
|--------|--------|-------|
| `Validity` | Complete | All theorems proven |
| `Equivocation` | Complete | Detection soundness/completeness |
| `Evidence` | Complete | CRDT properties |
| `Frost` | Complete | Aggregation properties |
| `Agreement` | Uses axiom | Depends on FROST uniqueness |
| `Liveness` | Axioms | Timing assumptions |
| `Journal` | Complete | CRDT semilattice properties |

## aura-quint Crate

The `aura-quint` crate provides Rust integration with Quint specifications.

### QuintRunner

The runner executes Quint verification and parses results.

```rust
use aura_quint::runner::{QuintRunner, RunnerConfig};
use aura_quint::PropertySpec;

let config = RunnerConfig {
    default_timeout: Duration::from_secs(60),
    max_steps: 1000,
    generate_counterexamples: true,
    ..Default::default()
};
let mut runner = QuintRunner::with_config(config)?;
let spec = PropertySpec::invariant("UniqueCommitPerInstance");
let result = runner.verify_property(&spec).await?;
```

The runner provides `verify_property` for invariant checking and `simulate` for trace-based testing. It caches results and can generate counterexamples.

### Property Evaluator

The evaluator checks properties against Rust state.

```rust
use aura_quint::evaluator::PropertyEvaluator;

let evaluator = PropertyEvaluator::new();
let result = evaluator.evaluate("chargeBeforeSend", &state)?;
```

Properties translate from Quint syntax to Rust predicates.

### Property Categories

The evaluator classifies properties by keyword patterns.

| Category | Keywords | Examples |
|----------|----------|----------|
| Authorization | `grant`, `permit`, `guard` | `guardChainOrder` |
| Budget | `budget`, `charge`, `spent` | `chargeBeforeSend` |
| Integrity | `attenuation`, `signature` | `attenuationOnlyNarrows` |
| Liveness | `eventually`, `progress` | `eventualConvergence` |
| Safety | `never`, `always`, `invariant` | `uniqueCommit` |

Categories help organize verification coverage reports.

## Quint Integration in aura-simulator

The simulator provides deeper Quint integration for model-based testing.

### ITFLoader

```rust
use aura_simulator::quint::itf_loader::ITFLoader;

let trace = ITFLoader::load("trace.itf.json")?;
```

The loader parses ITF traces into typed Rust structures.

### QuintMappable Trait

Types that map between Quint and Rust implement `QuintMappable`.

```rust
use aura_core::effects::quint::QuintMappable;

impl QuintMappable for ConsensusState {
    fn from_quint(value: &QuintValue) -> Result<Self> {
        // Parse Quint JSON into Rust type
    }

    fn to_quint(&self) -> QuintValue {
        // Convert Rust type to Quint JSON
    }
}
```

This trait enables bidirectional state mapping.

### ActionRegistry

The registry maps Quint action names to Rust handlers.

```rust
use aura_simulator::quint::action_registry::{ActionRegistry, ActionHandler};

let mut registry = ActionRegistry::new();
registry.register("initContext", Box::new(InitContextHandler));
registry.register("submitVote", Box::new(SubmitVoteHandler));

let result = registry.execute("initContext", &params, &effects).await?;
```

Handlers implement Quint actions using real effect handlers.

### StateMapper

The mapper converts between Aura and Quint state representations.

```rust
use aura_simulator::quint::state_mapper::StateMapper;

let mapper = StateMapper::default();
let quint_state = mapper.aura_to_quint(&aura_state)?;
let updated_aura = mapper.quint_to_aura(&quint_state)?;
```

Bidirectional mapping enables state synchronization during trace replay.

### GenerativeSimulator

The simulator replays ITF traces through real effect handlers.

```rust
use aura_simulator::quint::generative_simulator::{
    GenerativeSimulator,
    GenerativeSimConfig,
};

let config = GenerativeSimConfig {
    max_steps: 1000,
    check_invariants_every: 10,
    seed: Some(42),
};
let simulator = GenerativeSimulator::new(config)?;
let result = simulator.replay_trace(&trace).await?;
```

Replay validates that implementations match Quint specifications.

## Telltale Formal Guarantees

Telltale provides session type verification for choreographic protocols.

### Session Type Projections

Choreographies project to local session types for each participant.

```rust
#[choreography]
async fn two_party_exchange<A, B>(
    #[role] alice: A,
    #[role] bob: B,
) {
    alice.send(bob, message)?;
    let response = bob.recv(alice)?;
}
```

The macro generates session types that ensure protocol compliance.

### Leakage Tracking

The `LeakageTracker` monitors information flow during protocol execution.

```rust
use aura_mpst::LeakageTracker;

let tracker = LeakageTracker::new(budget);
tracker.record_send(recipient, message_size)?;
let remaining = tracker.remaining_budget();
```

Choreography annotations specify leakage costs. The tracker enforces budgets at runtime.

### Guard Annotations

Guards integrate with session types through annotations.

```rust
#[guard_capability("send_message")]
#[flow_cost(100)]
#[journal_facts("MessageSent")]
async fn send_step() {
    // Implementation
}
```

Annotations generate guard chain invocations. The Telltale compiler verifies annotation consistency.

## Related Documentation

- [Verification Guide](806_verification_guide.md) — Practical workflows, commands, and Quint-Lean correspondence tables
- [Verification Coverage](998_verification_coverage.md) — Current metrics, file inventories, and CI gates
- [Simulation Infrastructure Reference](118_simulator.md) — ITF trace format and generative simulation
