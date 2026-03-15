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
- `Observed` consumers may render authentication state but not author it.

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
## Boundaries
- Session ticket cryptography lives in aura-signature.
- Biscuit token management lives in aura-authorization.
- Runtime service wrappers live in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
