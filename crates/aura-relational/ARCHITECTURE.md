# Aura Relational (Layer 5)

## Purpose

Contact and relationship management providing cross-authority relationship facts, guardian request handling, and consensus-backed relationship establishment.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Contact and guardian binding facts and reducers | Recovery protocol logic (aura-recovery) |
| Guardian request handling and service | Consensus coordination (aura-consensus) |
| Cross-authority relational context | Runtime relationship cache (aura-agent) |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers, relational types |
| Incoming | aura-journal | Fact infrastructure, reduction |
| Incoming | aura-consensus | Cross-authority agreement |
| Outgoing | — | `ContactFact`, `ContactFactReducer` for contact relationship facts |
| Outgoing | — | `GuardianRequest`, `GuardianRequestState` for guardian binding requests |
| Outgoing | — | `GuardianService` for guardian relationship management |
| Outgoing | — | `RelationalContext` for cross-authority context |

## Invariants

- Facts must be reduced under their matching `ContextId`.
- Cross-authority relationships are established through explicit consensus flows.
- Guardian bindings require mutual agreement.

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

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-relational` is primarily `Pure` relational-domain logic.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/context/domain types | `Pure` | Deterministic relational fact reduction and context-scoped relationship semantics. |
| relationship establishment, guardian binding requests, mutual-agreement flows | `MoveOwned` | Exclusive relationship authority and agreement handoff remain explicit. |
| cross-authority consensus-backed operations | typed workflow boundary | Direct consensus integration is explicit; no hidden adapter layer remains. |
| long-lived coordination | none local | Runtime relationship workflow ownership belongs in higher layers. |
| Observed-only surfaces | `Observed` | Observation does not author relational truth. |

### Capability-Gated Points

- parity-critical relational fact publication
- consensus-backed relationship establishment and guardian binding transitions

## Testing

### Strategy

All tests are inline — appropriate for a relational domain crate with no integration test surface. Tests verify fact reduction, guardian binding, and context scoping. Mutual agreement enforcement is consensus-gated (Category C) and tested at the consensus layer.

### Commands

```
cargo test -p aura-relational
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Contact fact reduces under wrong context | `src/facts.rs` `test_contact_reducer_rejects_context_mismatch` | Covered |
| Guardian binding reduces under wrong context | `src/facts.rs` `test_guardian_binding_reducer_rejects_context_mismatch` | Covered |
| Contact fact serialization lossy | `src/facts.rs` `test_contact_fact_serialization` | Covered |
| Reducer non-idempotent | `src/facts.rs` `test_contact_reducer_idempotence` | Covered |
| Guardian binding details roundtrip lossy | `src/facts.rs` `guardian_binding_details_roundtrip` | Covered |
| Guardian binding builder wrong | `src/guardian.rs` `test_guardian_binding_builder` | Covered |
| Emergency op not distinguished | `src/guardian.rs` `test_recovery_op_emergency` | Covered |
| Mutual agreement bypassed | Consensus-gated (Category C, tested at L4) | Cross-crate |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Relational Contexts](../../docs/114_relational_contexts.md)
- [Operation Categories](../../docs/109_operation_categories.md)
