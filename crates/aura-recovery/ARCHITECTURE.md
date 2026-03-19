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
## Testing

### Strategy

Guardian threshold enforcement and ceremony safety are the primary concerns.
Integration tests in `tests/ceremony/` verify protocol choreography, ceremony
types, and invariant properties. Inline tests verify fact reduction, state
derivation, and membership change safety.

### Running tests

```
cargo test -p aura-recovery
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Recovery with < threshold guardians succeeds | `src/state.rs` `test_setup_threshold_met`, `test_setup_state_derivation` | Covered |
| Setup fails when guardian declines below threshold | `src/state.rs` `test_setup_failed` | Covered |
| Fact reduces under wrong context | `src/facts.rs` `test_reducer_rejects_context_mismatch` | Covered |
| Remove last guardian leaves account unrecoverable | `src/guardian_membership.rs` `test_apply_remove_last_guardian_fails` | Covered |
| Duplicate guardian inflates quorum | `src/guardian_membership.rs` `test_apply_add_duplicate_guardian_fails` | Covered |
| Fact sub-type collision in journal | `src/facts.rs` `test_sub_type_uniqueness` | Covered |
| Ceremony ID collision across instances | `tests/ceremony/` `ceremony_id_collision_resistance` | Covered |
| Reduction non-idempotent | `src/facts.rs` `test_reducer_idempotence` | Covered |
| Independent fact reduction non-commutative | `src/view.rs` `test_reduction_commutes_for_independent_facts` | Covered |
| Threshold must be positive and ≤ guardian count | `tests/ceremony/` `threshold_must_be_positive` | Covered |
| Guardian set contains duplicates | `tests/ceremony/` `recovery_requires_unique_guardians` | Covered |
| Protocol choreography incoherent | `tests/ceremony/` `recovery_protocol_choreography_is_coherent_and_orphan_free` | Covered |

## Boundaries
- Threshold cryptography lives in aura-core (FROST primitives).
- Consensus coordination lives in aura-consensus.
- Runtime recovery service lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
