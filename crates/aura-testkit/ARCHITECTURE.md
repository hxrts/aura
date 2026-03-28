# Aura Testkit (Layer 8)

## Purpose

Shared testing infrastructure providing test fixtures, effect system harnesses, mock implementations, time control, and simulation support for deterministic testing across all layers.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Test fixtures and builders (accounts, devices, authorities) | Layer 1-3 imports of this crate (circular dependency) |
| Mock effect implementations | Production handlers (aura-effects) |
| Effect capture and assertion utilities | Simulation runtime (aura-simulator) |
| Deterministic time control and scheduling | |
| Privacy analysis and information flow verification tools | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | Layers 1-7 | Core, domains, effects, protocols, features, runtime |
| Outgoing | — | `MockCryptoHandler`, `MockTimeHandler`, `InMemoryStorageHandler` |
| Outgoing | — | Test fixtures, builders, effect capture and assertion utilities |
| Outgoing | — | Deterministic time control and scheduling |

## Invariants

- Must NOT be imported by Layer 1-3 crates (would create circular dependencies).
- May be imported by Layer 4-7 in `[dev-dependencies]` only.
- Tests are deterministic and isolated.
- Mock effects behave consistently with production ones.
- Mock handlers MAY be stateful (using `Arc<Mutex<>>`) for controllable testing.
- Host-only helpers must be explicit and local; shared or wasm-exercised test surfaces must stay aligned with the same async-trait and boundary contracts as production.

### Host-Only vs Shared Test Surfaces

`aura-testkit` now distinguishes between:

- shared test surfaces, which remain available on native and wasm-targeted test lanes and must follow the same portability and boundary model as production-facing helpers
- host-only test surfaces, which may use native task handles or short blocking critical sections, but must be explicitly gated and documented

Current host-only surface:

- `MockRuntimeBridge` in `src/mock_runtime_bridge.rs`

Why it is host-only:

- it owns native task handles for deterministic teardown
- it provides a native testing bridge over `RuntimeBridge`
- its short blocking critical sections are intentional for host-only deterministic cleanup, not a model for shared test infrastructure

Enforcement:

- `src/lib.rs` gates `mock_runtime_bridge` and the `MockRuntimeBridge` re-export behind `#[cfg(not(target_arch = "wasm32"))]`
- `scripts/check/testkit-exception-boundary.sh` keeps the `clippy::disallowed_types` exception set explicit and local to named files

### InvariantMockContractFidelity

Mock effects must preserve observable contracts used by production guard and protocol paths.

Enforcement locus:
- src mock handlers model production effect behavior for deterministic tests.
- Fixtures and helpers avoid hidden nondeterminism in conformance scenarios.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-testkit

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines expected observable semantics.
- [Testing Guide](../../docs/804_testing_guide.md) defines mock fidelity requirements.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-testkit` uses test-only `ActorOwned` mock state for deterministic control and models `MoveOwned` transfer and capability behavior so tests can exercise the production ownership contract faithfully.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| Stateful mock handlers and in-memory effect doubles | `ActorOwned` (test-only) | Individual mock/test harness owner controls mutation; tests and assertions observe. |
| Transfer/capability modeling helpers | `MoveOwned` | Helper issuing the modeled token/handoff owns the transfer; tests and assertions observe. |
| Deterministic time/network/simulation controllers | `ActorOwned` (test-only) | Controller instance owns mutation; tests and diagnostics observe. |
| Fixtures, builders, and capture/assertion helpers | `Pure` / `Observed` | Test harness caller owns setup; tests and diagnostics observe. |

### Capability-Gated Points

- mock/runtime bridge helpers that model authoritative lifecycle/readiness or capability behavior for tests must do so explicitly and remain test-only
- stateful effect doubles may expose mutation for deterministic control, but those shortcuts must not become production-facing backdoors
- mock transport/journal/time helpers should preserve the same capability and ownership semantics the production code expects to consume

## Testing

### Strategy

Mock contract fidelity and handler coverage are the primary concerns. Tests are organized into `tests/amp/` for AMP fixtures, `tests/consensus/` for consensus differential and conformance, `tests/protocol/` for protocol fixtures, and `tests/conformance/` for cross-implementation conformance. Top-level files cover handler coverage, effects, performance, and tree tests.

### Commands

```
cargo test -p aura-testkit
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Effect handler behavior wrong | MockContractFidelity | `tests/effect_handlers_test.rs` | Covered |
| AMP channel fixture wrong | — | `tests/amp/amp_channel.rs` | Covered |
| AMP consensus fixture wrong | — | `tests/amp/amp_consensus.rs` | Covered |
| Consensus differential fails | — | `tests/consensus/consensus_differential.rs` | Covered |
| Consensus ITF conformance wrong | — | `tests/consensus/consensus_itf_conformance.rs` | Covered |
| FROST pipelining broken | — | `tests/consensus/frost_pipelining_tests.rs` | Covered |
| Golden fixture drifts from production | MockContractFidelity | `tests/conformance/conformance_golden_fixtures.rs` | Covered |
| Lean differential fails | — | `tests/conformance/lean_differential.rs` | Covered |
| Time control non-deterministic | MockContractFidelity | `tests/unified_time_integration.rs` | Covered |
| Byzantine capability differential | — | `tests/byzantine_capability_differential.rs` | Covered |
| Effect type has no testkit handler | MockContractFidelity | `tests/handler_coverage_tests.rs` (9 tests) | Covered |
| Handler creation in all modes | MockContractFidelity | `tests/handler_creation_test.rs` (4 tests) | Covered |
| Effects integration broken | MockContractFidelity | `tests/effects_test.rs` (6 tests) | Covered |
| Tree under load fails | — | `tests/tree_scalability.rs` (5 tests) | Covered |
| Tree under chaos diverges | — | `tests/tree_chaos.rs` | Disabled (waiting on chaos infra) |
| Performance regression detected | — | `tests/performance_regression.rs` | Disabled (waiting on types) |

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
