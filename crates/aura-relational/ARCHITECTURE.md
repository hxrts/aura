# Aura Relational (Layer 5) - Architecture and Invariants

## Purpose
Contact and relationship management providing cross-authority relationship facts,
guardian request handling, and consensus-backed relationship establishment.

## Inputs
- aura-core (effect traits, identifiers, relational types).
- aura-journal (fact infrastructure, reduction).
- aura-consensus (for cross-authority agreement).

## Outputs
- `ContactFact`, `ContactFactReducer` for contact relationship facts.
- `GuardianRequest`, `GuardianRequestState` for guardian binding requests.
- `GuardianService` for guardian relationship management.
- `ConsensusAdapter` for consensus-backed operations.
- `RelationalContext` for cross-authority context.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Cross-authority relationships are established through explicit consensus flows.
- Guardian bindings require mutual agreement.

## Boundaries
- Recovery protocol logic lives in aura-recovery.
- Consensus coordination lives in aura-consensus.
- Runtime relationship cache lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
