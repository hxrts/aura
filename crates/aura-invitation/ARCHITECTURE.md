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
## Boundaries
- Relationship state lives in aura-relational.
- Transport coordination lives in aura-protocol.
- Runtime invitation cache lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
