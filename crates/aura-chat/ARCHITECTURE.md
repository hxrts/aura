# Aura Chat (Layer 5) - Architecture and Invariants

## Purpose
Secure messaging domain providing channel management, message facts, and chat
state reduction for encrypted group and direct messaging.

## Inputs
- aura-core (effect traits, identifiers: ChannelId, ContextId).
- aura-journal (fact infrastructure, reduction pipeline).

## Outputs
- `ChatFact`, `ChatFactReducer`, `ChatDelta` for journal integration.
- `ChatFactService` for message and channel operations.
- `ChatGroup` for group membership management.
- `ChatViewReducer` for deriving chat state views.
- `ChatGuards` for message authorization.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Message payloads are opaque bytes; decryption is a higher-layer concern.
- Channel creation and membership changes are journaled as facts.

## Ownership Model

- `aura-chat` is primarily `Pure` fact, reducer, and workflow-domain logic.
- Chat/channel authority transfer and operation handles should be explicit and
  `MoveOwned` where exclusivity matters.
- Long-lived mutable chat runtime ownership belongs in explicit `ActorOwned`
  services in higher layers, not hidden in chat helpers.
- Message and membership publication must remain capability-gated and typed.
- `Observed` chat views are downstream and must not author chat truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/view reduction | `Pure` | Deterministic chat fact reduction and derived state. |
| message/channel/membership workflow-domain operations | `MoveOwned` | Exclusive channel/message authority and operation handles remain explicit. |
| long-lived mutable chat ownership | none local | Runtime chat coordination and caches belong in higher layers. |
| capability-gated publication | typed domain/workflow boundary | Message and membership publication stay explicit and auditable. |
| Observed-only surfaces | chat view consumers only | UI/runtime views remain downstream. |

### Capability-Gated Points

- message publication and membership-change admission
- chat/channel operations consumed by higher-layer guards and runtime services

### Verification Hooks

- `cargo check -p aura-chat`
- `cargo test -p aura-chat -- --nocapture`

### Detailed Specifications

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
## Boundaries
- Encryption/decryption lives in aura-effects (crypto handlers).
- Transport coordination lives in aura-protocol.
- Runtime caching lives in aura-agent services.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
