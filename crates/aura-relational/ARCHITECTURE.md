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

### Detailed Specifications

### InvariantRelationalMutualAgreement
Relational state activation requires explicit mutual agreement across authorities.

Enforcement locus:
- src relationship reducers validate bilateral facts before active state.
- Cross-authority flows route through consensus and journal evidence.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-relational

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines context-bound relational state.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines operation-scoped agreement.
## Boundaries
- Recovery protocol logic lives in aura-recovery.
- Consensus coordination lives in aura-consensus.
- Runtime relationship cache lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

