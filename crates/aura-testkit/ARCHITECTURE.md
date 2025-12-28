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

## Boundaries
- Foundation layers should create internal test utilities instead.
- Production handlers live in aura-effects (stateless).
- Simulation runtime lives in aura-simulator.
