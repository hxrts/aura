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
## Boundaries
- Foundation layers should create internal test utilities instead.
- Production handlers live in aura-effects (stateless).
- Simulation runtime lives in aura-simulator.
