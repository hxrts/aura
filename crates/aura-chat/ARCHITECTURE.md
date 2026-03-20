# Aura Chat (Layer 5)

## Purpose

Secure messaging domain providing channel management, message facts, and chat
state reduction for encrypted group and direct messaging.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Chat facts, reducers, and view derivation | Encryption/decryption (aura-effects crypto handlers) |
| Channel creation and membership management | Transport coordination (aura-protocol) |
| Message authorization guards | Runtime caching (aura-agent services) |
| Chat group membership logic | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers (`ChannelId`, `ContextId`) |
| Incoming | aura-journal | Fact infrastructure, reduction pipeline |
| Outgoing | — | `ChatFact`, `ChatFactReducer`, `ChatDelta` for journal integration |
| Outgoing | — | `ChatFactService` for message and channel operations |
| Outgoing | — | `ChatGroup` for group membership management |
| Outgoing | — | `ChatViewReducer` for deriving chat state views |
| Outgoing | — | `ChatGuards` for message authorization |

## Invariants

- Facts must be reduced under their matching `ContextId`.
- Message payloads are opaque bytes; decryption is a higher-layer concern.
- Channel creation and membership changes are journaled as facts.

### InvariantChatContextReduction

Chat facts reduce only within their context and preserve deterministic replay order.

Enforcement locus:
- src fact services and reducers validate context boundaries.
- Message and membership state transitions are journal backed.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-chat

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines context isolation and deterministic reduction.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines consistency expectations.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-chat` is primarily `Pure` fact, reducer, and workflow-domain logic.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/view reduction | `Pure` | Deterministic chat fact reduction and derived state. |
| message/channel/membership workflow-domain operations | `MoveOwned` | Exclusive channel/message authority and operation handles remain explicit. |
| long-lived mutable chat ownership | none local | Runtime chat coordination and caches belong in higher layers. |
| capability-gated publication | typed domain/workflow boundary | Message and membership publication stay explicit and auditable. |
| Observed-only surfaces | `Observed` | UI/runtime views remain downstream. |

### Capability-Gated Points

- message publication and membership-change admission
- chat/channel operations consumed by higher-layer guards and runtime services

## Testing

### Strategy

All tests are inline — appropriate for a messaging domain crate whose tests
verify fact reduction, guard evaluation, and view derivation. No integration
test surface is needed.

### Commands

```
cargo test -p aura-chat
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Message reduces in wrong context | `src/facts.rs` `reducer_rejects_context_mismatch` | Covered |
| Fact serialization roundtrip lossy | `src/facts.rs` `test_chat_fact_serialization` | Covered |
| Reducer non-idempotent | `src/facts.rs` `test_reducer_idempotence` | Covered |
| Type ID inconsistent across variants | `src/facts.rs` `test_type_id_consistency` | Covered |
| Capability check bypassed | `src/fact_service.rs` `denied_when_missing_capability` | Covered |
| Budget not charged before journal append | `src/fact_service.rs` `approved_orders_budget_before_journal_append` | Covered |
| Channel view delta compaction wrong | `src/view.rs` `test_compact_deltas_merges_channel_updates` | Covered |
| Sealed message leaks plaintext in view | `src/view.rs` `test_message_sent_reduction` (verifies `<sealed message>` placeholder) | Covered |
| Group membership check incorrect | `src/group.rs` `test_group_membership` | Covered |
| Message lifecycle timestamps wrong | `src/facts.rs` `test_message_lifecycle_facts` | Covered |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Operation Categories](../../docs/109_operation_categories.md)
