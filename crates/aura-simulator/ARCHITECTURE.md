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

## Boundaries
- Must NOT be imported by Layers 1-5.
- Composable fault injection combines with production effects.
- Quint specs live in verification/quint/.
