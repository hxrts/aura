# Aura Invitation (Layer 5) - Architecture and Invariants

## Purpose
Invitation protocol for establishing relationships between authorities, including
invitation creation, redemption, and ceremony coordination.

## Inputs
- aura-core (effect traits, identifiers).
- aura-authentication (session and identity verification).
- aura-authorization (Biscuit tokens for invitation capabilities).
- aura-guards (invitation guards).

## Outputs
- `InvitationFact`, `InvitationFactReducer`, `InvitationDelta` for journal integration.
- `InvitationCeremony` for multi-party invitation flows.
- `InvitationProtocol` for invitation message exchange.
- `InvitationService` for invitation lifecycle management.
- `InvitationGuards` for authorization checks.
- `Relationship` struct for established connections.

## Invariants
- Facts with known context must reduce under their matching `ContextId`.
- Invitation identifiers are treated as stable binding keys.
- Invitation redemption creates mutual relational context.

## Ownership Model

- `aura-invitation` is primarily `Pure` invitation-domain logic plus single-owner
  workflow contracts.
- Invitation lifecycle handles, acceptance ownership, and transfer surfaces
  should be explicit and `MoveOwned`.
- Long-lived invitation coordination should be single-owner and capability-gated.
- Invitation operations must end with typed terminal success, failure, or
  cancellation.
- Ceremony and protocol state machines must encode abort/failure/decline with
  typed terminal payloads rather than raw string reasons.
- `Observed` layers may display invitation state but must not synthesize
  semantic truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/domain types | `Pure` | Deterministic invitation fact reduction and relationship-binding semantics. |
| invitation lifecycle handles, acceptance/redemption flows, ceremony/protocol state | `MoveOwned` | Exclusive invitation authority and lifecycle ownership remain explicit. |
| long-lived invitation coordination | selective single-owner | Ongoing invitation coordination must stay single-owner and capability-gated. |
| capability-gated publication | typed workflow boundary | Invitation creation/acceptance/redemption publication stays explicit and terminally typed. |
| Observed-only surfaces | invitation view consumers only | UI/runtime observation remains downstream. |

### Capability-Gated Points

- invitation creation, acceptance, redemption, and relationship-establishment
  boundaries
- ceremony/protocol publication consumed by higher-layer runtime and interface
  flows

### Verification Hooks

- `cargo check -p aura-invitation`
- `cargo test -p aura-invitation -- --nocapture`

### Detailed Specifications

### InvariantInvitationRedemptionUniqueness
Invitation redemption must be unique and must produce consistent relational context state.

Enforcement locus:
- src invitation fact reducers validate identifier and context binding.
- Redemption writes journal evidence for replay and audit.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-invitation

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines context-scoped fact semantics.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines invitation safety expectations.
## Testing

### Strategy

Invitation redemption uniqueness and ceremony correctness are the primary
concerns. Integration tests in `tests/ceremony/` verify end-to-end send/accept
flows with guard evaluation. The contact establishment matrix stays top-level
as a cross-flow equivalence test. Inline tests verify fact reduction, guard
evaluation, descriptor validity, and protocol serialization.

### Running tests

```
cargo test -p aura-invitation
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Fact reduces under wrong context | `src/facts.rs` `test_reducer_rejects_context_mismatch` | Covered |
| Expired descriptor accepted | `src/descriptor.rs` `test_is_expired`, `test_is_valid_at` | Covered |
| Capability check bypassed | `src/service.rs` `test_prepare_send_invitation_missing_capability` | Covered |
| Insufficient budget allows invitation | `src/service.rs` `test_prepare_send_invitation_insufficient_budget` | Covered |
| E2E send → accept produces wrong state | `tests/ceremony/invitation_service_e2e.rs` | Covered |
| Contact flows produce different facts | `tests/contact_establishment_matrix.rs` | Covered |
| Ceremony ID non-deterministic | `src/invitation_ceremony.rs` `test_ceremony_id_determinism` | Covered |
| Fact serialization roundtrip lossy | `src/facts.rs` `test_invitation_fact_serialization` | Covered |
| Reducer non-idempotent | `src/facts.rs` `test_reducer_idempotence` | Covered |
| Protocol message serialization breaks | `src/protocol.rs` (15 inline tests) | Covered |
| Message exceeds max length | `src/service.rs` `test_prepare_send_invitation_message_too_long` | Covered |
| Legacy ceremony payload incompatible | `src/view.rs` `test_view_reducer_handles_legacy_ceremony_committed_payload` | Covered |

## Boundaries
- Relationship state lives in aura-relational.
- Transport coordination lives in aura-protocol.
- Runtime invitation cache lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
