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

## Ownership Model

- `aura-recovery` is primarily `Pure` recovery-domain logic plus explicit
  ceremony/workflow contracts.
- Recovery grants, approvals, and handoffs that require exclusivity should use
  `MoveOwned` surfaces.
- Long-lived recovery coordination should be explicit and single-owner rather
  than spread across wrappers and views.
- Recovery publication and transitions must remain capability-gated and typed.
- Recovery lifecycle reduction must preserve typed terminal failure/rejection
  detail in derived state rather than collapsing everything to bare `Failed`
  flags.
- `Observed` recovery views are downstream of authoritative recovery semantics.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/state/view logic | `Pure` | Deterministic recovery fact reduction and typed derived state. |
| recovery grants, approvals, handoffs, ceremonies, protocol contracts | `MoveOwned` | Exclusive recovery authority and handoff records remain explicit. |
| `RecoveryProtocolHandler` approval tracking | local single-owner mutation | Approval tracking is now handler-local mutable state, not shared across clones. |
| long-lived recovery coordination | selective single-owner | Ongoing recovery coordination must stay explicit and not leak into views/wrappers. |
| capability-gated publication | typed ceremony/workflow boundary | Recovery transitions and publication remain explicit and auditable. |

### Capability-Gated Points

- grant/approval/recovery transitions
- ceremony and protocol publication consumed by higher-layer runtime/interface
  flows

### Verification Hooks

- `cargo check -p aura-recovery`
- `cargo test -p aura-recovery --lib test_recovery_failure_preserves_reason -- --nocapture`

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
