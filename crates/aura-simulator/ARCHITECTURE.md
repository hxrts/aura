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

