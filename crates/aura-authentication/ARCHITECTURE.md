# Aura Authentication (Layer 5) - Architecture and Invariants

## Purpose
End-to-end authentication protocol including challenge-response flows, session
management, device key derivation, and guardian-based recovery authorization.

## Inputs
- aura-core (effect traits, identifiers).
- aura-authorization (Biscuit tokens, capabilities).
- aura-signature (session types, identity verification).
- aura-guards (guard evaluation, Biscuit integration).

## Outputs
- `AuthFact`, `AuthFactReducer`, `AuthFactDelta` for journal integration.
- `AuthService` for session ticket issuance and validation.
- `AuthGuards` for recovery operation authorization.
- `DkdDerivation` for device key derivation.
- `AuthView` for authentication state queries.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Session and request identifiers are treated as stable binding keys.
- Recovery and guardian approval flows are consensus-gated (Category C).

## Ownership Model

- `aura-authentication` is primarily `Pure` domain logic plus workflow
  contracts.
- Authentication ceremonies that transfer exclusive authority should expose
  `MoveOwned` handles or handoff records rather than shared mutable owner
  fields.
- Long-lived ceremony coordination should be explicit and single-owner.
- Capability-gated publication and typed terminal failure are required for
  parity-critical authentication flows.
- Authentication view reduction must preserve typed terminal failure/denial
  detail instead of collapsing failures to request/session ids plus free text.
- `Observed` consumers may render authentication state but not author it.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/views | `Pure` | Authentication state reduction and typed view projection. |
| ceremonies, request/session workflows, recovery authorization flows | `MoveOwned` | Exclusive request/session/approval authority remains explicit. |
| long-lived ceremony coordination | selective single-owner | Any ongoing coordination must remain explicit and single-owner, not ambient shared state. |
| capability-gated publication | typed workflow boundary | Authentication publication and denial/failure surfaces stay typed and explicit. |
| Observed-only surfaces | `AuthView` consumers only | UI/runtime observation remains downstream. |

### Capability-Gated Points

- recovery authorization and guardian approval flows
- parity-critical authentication publication and session/recovery outcome
  surfaces

### Verification Hooks

- `cargo check -p aura-authentication`
- `cargo test -p aura-authentication -- --nocapture`

### Detailed Specifications

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
## Testing

### Strategy

All tests are inline — appropriate for an authentication crate with no
integration test surface beyond the service layer. Tests verify session
lifecycle, guard evaluation, fact reduction, and DKD protocol correctness.

### Running tests

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
| DKD agreement mode wrong | `src/dkd.rs` `test_dkd_agreement_mode_requires_consensus` | Covered |
| DKD contribution validation fails on mismatch | `src/dkd.rs` `test_contribution_validation` | Covered |

## Boundaries
- Session ticket cryptography lives in aura-signature.
- Biscuit token management lives in aura-authorization.
- Runtime service wrappers live in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
