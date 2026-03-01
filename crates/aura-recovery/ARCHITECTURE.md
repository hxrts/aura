# Aura Recovery (Layer 5) - Architecture and Invariants

## Purpose
Guardian-based recovery protocol enabling threshold key recovery through social
relationships. Includes guardian setup, membership management, and recovery ceremonies.

## Inputs
- aura-core (effect traits, identifiers, threshold types).
- aura-authentication (recovery context, operation types).
- aura-journal (fact infrastructure).

## Outputs
- `RecoveryFact`, `RecoveryFactReducer`, `RecoveryDelta` for journal integration.
- `RecoveryEffects`, `RecoveryNetworkEffects` for recovery operations.
- `GuardianSetupCoordinator`, `GuardianMembershipCoordinator` for guardian management.
- `GuardianCeremony`, `RecoveryCeremony` for multi-party flows.
- `RecoveryProtocol`, `RecoveryProtocolHandler` for recovery execution.
- `RecoveryState`, `GuardianProfile`, `GuardianSet` for state management.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Recovery and guardian membership transitions are consensus-gated (Category C).
- Guardian threshold must be satisfied for successful recovery.

### Detailed Specifications

### InvariantRecoveryThresholdEnforcement
Recovery transitions require guardian threshold satisfaction and consensus-gated membership changes.

Enforcement locus:
- src recovery reducers and services validate guardian threshold state.
- Category C transitions rely on consensus outputs.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-recovery

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines monotone state transitions.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines threshold safety expectations.
## Boundaries
- Threshold cryptography lives in aura-core (FROST primitives).
- Consensus coordination lives in aura-consensus.
- Runtime recovery service lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

