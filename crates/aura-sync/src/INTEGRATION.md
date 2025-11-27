<!-- Aura Sync Integration Guide -->

# Integrating Aura Sync (Layer 5)

Aura Sync provides end-to-end synchronization protocols that are assembled from the lower layers of the architecture. Use this guide to wire the crate into applications and simulations without breaking layer boundaries.

## Required Effects
- `NetworkEffects` for transport
- `JournalEffects` for fact retrieval and commits
- `CryptoEffects` for hashing and signature checks
- `PhysicalTimeEffects` for timeouts and scheduling
- `RandomEffects` for nonce generation

All protocol entry points are generic over these traits; pass the effect system from `aura-agent` in production or `aura-simulator`/`aura-testkit` for deterministic testing.

## Configuration
- Start from `SyncConfig::for_production()` or `SyncConfig::for_testing()`.
- Override values via environment variables (prefixed `AURA_SYNC_*`) or the builder for per-process tuning.
- Validate with `SyncConfig::validate()` before use to catch misconfigurations early.

## Typical Assembly
1. Load config: `let config = SyncConfig::from_env();`
2. Construct infrastructure: `PeerManager`, retry/backoff, caches.
3. Create protocols: `AntiEntropyProtocol`, `AuthorityJournalSync`, `NamespacedSync`.
4. Orchestrate via services: `maintenance::SyncService` to run periodic rounds.

## Authorization and Guard Chain
- Capability checks rely on Biscuit tokens evaluated by `AuthorizationEffects` before sending or applying any sync data.
- Ensure guard evaluators are provided by the runtime; sync protocols assume the guard chain (`CapGuard → FlowGuard → Leakage → Journal → Transport`) is active.

## Simulation and Testing
- Use `aura-simulator` for deterministic runs; inject simulated time and network delay via the effect system.
- `SyncConfig::for_testing()` minimizes timeouts and removes jitter for reproducibility.

## Observability
- Provide a `MetricsCollector` from `core::metrics` to capture protocol timings, retries, and failure reasons.
- Log transport and authorization failures at the orchestration layer to aid debugging.

## Safety Checklist
- No direct runtime calls: all I/O and timing must flow through effects.
- Validate Biscuit tokens before accepting peer data.
- Enforce flow budgets and leakage constraints when bridging to transport.
