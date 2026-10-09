# Aura Simulator (Layer 6)

## Purpose

Deterministic simulation runtime for testing and protocol verification. Implements simulation-specific effect handlers enabling reproducible testing without real delays or inherent failures.

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

The AMP lifecycle leave action delegates membership rekeying to the agent's
simulation factory. It retains native consensus evidence and checks equal
successor keys for remaining members and key absence for departed members.
The simulator does not construct epoch certificates. Later observational
normal/emergency transition steps remain model checks rather than native
finalization or cryptoshred evidence.

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
- Simulator paths become tightly coupled to protocol-machine execution backends.
- Default simulation behavior changes when telltale parity is unused.

Verification hooks:
- just test-crate aura-simulator

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines envelope comparison policy.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines runtime conformance constraints.

### InvariantTelltaleArtifactMappingCanonical

Artifact requirements for telltale parity must stay canonical. Supported
file/control-plane report lanes must carry upstream Telltale 11 run sidecars as
their default semantic surface, while Aura differential comparison remains the
low-level surface diff rather than a second theorem authority.

Enforcement locus:
- `src/telltale_parity.rs` validates required Aura conformance surfaces before comparison.
- `src/telltale_parity.rs` requires upstream run sidecars for supported report lanes and loads them into the emitted report.

Failure mode:
- Different lanes compare non-equivalent Aura surfaces and produce false mismatches.
- Supported parity lanes silently fall back to Aura-local summaries and lose theorem-facing context.
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
| Deferred non-AMP ingress during a bounded message scan | `MoveOwned` | A local drop guard returns borrowed envelopes on success, failure and cancellation; restoration overflow fails replay. Capacity-reserving runtime leases and multi-channel routing remain required before this proves complete mailbox ownership. |

### Capability-Gated Points

- Fault injection configuration is simulator-owned and may mutate only through simulator control surfaces.
- Shared inbox/state transfer into simulator handlers is explicit and scoped to simulation harness/composer boundaries.
- Differential and parity outputs are observed artifacts and must not become a new semantic-truth source for production flows.

## Testing

### Strategy

Deterministic replay and protocol simulation fidelity are the primary concerns. Integration tests verify each simulated protocol produces correct outcomes. Property tests verify consensus and choreography invariants under fault injection. ITF trace replay verifies conformance with Quint formal models.

### Commands

The AMP lifecycle target requires the checked-in 24-step
`verification/quint/traces/amp_channel.itf.json` artifact. Missing artifacts
and missing action handlers fail replay. `just ci-amp-lifecycle-trace`
verifies deterministic regeneration before the workspace test lane. Replay
uses owned temporary storage and the default test stack; stack inflation is
not a substitute for bounded delegated futures.

Each AMP replay agent completes native threshold-service authority bootstrap
before action admission. This establishes its original physical signer,
protected genesis, active epoch and public policy. Channel bootstrap packages
cannot substitute for authority identity custody. The required full lifecycle
replay exercises genuine invitation issuance and acceptance; bootstrap and
invitation errors retain their concrete sources.

After each native join or leave, the closed fixture replicates only matching original
membership entries through journal merge/persist, retaining their exact source
keys, order and payload, together with the original producer's channel checkpoint
required for canonical AMP reduction. Missing source evidence fails replay. The post-leave
invariant checks both remaining actors' canonical membership; it cannot repair
membership from the expected set. This direct fixture delivery does not prove
production transport authorization or reactive projection synchronization.
Peers acknowledge expected participant presence or absence through canonical
reduction. The source actor alone produces the departure; peers do not create
independent replacement leave events.

The unchanged 24-step trace has two evidence scopes. Steps 1–14 exercise actual
runtime creation, invitations, original membership, delivery and departure.
Steps 15–24 use the private observational `amp_transition_model` adapter to check
phase sequencing, exact parent/context/channel binding, competing successors,
conflict suppression and emergency policy invariants. Typed transition identities
derive from the actual native base scope and observed members, but model A2/A3
statuses grant no production authority. The adapter publishes no certificate or
finalization facts, invents no signature/consensus ID, and asserts native epochs
remain unchanged. Cryptoshred policy observation proves no physical destruction.
Real verified A2 issuance and owned A3 committee integration remain outstanding.
Negative phase, foreign parent, conflict resurrection and suspect/destruction
regressions enforce this observational adapter's scope.

Channel creation commits the complete chat creation fact through the runtime
journal after AMP creation and creator join. The closed three-actor fixture
owns its immutable bootstrap roster at creation. Required checkpoint reads
and replicated leave operations propagate errors rather than treating them
as absence or proceeding with only part of the membership update.

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
| Required surfaces not enforced or upstream context omitted | TelltaleArtifactMappingCanonical | `src/telltale_parity.rs` `surface_validation_rejects_missing_required_surface`, `file_lane_embeds_upstream_telltale_sidecars` | Covered |
| Parity report artifact unstable | TelltaleArtifactMappingCanonical | `src/telltale_parity.rs` `file_lane_writes_stable_report_artifact` | Covered |
| Consensus protocol simulation wrong | — | `tests/consensus_protocol_test.rs`, `tests/consensus_property_tests.rs` | Covered |
| Invitation protocol simulation wrong | — | `tests/invitation_protocol_test.rs` | Covered |
| Recovery protocol simulation wrong | — | `tests/recovery_protocol_test.rs` | Covered |
| Guardian ceremony simulation wrong | — | `tests/guardian_ceremony_test.rs`, `tests/guardian_setup_protocol_test.rs` | Covered |
| Fault injection leaks to non-faulty paths | — | `tests/protocol_fault_injection.rs` | Covered |
| ITF trace replay diverges from Quint | — | `tests/itf_trace_replay.rs` | Covered |
| Liveness under partitions fails | — | `tests/liveness_under_partitions.rs` | Covered |
| AMP transition conflict/emergency semantics drift from the reducer/model | — | `tests/amp_transition_scenarios.rs` | Covered |
| Guard interpreter not deterministic | DeterministicReplay | `src/effects/guard_interpreter.rs` `test_deterministic_nonce_generation` | Covered |
| Property monitor misses invariant violation | — | `tests/fault_invariant_monitor.rs` | Covered |

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Simulator](../../docs/119_simulator.md)
- [Formal Verification Reference](../../docs/120_verification.md)
- [Verification Coverage Report](../../docs/998_verification_coverage.md)
