# Aura Recovery (Layer 5)

## Purpose

Guardian-based recovery protocol enabling threshold key recovery through social relationships. Includes guardian setup, membership management, and recovery ceremonies.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Recovery facts, reducers, and deltas | Threshold cryptography (aura-core FROST primitives) |
| Guardian setup and membership coordination | Consensus coordination (aura-consensus) |
| Recovery ceremonies and protocol handlers | Runtime recovery service (aura-agent) |
| Recovery state and guardian profile management | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers, threshold types |
| Incoming | aura-authentication | Recovery context, operation types |
| Incoming | aura-journal | Fact infrastructure |
| Incoming | aura-macros | Capability boundary and derive macros |
| Outgoing | — | `RecoveryFact`, `RecoveryFactReducer`, `RecoveryDelta` for journal integration |
| Outgoing | — | `RecoveryEffects`, `RecoveryNetworkEffects` for recovery operations |
| Outgoing | — | `GuardianSetupCoordinator`, `GuardianMembershipCoordinator` for guardian management |
| Outgoing | — | `GuardianCeremony`, `RecoveryCeremony` for multi-party flows |
| Outgoing | — | `RecoveryProtocol`, `RecoveryProtocolHandler` for recovery execution |
| Outgoing | — | `RecoveryState`, `GuardianProfile`, `GuardianSet` for state management |

## Invariants

- Facts must be reduced under their matching `ContextId`.
- Recovery and guardian membership transitions are consensus-gated (Category C).
- Guardian threshold must be satisfied for successful recovery.
- `RecoveryProtocol`, `GuardianSetup`, and `GuardianCeremony` require the
  `AuraAuthorityEvidence` theorem pack at launch time.

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

### InvariantRecoveryAuthorityEvidenceAdmission

The recovery flows that depend on authoritative read/materialization semantics
must fail closed unless the runtime admits `AuraAuthorityEvidence`.

Enforcement locus:
- `src/recovery_protocol.tell`
- `src/guardian_setup.tell`
- `src/guardian_ceremony.tell`
- Aura runtime admission through `aura-agent::runtime::choreo_engine`

Failure mode:
- Recovery launch proceeds on a runtime that cannot honor authoritative
  evidence/materialization assumptions.

Verification hooks:
- `cargo test -p aura-recovery`
- `cargo test -p aura-agent recovery -- --nocapture`

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-recovery` is primarily `Pure` recovery-domain logic plus explicit ceremony/workflow contracts.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/state/view logic | `Pure` | Deterministic recovery fact reduction and typed derived state. |
| recovery grants, approvals, handoffs, ceremonies, protocol contracts | `MoveOwned` | Exclusive recovery authority and handoff records remain explicit. |
| `RecoveryProtocolHandler` approval tracking | local single-owner mutation | Approval tracking is handler-local mutable state, not shared across clones. |
| long-lived recovery coordination | selective single-owner | Ongoing recovery coordination must stay explicit and not leak into views/wrappers. |
| capability-gated publication | typed ceremony/workflow boundary | Recovery transitions and publication remain explicit and auditable. |

### Capability-Gated Points

- grant/approval/recovery transitions
- ceremony and protocol publication consumed by higher-layer runtime/interface flows

## Testing

### Strategy

Guardian threshold enforcement and ceremony safety are the primary concerns. Integration tests in `tests/ceremony/` verify protocol choreography, ceremony types, and invariant properties. Inline tests verify fact reduction, state derivation, and membership change safety.

### Commands

```
cargo test -p aura-recovery
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Recovery with < threshold guardians succeeds | `src/state.rs` `test_setup_threshold_met`, `test_setup_state_derivation` | Covered |
| Duplicate share inflates count past threshold | `src/state.rs` `test_duplicate_share_submission_deduplicated` | Covered |
| Fact reduces under wrong context | `src/facts.rs` `test_reducer_rejects_context_mismatch` | Covered |
| Remove last guardian leaves unrecoverable | `src/guardian_membership.rs` `test_apply_remove_last_guardian_fails` | Covered |
| Duplicate guardian inflates quorum | `src/guardian_membership.rs` `test_apply_add_duplicate_guardian_fails` | Covered |
| Setup fails when guardians decline below threshold | `src/state.rs` `test_setup_failed` | Covered |
| Ceremony ID collision across instances | `tests/ceremony/` `ceremony_id_collision_resistance` | Covered |
| Independent fact reduction non-commutative | `src/view.rs` `test_reduction_commutes_for_independent_facts` | Covered |
| Recovery disputed after approval | `src/state.rs` `test_recovery_disputed` | Covered |
| Protocol choreography incoherent | `tests/ceremony/` `recovery_protocol_choreography_is_coherent_and_orphan_free` | Covered |
| Authority-evidence theorem-pack metadata drifts | `src/recovery_protocol.rs`, `src/guardian_setup.rs`, `src/guardian_ceremony.rs` theorem-pack tests | Covered |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Relational Contexts](../../docs/114_relational_contexts.md)
- [Operation Categories](../../docs/109_operation_categories.md)
