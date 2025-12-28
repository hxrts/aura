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

## Boundaries
- Relationship state lives in aura-relational.
- Transport coordination lives in aura-protocol.
- Runtime invitation cache lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
