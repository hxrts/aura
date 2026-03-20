# Aura Simulator (Layer 6)

## Purpose

Deterministic simulation runtime for testing and protocol verification. Implements
simulation-specific effect handlers enabling reproducible testing without real
delays or inherent failures.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Simulation-specific effect handlers | Persistent effect handlers (aura-effects) |
| Deterministic time, fault injection, and scheduling | Multi-party coordination (aura-protocol) |
| Scenario definitions and triggers | Layers 1-5 imports of this crate |
| Quint integration for formal verification | |
| Telltale parity boundary and differential testing | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | Layers 1-5 | Protocol/domain logic |
| Incoming | — | `SimulatorConfig` with simulation parameters and seeds |
| Incoming | — | Fault injection strategies (`ByzantineStrategy`, `ChaosStrategy`) |
| Outgoing | — | `SimulationTimeHandler`, `SimulationFaultHandler`, `SimulationScenarioHandler` |
| Outgoing | — | `SimulationEffectComposer`, `ComposedSimulationEnvironment` |
| Outgoing | — | `SimulatorConfig`, `SimulatorContext`, `SimulationOutcome` |
| Outgoing | — | `TestkitSimulatorBridge` for aura-testkit integration |

## Invariants

- Deterministic execution: Same seed produces identical execution paths.
- No real delays: Simulated time advances without actual delays.
- Effect-based only: All simulation via effect system (no globals).
- Must NOT create persistent effect handlers (use aura-effects).
- Must NOT implement multi-party coordination (use aura-protocol).

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

### InvariantTelltaleParityBoundaryStable

Telltale parity integration must remain artifact-driven and profile-selectable.

Enforcement locus:
- `src/telltale_parity.rs` defines boundary input and runner trait.
- `src/differential_tester.rs` evaluates strict and envelope-bounded profiles.

Failure mode:
- Simulator paths become tightly coupled to VM execution backends.
- Default simulation behavior changes when telltale parity is unused.

Verification hooks:
- just test-crate aura-simulator

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines envelope comparison policy.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines runtime conformance constraints.

### InvariantTelltaleArtifactMappingCanonical

Artifact mapping for telltale parity must be stable and shared across lanes.

Enforcement locus:
- `src/telltale_parity.rs` publishes `TELLTALE_SURFACE_MAPPINGS_V1`.
- `src/telltale_parity.rs` validates required Aura conformance surfaces before comparison.

Failure mode:
- Different lanes compare non-equivalent surfaces and produce false mismatches.
- Parity reports cannot be replayed or audited consistently.

Verification hooks:
- just test-crate aura-simulator

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines required surfaces and envelope classes.
- [Verification Coverage Report](../../docs/998_verification_coverage.md) tracks parity coverage lanes and schema references.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-simulator` is primarily an `ActorOwned` plus `Observed` crate.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| Simulation scheduler, clocks, and runtime coordination | `ActorOwned` | Simulator orchestration code owns mutation; tests/reports/diagnostics observe. |
| Fault injection and deterministic environment state | `ActorOwned` | Owning simulator service/task controls mutation; reports/bridges observe. |
| Differential/parity artifact comparison | `Observed` | Upstream artifacts and comparison contracts are authoritative; local comparison state only. |
| Quint / external verification bridge inputs | `Observed` | External artifact/schema producers are authoritative; bridge adaptation only. |

### Capability-Gated Points

- Fault injection configuration is simulator-owned and may mutate only through
  simulator control surfaces.
- Shared inbox/state transfer into simulator handlers is explicit and scoped to
  simulation harness/composer boundaries.
- Differential and parity outputs are observed artifacts and must not become a
  new semantic-truth source for production flows.

## Testing

### Strategy

Deterministic replay and protocol simulation fidelity are the primary
concerns. Integration tests verify each simulated protocol produces
correct outcomes. Property tests verify consensus and choreography
invariants under fault injection. ITF trace replay verifies conformance
with Quint formal models.

### Commands

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

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Simulator](../../docs/119_simulator.md)
- [Formal Verification Reference](../../docs/120_verification.md)
- [Verification Coverage Report](../../docs/998_verification_coverage.md)
