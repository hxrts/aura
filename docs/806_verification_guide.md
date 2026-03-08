# Verification and MBT Guide

This guide covers how to use formal verification and model-based testing to validate Aura protocols. It focuses on practical workflows with Quint, Lean, and generative testing.

Quint, simulator, and harness have distinct responsibilities. Quint defines models, traces, and invariants. The `aura-simulator` crate is a selectable deterministic runtime substrate. The `aura-harness` crate is the single executor for real TUI and web frontend flows. Shared semantic UI and scenario contracts live in `aura-app`.

`aura-app` is also the home of the shared-flow support and parity contract used by the real-runtime harness. This includes `SharedFlowId`, `SHARED_FLOW_SUPPORT`, `SHARED_FLOW_SCENARIO_COVERAGE`, `UiSnapshot`, semantic parity comparison helpers, and typed runtime event diagnostics.

## When to Verify

Verification suits protocols with complex state machines or security-critical properties. Use Quint model checking for exhaustive state exploration. Use Lean proofs for mathematical guarantees. Use generative testing to validate implementations against models.

Unit tests suffice for simple, well-understood behavior. Do not over-invest in verification for straightforward code.

See [Formal Verification Reference](119_verification.md) for the complete architecture documentation.

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

Harnesses provide standardized entry points for tooling. They should emit
semantic traces and invariants, not frontend-specific scripts or key sequences.

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

### Generating Semantic Traces

```bash
just quint-semantic-trace spec=verification/quint/harness/flows.qnt \
  out=verification/quint/traces/harness_flows.itf.json
```

ITF traces capture semantic state sequences and non-deterministic choices.
These traces are model artifacts. Real TUI and web execution belongs to the
harness, which consumes the shared semantic scenario contract. Shared web/TUI parity assertions also run against the same `UiSnapshot` contract rather than renderer text or DOM structure.

Do not add direct Quint-to-TUI or Quint-to-browser execution paths. Quint should
hand off semantic traces to the shared contract layer, then let the harness or
simulator consume them through their own adapters.

For shared end-to-end flows, the harness contract is semantic and state-based. Do not introduce frontend-specific Quint replay formats that encode raw keypress sequences, browser selectors, or label-based button targeting. Those belong in driver adapters and diagnostics, not in the semantic trace contract.

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
quint run --out-itf=artifacts/traces/new_trace.itf.json verification/quint/updated_spec.qnt
just ci-conformance
```

See [Testing Guide](804_testing_guide.md) for corpus policy details.

The main repository policy gate for shared-flow drift is:

```bash
just ci-shared-flow-policy
```

This complements Quint and simulator checks by enforcing that shared real-runtime
scenarios still use the semantic contract and that the shared-flow support map in
`aura-app` remains consistent with the harness/frontend surfaces.

## Telltale Verification Workflow in Aura

Use this workflow for choreography and simulator-level verification that depends on Telltale-derived checks.

### 1) Choreography compatibility gate (CI/tooling)

Run:

```bash
nix develop --command scripts/check/protocol-compat.sh --self-test
nix develop --command just ci-protocol-compat
```

This validates that known-compatible fixture pairs pass async subtyping checks. It confirms known-breaking fixture pairs fail as expected. It ensures changed `.choreo` files stay backward-compatible unless intentionally breaking.

Fixtures live in `crates/aura-testkit/fixtures/protocol_compat/`.

### 2) Macro-time coherence gate

Run:

```bash
nix develop --command cargo test -p aura-macros
```

This enforces compile-time coherence validation for choreographies, including negative compile-fail coverage.

### 3) Simulator invariant monitoring under injected faults

Run:

```bash
nix develop --command cargo test -p aura-simulator --test fault_invariant_monitor
```

This verifies that injected faults produce monitor-visible invariant violations (for example, `NoFaults` violations), and that a gate configured to require zero violations fails accordingly.

### 4) `telltale-lean-bridge` integration status

As of March 5, 2026, Aura includes `telltale-lean-bridge` as a workspace dependency and exposes it through `aura-quint`.

This adds direct access to upstream Lean runner and equivalence utilities from the Telltale project. It provides explicit schema and version linkage with upstream bridge contracts for cross-tool consistency. It also creates a cleaner path for future migration of local bridge helpers to upstream bridge APIs.

The `aura-quint` crate re-exports the upstream crate as `upstream_telltale_lean_bridge`. Call `aura_quint::upstream_telltale_lean_bridge_schema_version()` to get the upstream schema version. CI lanes remain `just ci-lean-quint-bridge` and `just ci-simulator-telltale-parity`.

### 5) `aura-testkit` Lean verification API migration (March 5, 2026)

As of March 5, 2026, legacy Lean verification compatibility types were removed from
`aura_testkit::verification` and `aura_testkit::verification::lean_oracle`.

Use the canonical full-fidelity types and methods.

| Legacy Type | Canonical Type |
|-------------|----------------|
| `Fact` | `LeanFact` |
| `ComparePolicy` | `LeanComparePolicy` |
| `TimeStamp` | `LeanCompareTimeStamp` (compare payloads) or `LeanTimeStamp` (journal facts) |
| `Ordering` | `LeanTimestampOrdering` |
| `FlowChargeInput`/`FlowChargeResult` | `LeanFlowChargeInput`/`LeanFlowChargeResult` |
| `TimestampCompareInput`/`TimestampCompareResult` | `LeanTimestampCompareInput`/`LeanTimestampCompareResult` |

| Legacy Method | Canonical Method |
|---------------|------------------|
| `verify_merge` | `verify_journal_merge` |
| `verify_reduce` | `verify_journal_reduce` |
| `verify_charge` | `verify_flow_charge` |
| `verify_compare` | `verify_timestamp_compare` |

Import Lean verification payload types from `aura_testkit::verification` which re-exports from `lean_types`. Construct structured journals with `LeanJournal` and `LeanNamespace`. Update tests to compare `LeanTimestampOrdering` values directly.

## Lean-Quint Bridge

The bridge connects Quint model checking with Telltale and Lean proof artifacts. It enables exporting Quint session models to a stable interchange format, importing Telltale and Lean properties back into Quint harnesses, and running cross-validation to detect divergence early in CI.

### Operator Workflow

Run the bridge lane:

```bash
just ci-lean-quint-bridge
```

Inspect outputs at `artifacts/lean-quint-bridge/bridge.log`, `artifacts/lean-quint-bridge/bridge_discrepancy_report.json`, and `artifacts/lean-quint-bridge/report.json`.

Run the simulator telltale parity lane:

```bash
just ci-simulator-telltale-parity
```

Inspect output at `artifacts/telltale-parity/report.json`.

### Data Contract

`aura-quint` defines a versioned interchange schema for bridge workflows.

| Type | Purpose |
|------|---------|
| `BridgeBundleV1` | Top-level bundle with `schema_version = "aura.lean-quint-bridge.v1"` |
| `SessionTypeInterchangeV1` | Session graph exchange |
| `PropertyInterchangeV1` | Quint, Telltale, and Lean property exchange |
| `ProofCertificateV1` | Proof or model-check evidence |

Use this schema as the canonical data contract when exporting Quint sessions to Telltale formats or importing Telltale and Lean properties into Quint harnesses.

### Export Workflow

Export moves session models from Quint to Telltale format.

1. Parse Quint JSON IR with `parse_quint_modules(...)`
2. Build the bridge bundle with `export_quint_to_telltale_bundle(...)`
3. Validate structural correctness with `validate_export_bundle(...)`

### Import Workflow

Import brings Telltale and Lean properties back into Quint harnesses.

1. Select importable properties with `parse_telltale_properties(...)`
2. Generate Quint invariant module text with `generate_quint_invariant_module(...)`
3. Map certificates into Quint assertion comments with `map_certificates_to_quint_assertions(...)`

### Cross-Validation Workflow

Cross-validation detects proof and model divergence. Use `run_cross_validation(...)` from `aura-quint` to execute Quint checks through a `QuintModelCheckExecutor`, compare outcomes to bridge proof certificates, and emit a `CrossValidationReport` with explicit discrepancy entries.

Run cross-validation in CI:

```bash
just ci-lean-quint-bridge
```

This command produces artifacts under `artifacts/lean-quint-bridge/` including `bridge.log` and `report.json`.

### Handling Discrepancies

When cross-validation reports discrepancies, follow these steps. First confirm the property identity mapping (`property_id`) between model and proof pipelines. Then re-run the failing property in Quint and capture the trace or counterexample. Next re-check proof certificate assumptions against the current protocol model. Do not merge until the mismatch is resolved or explicitly justified.

For telltale parity mismatches, read `comparison_classification`, `first_mismatch_surface`, and `first_mismatch_step_index` first. Re-run the failing lane with the same scenario and seed. Confirm that required surfaces (`observable`, `scheduler_step`, `effect`) were captured before examining envelope differences.

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
just verify-lean       # Build proofs
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

## Adding or Updating Invariants

When adding or modifying invariants, follow this workflow to maintain traceability across docs, tests, and proofs.

1. Add or update the invariant under `## Invariants` in the crate's `ARCHITECTURE.md`.
2. Add a detailed specification section in the same file with invariant name, enforcement locus, failure mode, and verification hooks.
3. Use canonical `InvariantXxx` naming for traceability across docs, tests, and proofs.
4. Add or update tests and simulator scenarios that detect violations.
5. Update the traceability matrix in [Project Structure](999_project_structure.md#traceability-matrix) if the invariant is cross-crate or contract-level.

Formal and model checks should reference the same canonical names listed in the traceability matrix.

## Quint-Lean Correspondence

This section maps Quint model invariants to Lean theorem proofs, providing traceability between model checking and formal proofs.

### Types Correspondence

| Quint Type | Lean Type | Rust Type |
|------------|-----------|-----------|
| `ConsensusId` | `Aura.Domain.Consensus.Types.ConsensusId` | `consensus::types::ConsensusId` |
| `ResultId` | `Aura.Domain.Consensus.Types.ResultId` | `consensus::types::ResultId` |
| `PrestateHash` | `Aura.Domain.Consensus.Types.PrestateHash` | `consensus::types::PrestateHash` |
| `AuthorityId` | `Aura.Domain.Consensus.Types.AuthorityId` | `core::AuthorityId` |
| `ShareData` | `Aura.Domain.Consensus.Types.ShareData` | `consensus::types::SignatureShare` |
| `ThresholdSignature` | `Aura.Domain.Consensus.Types.ThresholdSignature` | `consensus::types::ThresholdSignature` |
| `CommitFact` | `Aura.Domain.Consensus.Types.CommitFact` | `consensus::types::CommitFact` |
| `WitnessVote` | `Aura.Domain.Consensus.Types.WitnessVote` | `consensus::types::WitnessVote` |
| `Evidence` | `Aura.Domain.Consensus.Types.Evidence` | `consensus::types::Evidence` |

### Invariant-Theorem Correspondence

#### Agreement Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantUniqueCommitPerInstance` | `Aura.Proofs.Consensus.Agreement.agreement` | proven |
| `InvariantUniqueCommitPerInstance` | `Aura.Proofs.Consensus.Agreement.unique_commit` | proven |
| - | `Aura.Proofs.Consensus.Agreement.commit_determinism` | proven |

#### Validity Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantCommitRequiresThreshold` | `Aura.Proofs.Consensus.Validity.commit_has_threshold` | proven |
| `InvariantSignatureBindsToCommitFact` | `Aura.Proofs.Consensus.Validity.validity` | proven |
| - | `Aura.Proofs.Consensus.Validity.distinct_signers` | proven |
| - | `Aura.Proofs.Consensus.Validity.prestate_binding_unique` | proven |
| - | `Aura.Proofs.Consensus.Validity.honest_participation` | proven |
| - | `Aura.Proofs.Consensus.Validity.threshold_unforgeability` | axiom |

#### FROST Integration Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantSignatureThreshold` | `Aura.Proofs.Consensus.Frost.aggregation_threshold` | proven |
| - | `Aura.Proofs.Consensus.Frost.share_session_consistency` | proven |
| - | `Aura.Proofs.Consensus.Frost.share_result_consistency` | proven |
| - | `Aura.Proofs.Consensus.Frost.distinct_signers` | proven |
| - | `Aura.Proofs.Consensus.Frost.share_binding` | proven |

#### Evidence CRDT Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| - | `Aura.Proofs.Consensus.Evidence.merge_comm_votes` | proven |
| - | `Aura.Proofs.Consensus.Evidence.merge_assoc_votes` | proven |
| - | `Aura.Proofs.Consensus.Evidence.merge_idem` | proven |
| - | `Aura.Proofs.Consensus.Evidence.merge_preserves_commit` | proven |
| - | `Aura.Proofs.Consensus.Evidence.commit_monotonic` | proven |

#### Equivocation Detection Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantEquivocationDetected` | `Aura.Proofs.Consensus.Equivocation.detection_soundness` | proven |
| `InvariantEquivocationDetected` | `Aura.Proofs.Consensus.Equivocation.detection_completeness` | proven |
| `InvariantEquivocatorsExcluded` | `Aura.Proofs.Consensus.Equivocation.exclusion_correctness` | proven |
| `InvariantHonestMajorityCanCommit` | `Aura.Proofs.Consensus.Equivocation.honest_never_detected` | proven |
| - | `Aura.Proofs.Consensus.Equivocation.verified_proof_sound` | proven |

#### Byzantine Tolerance (Adversary Module)

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantByzantineThreshold` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.byzantine_cannot_forge` | claim |
| `InvariantEquivocationDetected` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.equivocation_detectable` | claim |
| `InvariantHonestMajorityCanCommit` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.honest_majority_sufficient` | claim |
| `InvariantEquivocatorsExcluded` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.equivocators_excluded` | claim |
| `InvariantCompromisedNoncesExcluded` | - | Quint only |

#### Liveness Properties

| Quint Property | Lean Support | Notes |
|----------------|--------------|-------|
| `InvariantProgressUnderSynchrony` | `Aura.Proofs.Consensus.Liveness.livenessClaims.terminationUnderSynchrony` | axiom |
| `InvariantByzantineTolerance` | `byzantine_threshold` | axiom |
| `FastPathProgressCheck` | `Aura.Proofs.Consensus.Liveness.livenessClaims.fastPathBound` | axiom |
| `SlowPathProgressCheck` | `Aura.Proofs.Consensus.Liveness.livenessClaims.fallbackBound` | axiom |
| `NoDeadlock` | `Aura.Proofs.Consensus.Liveness.livenessClaims.noDeadlock` | axiom |
| `InvariantRetryBound` | - | Quint model checking only |

#### Module Correspondence

| Lean Module | Quint File | What It Proves |
|-------------|------------|----------------|
| `Proofs.ContextIsolation` | `authorization.qnt`, `leakage.qnt` | Context separation and bridge authorization |
| `Proofs.Consensus.Agreement` | `consensus/core.qnt` | Agreement safety (unique commits) |
| `Proofs.Consensus.Evidence` | `consensus/core.qnt` | CRDT semilattice properties |
| `Proofs.Consensus.Frost` | `consensus/frost.qnt` | Threshold signature correctness |
| `Proofs.Consensus.Liveness` | `consensus/liveness.qnt` | Synchrony model axioms |
| `Proofs.Consensus.Adversary` | `consensus/adversary.qnt` | Byzantine tolerance bounds |
| `Proofs.Consensus.Equivocation` | `consensus/adversary.qnt` | Detection soundness/completeness |

## Related Documentation

See [Formal Verification Reference](119_verification.md) for architecture details. See [Simulation Guide](805_simulation_guide.md) for trace replay. See [Testing Guide](804_testing_guide.md) for conformance testing. See [Project Structure](999_project_structure.md#invariant-traceability) for the invariant index and traceability matrix.
