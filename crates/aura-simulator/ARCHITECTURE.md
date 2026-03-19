# Aura Simulator (Layer 6) - Architecture and Invariants

## Purpose
Deterministic simulation runtime for testing and protocol verification. Implements
simulation-specific effect handlers enabling reproducible testing without real
delays or inherent failures.

## Inputs
- Lower layers (Layers 1-5) for protocol/domain logic.
- `SimulatorConfig` with simulation parameters and seeds.
- Fault injection strategies (`ByzantineStrategy`, `ChaosStrategy`).
- Scenario definitions and triggers.

## Outputs
- `SimulationTimeHandler`, `SimulationFaultHandler`, `SimulationScenarioHandler`.
- `SimulationEffectComposer`, `ComposedSimulationEnvironment`.
- `SimulatorConfig`, `SimulatorContext`, `SimulationOutcome`.
- `TestkitSimulatorBridge` for aura-testkit integration.
- Quint integration for formal verification specs.

## Invariants
- Deterministic execution: Same seed produces identical execution paths.
- No real delays: Simulated time advances without actual delays.
- Effect-based only: All simulation via effect system (no globals).
- Must NOT create persistent effect handlers (use aura-effects).
- Must NOT implement multi-party coordination (use aura-protocol).

## Ownership Model

`aura-simulator` is primarily an `ActorOwned` plus `Observed` crate.

- `ActorOwned`
  - simulation runtime coordination
  - deterministic event scheduling
  - fault-injection orchestration
- `Observed`
  - artifact comparison
  - parity reporting
  - replay diagnostics
- not product-semantic `MoveOwned`
  - simulation may model transfer surfaces, but it must not invent alternate
    ownership contracts for production parity-critical flows
- not a semantic-truth source for frontend/runtime ownership
  - simulator reports and comparisons observe product contracts rather than
    redefining them

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Simulation scheduler, clocks, and runtime coordination | `ActorOwned` | simulator runtime / scheduler owner | simulator orchestration code | tests, reports, diagnostics |
| Fault injection and deterministic environment state | `ActorOwned` | owning simulator service/task | simulator control paths | reports, bridges |
| Differential/parity artifact comparison | `Observed` | upstream artifacts and comparison contracts | local comparison state only | verification outputs, diagnostics |
| Quint / external verification bridge inputs | `Observed` | external artifact/schema producers | bridge adaptation only | simulator comparison/reporting |

### Capability-Gated Points

- Fault injection configuration is simulator-owned and may mutate only through
  simulator control surfaces.
- Shared inbox/state transfer into simulator handlers is explicit and scoped to
  simulation harness/composer boundaries.
- Differential and parity outputs are observed artifacts and must not become a
  new semantic-truth source for production flows.

### Verification Hooks

- `cargo check -p aura-simulator`
- `cargo test -p aura-simulator --lib`
- `just test-crate aura-simulator`

### Detailed Specifications

### InvariantSimulationDeterministicReplay
Given the same seed and inputs, simulation execution paths and outcomes must be deterministic.

Enforcement locus:
- src simulator control paths derive behavior from explicit deterministic inputs.
- No direct runtime globals are used for simulation state progression.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-simulator

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines deterministic interpretation constraints.
- [Simulator](../../docs/119_simulator.md) defines replay and determinism expectations.
## Testing

### Strategy

Deterministic replay and protocol simulation fidelity are the primary
concerns. Integration tests verify each simulated protocol produces
correct outcomes. Property tests verify consensus and choreography
invariants under fault injection. ITF trace replay verifies conformance
with Quint formal models.

### Running tests

```
cargo test -p aura-simulator
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Same seed produces different execution | DeterministicReplay | `src/scenarios/` `simulation_time_handler_deterministic_start` | Covered |
| Replay transcript mismatch | DeterministicReplay | `src/async_host.rs` `async_host_replay_matches_recorded_transcript` | Covered |
| Replay mismatch not detected | DeterministicReplay | `src/async_host.rs` `async_host_replay_detects_mismatch` | Covered |
| Parity diverges between sync/async hosts | TelltaleParityBoundaryStable | `src/async_host.rs` `async_host_parity_matches_sync_host_on_representative_suite` | Covered |
| Surface mapping doesn't match required surfaces | TelltaleArtifactMappingCanonical | `src/telltale_parity.rs` `canonical_surface_mapping_matches_required_surfaces` | Covered |
| Parity report artifact unstable | TelltaleArtifactMappingCanonical | `src/telltale_parity.rs` `file_lane_writes_stable_report_artifact` | Covered |
| Consensus protocol simulation wrong | — | `tests/consensus_protocol_test.rs`, `tests/consensus_property_tests.rs` | Covered |
| Invitation protocol simulation wrong | — | `tests/invitation_protocol_test.rs` | Covered |
| Recovery protocol simulation wrong | — | `tests/recovery_protocol_test.rs` | Covered |
| Guardian ceremony simulation wrong | — | `tests/guardian_ceremony_test.rs`, `tests/guardian_setup_protocol_test.rs` | Covered |
| Fault injection leaks to non-faulty paths | — | `tests/protocol_fault_injection.rs` | Covered |
| ITF trace replay diverges from Quint | — | `tests/itf_trace_replay.rs` | Covered |
| Liveness under partitions fails | — | `tests/liveness_under_partitions.rs` | Covered |
| Guard interpreter not deterministic | DeterministicReplay | `src/effects/guard_interpreter.rs` `test_deterministic_nonce_generation` | Covered |
| Property monitor misses invariant violation | — | `tests/fault_invariant_monitor.rs` | Covered |

## Boundaries
- Must NOT be imported by Layers 1-5.
- Composable fault injection combines with production effects.
- Quint specs live in verification/quint/.

## Telltale VM Parity Boundary
- `aura-simulator` exposes a boundary entry point through `TelltaleParityRunner`.
- The boundary takes normalized conformance artifacts as input.
- The boundary does not execute telltale VM directly in default simulator paths.
- Runtime telltale execution remains owned by external lanes and adapters.

### InvariantTelltaleParityBoundaryStable
Telltale parity integration must remain artifact-driven and profile-selectable.

Enforcement locus:
- `src/telltale_parity.rs` defines boundary input and runner trait.
- `src/differential_tester.rs` evaluates strict and envelope-bounded profiles.

Failure mode:
- Simulator paths become tightly coupled to VM execution backends.
- Default simulation behavior changes when telltale parity is unused.

Verification hooks:
- `just test-crate aura-simulator`

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines envelope comparison policy.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines runtime conformance constraints.

### Canonical Artifact Mapping
- `observable` telltale events map to Aura `observable` surface with identity normalization.
- `scheduler_step` telltale events map to Aura `scheduler_step` surface with tick normalization.
- `effect` telltale events map to Aura `effect` surface with envelope-class normalization.
- Mapping schema id: `aura.telltale-parity.report.v1`.

### Bridge Ownership
- `aura-simulator` owns artifact-level parity entry points and differential comparison invocation.
- `aura-simulator` consumes bridge schema outputs but does not redefine schema structures.
- `aura-simulator` does not own runtime VM admission logic.

### InvariantTelltaleArtifactMappingCanonical
Artifact mapping for telltale parity must be stable and shared across lanes.

Enforcement locus:
- `src/telltale_parity.rs` publishes `TELLTALE_SURFACE_MAPPINGS_V1`.
- `src/telltale_parity.rs` validates required Aura conformance surfaces before comparison.

Failure mode:
- Different lanes compare non-equivalent surfaces and produce false mismatches.
- Parity reports cannot be replayed or audited consistently.

Verification hooks:
- `just test-crate aura-simulator`

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines required surfaces and envelope classes.
- [Verification Coverage Report](../../docs/998_verification_coverage.md) tracks parity coverage lanes and schema references.
