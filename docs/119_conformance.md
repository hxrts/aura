# Conformance and Parity Reference

This document specifies the conformance testing infrastructure that enforces deterministic behavior across native and WASM targets.

## Overview

Conformance testing validates that protocol implementations produce identical results across execution environments. The primary focus is native/WASM parity. All protocol transitions must be deterministic given the same inputs.

The system enforces three principles. Effect boundaries isolate non-determinism. Artifact formats enable cross-platform comparison. CI lanes catch divergence before merge.

## Determinism Rules

### Pure Transition Core

Given the same input stream, protocol transitions must produce identical outputs. No hidden state may affect observable behavior. All state must flow through explicit effect calls.

### Effect Boundary Discipline

Non-determinism is permitted only through explicit algebraic effects. Time comes from `PhysicalTimeEffects`. Randomness comes from `RandomEffects`. Storage comes from `StorageEffects`. No direct system calls are allowed.

### No Wall-Clock Dependence

Strict conformance lanes compare logical steps, not wall-clock timing. Tests must not depend on execution speed. Time-dependent behavior uses simulated time through effect handlers.

### Stable Serialization

Conformance artifacts use canonical encoding. Binary formats use deterministic field ordering. JSON uses sorted keys. No floating-point values appear in serialized state.

### No Undeclared Divergence

Any difference outside declared commutative or algebraic envelopes is a failure. New effect kinds must be explicitly classified before use.

## Artifact Format

### AuraConformanceArtifactV1

The conformance artifact captures execution state for comparison.

```rust
use aura_testkit::conformance::AuraConformanceArtifactV1;

let artifact = AuraConformanceArtifactV1 {
    observable: observable_outputs,
    scheduler_step: scheduler_state,
    effect: effect_trace,
    metadata: execution_metadata,
};
artifact.validate()?;
```

Validation fails if any required surface is missing or malformed.

### Required Surfaces

Every conformance artifact must capture three surfaces.

| Surface | Purpose | Content |
|---------|---------|---------|
| `observable` | Protocol-visible outputs | Normalized message contents |
| `scheduler_step` | Logical progression | Step index, session state, role progression |
| `effect` | Effect envelope trace | Sequence of effect calls with arguments |

Missing surfaces cause validation failure. This ensures complete execution capture.

### Metadata

Artifacts include metadata for debugging and correlation.

```rust
pub struct ConformanceMetadata {
    pub scenario_name: String,
    pub seed: u64,
    pub platform: Platform,
    pub timestamp: u64,
    pub version: String,
}
```

Metadata does not affect comparison. It aids investigation of failures.

## Effect Envelope Classification

Each effect kind has a comparison class that determines how differences are evaluated.

### Classification Table

| Effect Kind | Class | Comparison Rule |
|-------------|-------|-----------------|
| `send_decision` | `commutative` | Order-insensitive under normalization |
| `invoke_step` | `commutative` | Scheduler interleavings normalized |
| `handle_recv` | `strict` | Byte-exact match required |
| `handle_choose` | `strict` | Branch choice must match |
| `handle_acquire` | `strict` | Guard semantics must match |
| `handle_release` | `strict` | Guard semantics must match |
| `topology_event` | `algebraic` | Reduced via topology-normal form |

### Comparison Classes

The `strict` class requires exact matches. Any difference is a failure.

The `commutative` class normalizes order before comparison. Multiple equivalent orderings are acceptable.

The `algebraic` class applies domain-specific reduction before comparison. Equivalent states may have different representations.

### Adding New Effect Kinds

New effect kinds must be classified before use.

```rust
use aura_core::conformance::AURA_EFFECT_ENVELOPE_CLASSIFICATIONS;

// Add to the classification map
AURA_EFFECT_ENVELOPE_CLASSIFICATIONS.insert(
    "new_effect_kind",
    ComparisonClass::Strict,
);

// Verify classification exists
aura_core::assert_effect_kinds_classified(&effect_trace)?;
```

Unclassified effect kinds cause conformance checks to fail.

## Conformance Lanes

CI runs two conformance lanes with different comparison strategies.

### Strict Lane

The strict lane compares native cooperative and WASM cooperative execution.

```bash
just ci-conformance-strict
```

This lane uses the `native_coop` and `wasm_coop` executors. Both use cooperative scheduling. Differences indicate platform-specific behavior.

### Differential Lane

The differential lane compares native threaded and native cooperative execution.

```bash
just ci-conformance-diff
```

This lane detects scheduling-dependent behavior. It also runs model-level differential test suites.

### CI Integration

Protected branches require conformance passage.

```bash
just ci-conformance
```

This command runs both lanes. Any undeclared divergence blocks merge.

## Corpus Policy

The conformance corpus defines test inputs for parity checking.

### Fixed Seeds

Fixed seeds provide stable regression coverage.

```bash
AURA_CONFORMANCE_SEED=42 cargo test conformance
```

The `AURA_CONFORMANCE_SEED` environment variable selects a specific seed for reproduction.

### Rotating Seed Window

Rotating seeds provide broader coverage over time.

```bash
AURA_CONFORMANCE_ROTATING_WINDOW=100 cargo test conformance
```

CI uses a window of recent seeds. The window advances with each run.

### ITF-Derived Seeds

Seeds can derive from Quint ITF traces.

```bash
AURA_CONFORMANCE_ITF_TRACE=trace.itf.json cargo test conformance
```

This couples conformance coverage to formal model coverage.

### Mutation Corpus

The mutation corpus contains inputs with expected divergence categories.

| Category | Description | Handling |
|----------|-------------|----------|
| `observable` | Expected output differences | Declared, not blocking |
| `strict` | Expected strict mismatches | Declared, not blocking |
| `unclassified` | Unexpected differences | Blocking |

Declared divergences document known platform differences. Undeclared divergences indicate bugs.

## ITF Trace Format

ITF (Informal Trace Format) traces come from Quint model checking.

### Structure

```json
{
  "#meta": {
    "format": "ITF",
    "source": "quint",
    "version": "1.0"
  },
  "vars": ["phase", "participants", "messages"],
  "states": [
    {
      "#meta": { "index": 0 },
      "phase": "Setup",
      "participants": [],
      "messages": []
    },
    {
      "#meta": { "index": 1, "action": "addParticipant" },
      "phase": "Setup",
      "participants": ["alice"],
      "messages": []
    }
  ]
}
```

Each state represents a model state. Transitions between states correspond to actions.

### Non-Deterministic Picks

ITF traces capture non-deterministic choices for replay.

```json
{
  "#meta": {
    "index": 3,
    "action": "selectLeader",
    "nondet_picks": { "leader": "bob" }
  }
}
```

The `nondet_picks` field records choices made by Quint. Replay uses these values to seed `RandomEffects`.

### Loading Traces

```rust
use aura_simulator::quint::itf_loader::ITFLoader;

let trace = ITFLoader::load("trace.itf.json")?;
for (i, state) in trace.states.iter().enumerate() {
    let action = state.meta.action.as_deref();
    let picks = &state.meta.nondet_picks;
}
```

The loader validates trace format and extracts typed state.

## Troubleshooting

### Unclassified Envelope Kind

Symptom: `unclassified effect_kind` error

Fix: Add the kind to `AURA_EFFECT_ENVELOPE_CLASSIFICATIONS` with appropriate class.

### Missing Required Surface

Symptom: Artifact validation fails for `observable`, `scheduler_step`, or `effect`

Fix: Ensure runtime captures all three surfaces before computing digest.

### Native/Threaded Mismatch

Symptom: First mismatch in `scheduler_step` or strict `effect`

Fix: Remove ordering-sensitive hidden state. Normalize only declared commutative or algebraic envelopes.

### WASM Runner Schema Mismatch

Symptom: `wasm-bindgen` schema version error

Fix: Align `wasm-bindgen-test-runner` version with workspace `wasm-bindgen` version.

### Reproducing Failures

Native reproduction:

```bash
AURA_CONFORMANCE_SCENARIO=scenario_name \
AURA_CONFORMANCE_SEED=42 \
cargo test -p aura-agent \
  --features choreo-backend-telltale-vm \
  --test telltale_vm_parity test_name \
  -- --nocapture
```

WASM reproduction:

```bash
AURA_CONFORMANCE_SCENARIO=scenario_name \
AURA_CONFORMANCE_SEED=42 \
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
cargo test -p aura-agent \
  --target wasm32-unknown-unknown \
  --features web,choreo-backend-telltale-vm \
  --test telltale_vm_parity test_name \
  -- --nocapture
```

## CI Artifacts

Conformance artifacts upload to CI for failure triage.

```
artifacts/conformance/
├── native_coop/
│   └── scenario_seed_artifact.json
├── wasm_coop/
│   └── scenario_seed_artifact.json
└── diff_report.json
```

The diff report highlights specific mismatches for investigation.

## Related Documentation

See [Testing Guide](805_testing_guide.md) for writing conformance tests. See [Simulation Infrastructure Reference](118_simulator.md) for ITF trace replay. See [Formal Verification Reference](120_verification.md) for Quint integration.
