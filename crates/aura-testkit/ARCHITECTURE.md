# Aura Testkit (Layer 8) - Architecture and Invariants

## Purpose
Shared testing infrastructure providing test fixtures, effect system harnesses,
mock implementations, time control, and simulation support for deterministic
testing across all layers.

## Inputs
- All lower layers (Layer 1-7): core, domains, effects, protocols, features, runtime.
- Internal testing patterns and effect-based test harnesses.

## Outputs
- Test fixtures and builders (accounts, devices, authorities).
- Mock effect implementations: `MockCryptoHandler`, `MockTimeHandler`, `InMemoryStorageHandler`.
- Effect capture and assertion utilities.
- Deterministic time control and scheduling.
- Privacy analysis and information flow verification tools.

## Invariants
- Must NOT be imported by Layer 1-3 crates (would create circular dependencies).
- May be imported by Layer 4-7 in `[dev-dependencies]` only.
- Tests are deterministic and isolated.
- Mock effects behave consistently with production ones.
- Mock handlers MAY be stateful (using `Arc<Mutex<>>`) for controllable testing.

## Ownership Model

- `aura-testkit` may use test-only `ActorOwned` mock state for deterministic
  control.
- It may also model `MoveOwned` transfer and capability behavior so tests can
  exercise the production ownership contract faithfully.
- Test helpers must not become a backdoor for parity-critical production
  semantic authorship.
- Capability-aware shortcuts should be narrow, explicit, and documented.
- `Observed` diagnostics and fixtures remain downstream of the modeled semantic
  truth.

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Stateful mock handlers and in-memory effect doubles | `ActorOwned` (test-only) | individual mock/test harness owner | mock handler internals | tests and assertions |
| Transfer/capability modeling helpers | `MoveOwned` | helper issuing the modeled token/handoff | explicit helper APIs | tests and assertions |
| Deterministic time/network/simulation controllers | `ActorOwned` (test-only) | controller instance | controller internals | tests and diagnostics |
| Fixtures, builders, and capture/assertion helpers | `Observed` / `Pure` | test harness caller | helper-local setup state only | tests and diagnostics |

### Capability-Gated Points

- mock/runtime bridge helpers that model authoritative lifecycle/readiness or
  capability behavior for tests must do so explicitly and remain test-only
- stateful effect doubles may expose mutation for deterministic control, but
  those shortcuts must not become production-facing backdoors
- mock transport/journal/time helpers should preserve the same capability and
  ownership semantics the production code expects to consume

### Verification Hooks

- `cargo check -p aura-testkit`
- `cargo test -p aura-testkit -- --nocapture`
- targeted integration/property tests using the stateful handlers and
  controllable time/network surfaces
- `just ci-move-semantics`

### Detailed Specifications

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
## Testing

### Strategy

Mock contract fidelity and handler coverage are the primary concerns.
Tests are organized into `tests/amp/` for AMP fixtures, `tests/consensus/`
for consensus differential and conformance, `tests/protocol/` for protocol
fixtures, and `tests/conformance/` for cross-implementation conformance.
Top-level files cover handler coverage, effects, performance, and tree tests.

### Running tests

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
| Effect type has no testkit handler | MockContractFidelity | `tests/handler_coverage_tests.rs` | Disabled (`cfg(any())`) |
| Handler creation fails | MockContractFidelity | `tests/handler_creation_test.rs` | Disabled (`cfg(any())`) |
| Effects integration broken | MockContractFidelity | `tests/effects_test.rs` | Disabled (`cfg(any())`) |
| Tree under chaos diverges | — | `tests/tree_chaos.rs` | Disabled (`cfg(any())`) |
| Tree under load fails | — | `tests/tree_scalability.rs` | Disabled (`cfg(any())`) |
| Performance regression detected | — | `tests/performance_regression.rs` | Disabled (`cfg(any())`) |

## Boundaries
- Foundation layers should create internal test utilities instead.
- Production handlers live in aura-effects (stateless).
- Simulation runtime lives in aura-simulator.
