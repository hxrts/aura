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
- [Simulator](../../docs/118_simulator.md) defines replay and determinism expectations.
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
- [Conformance and Parity Reference](../../docs/119_conformance.md) defines envelope comparison policy.
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
- [Conformance and Parity Reference](../../docs/119_conformance.md) defines required surfaces and envelope classes.
- [Verification Coverage Report](../../docs/998_verification_coverage.md) tracks parity coverage lanes and schema references.
