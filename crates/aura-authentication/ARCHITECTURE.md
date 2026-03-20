# Aura Authentication (Layer 5)

## Purpose

End-to-end authentication protocol including challenge-response flows, session
management, device key derivation, and guardian-based recovery authorization.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Auth facts, reducers, and view projection | Session ticket cryptography (aura-signature) |
| Session lifecycle and ticket issuance | Biscuit token management (aura-authorization) |
| Device key derivation (DKD) | Runtime service wrappers (aura-agent) |
| Recovery operation authorization guards | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers |
| Incoming | aura-authorization | Biscuit tokens, capabilities |
| Incoming | aura-signature | Session types, identity verification |
| Incoming | aura-guards | Guard evaluation, Biscuit integration |
| Outgoing | — | `AuthFact`, `AuthFactReducer`, `AuthFactDelta` for journal integration |
| Outgoing | — | `AuthService` for session ticket issuance and validation |
| Outgoing | — | `AuthGuards` for recovery operation authorization |
| Outgoing | — | `DkdDerivation` for device key derivation |
| Outgoing | — | `AuthView` for authentication state queries |

## Invariants

- Facts must be reduced under their matching `ContextId`.
- Session and request identifiers are treated as stable binding keys.
- Recovery and guardian approval flows are consensus-gated (Category C).

### InvariantAuthenticationContextBinding

Authentication facts and identifiers must stay bound to the correct context and consensus-gated transitions.

Enforcement locus:
- src reducers and services validate context scope and identifier binding.
- Category C transitions depend on consensus admission.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-authentication

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines context-scoped semantics.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines consensus-gated agreement.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-authentication` is primarily `Pure` domain logic plus workflow contracts.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/views | `Pure` | Authentication state reduction and typed view projection. |
| ceremonies, request/session workflows, recovery authorization flows | `MoveOwned` | Exclusive request/session/approval authority remains explicit. |
| long-lived ceremony coordination | selective single-owner | Any ongoing coordination must remain explicit and single-owner, not ambient shared state. |
| capability-gated publication | typed workflow boundary | Authentication publication and denial/failure surfaces stay typed and explicit. |
| Observed-only surfaces | `Observed` | UI/runtime observation remains downstream. |

### Capability-Gated Points

- recovery authorization and guardian approval flows
- parity-critical authentication publication and session/recovery outcome
  surfaces

## Testing

### Strategy

All tests are inline — appropriate for an authentication crate with no
integration test surface beyond the service layer. Tests verify session
lifecycle, guard evaluation, fact reduction, and DKD protocol correctness.

### Commands

```
cargo test -p aura-authentication
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Session lifecycle (create → active → revoke) | `src/view.rs` `test_session_lifecycle` | Covered |
| Revoked session still active | `src/view.rs` `test_session_lifecycle` | Covered |
| Expired session not detected | `src/view.rs` `test_expired_session_detection` | Covered |
| Disjoint fact reduction non-commutative | `src/view.rs` `test_reduce_all_commutes_for_disjoint_sessions` | Covered |
| Capability check bypassed | `src/guards.rs` `test_check_capability_failure`, `src/service.rs` `test_request_challenge_missing_capability` | Covered |
| Excessive session duration allowed | `src/guards.rs` `test_evaluate_session_creation_duration_exceeded` | Covered |
| Expired challenge accepted | `src/guards.rs` `test_check_challenge_expiry` | Covered |
| Fact serialization roundtrip lossy | `src/facts.rs` `test_fact_serialization` | Covered |
| Reducer non-idempotent | `src/facts.rs` `test_reducer_idempotence` | Covered |
| Guardian approval flow incorrect | `src/view.rs` `test_recovery_approval_flow`, `src/service.rs` `test_guardian_approval_request` | Covered |
| DKD agreement mode wrong | `src/dkd.rs` `test_dkd_agreement_mode_requires_consensus` | Covered |
| DKD contribution validation fails on mismatch | `src/dkd.rs` `test_contribution_validation` | Covered |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Authorization](../../docs/106_authorization.md)
- [Operation Categories](../../docs/109_operation_categories.md)
