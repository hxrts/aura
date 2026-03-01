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

