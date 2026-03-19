# Aura Rendezvous (Layer 5) - Architecture and Invariants

## Purpose
Peer discovery and channel establishment protocol including descriptor exchange,
flood propagation, and LAN discovery for P2P connectivity.

## Inputs
- aura-core (effect traits, identifiers, capability types).
- aura-journal (fact infrastructure, capability refs).

## Outputs
- `RendezvousFact`, `RendezvousFactReducer` for descriptor facts.
- `RendezvousProtocol` for discovery message exchange.
- `RendezvousService` for descriptor lifecycle management.
- `RendezvousDescriptor` for peer addressing.
- `NewChannelProtocol` for channel establishment.
- `FloodPropagation`, `PacketBuilder`, `PacketCrypto` for flood-based discovery.
- `LanDiscovery` for local network peer discovery.

## Invariants
- Descriptor facts must reduce under their matching `ContextId`.
- Channel establishment requires valid, non-expired descriptors.
- Flood packets use nonce-based replay protection.

## Ownership Model

- `aura-rendezvous` combines `Pure` descriptor semantics with explicit
  `MoveOwned` channel-establishment authority where exclusivity matters.
- Long-lived peer/discovery runtime ownership belongs in explicit `ActorOwned`
  runtime services, not hidden in rendezvous helpers.
- Descriptor and channel-establishment publication must remain capability-gated
  and typed.
- Retry/lifecycle outcomes should terminate explicitly rather than relying on
  implicit background ownership.
- Rendezvous/channel-establishment lifecycle state must use typed terminal
  failure enums rather than stringly `Failed { reason }` or `Error(String)`
  payloads.
- `Observed` consumers may render rendezvous state but not author it.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| descriptor facts/reducers and validation logic | `Pure` | Deterministic descriptor semantics and reduction. |
| channel establishment, handshake, and protocol state | `MoveOwned` | Exclusive channel-establishment authority remains explicit and typed. |
| `RendezvousService` local caches/handshakers | local service-owned mutation | Local descriptor cache and handshake registry are service-local state, not shared semantic ownership across layers. |
| `FloodPropagation` topology/budget/nonce state | bounded coordination state | Uses injected topology references and local flood bookkeeping without becoming a global runtime owner. |
| long-lived discovery runtime ownership | none local | Ongoing peer/discovery ownership belongs in higher-layer runtime services. |

### Capability-Gated Points

- descriptor publication and channel-establishment publication
- retry/lifecycle outcomes consumed by higher-layer runtime/interface flows

### Verification Hooks

- `cargo check -p aura-rendezvous`
- `cargo test -p aura-rendezvous -- --nocapture`

### Detailed Specifications

### InvariantSecureChannelLifecycle
Secure channels are bound to `(context_id, peer, epoch)` and follow a strict lifecycle state machine.

Enforcement locus:
- `src/new_channel.rs`: channel state machine and transition checks.
- `src/new_channel.rs`: epoch mismatch detection and rotation handling.
- `src/new_channel.rs`: context and peer keyed lookup prevents channel aliasing.
- descriptor validation path: establishment requires non-expired rendezvous descriptors.

Failure mode:
- Stale epoch traffic accepted after rotation.
- Messages routed to wrong context or peer.
- Invalid transition sequences that produce undefined channel state.

Verification hooks:
- `cargo test -p aura-rendezvous new_channel`
- `cargo test -p aura-rendezvous channel`
- simulator scenarios for epoch rotation and replay attempts

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) requires typed, context-scoped communication transitions.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) requires receipt validity windows and replay prevention.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) requires epoch validity and transport safety properties.

## Testing

### Strategy

Channel lifecycle correctness and descriptor validity are the primary concerns.
Integration tests in `tests/channel/` verify end-to-end flows from descriptor
publication through handshake completion. Inline tests verify channel state
machine transitions, fact reduction, flood deduplication, and protocol
serialization.

### Running tests

```
cargo test -p aura-rendezvous
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Epoch regression accepted (stale keys) | `src/new_channel.rs` `channel_rotate_regression_marks_typed_error` | Covered |
| Handshake invalid state produces undefined channel | `src/new_channel.rs` `handshake_invalid_state_marks_typed_failure` | Covered |
| Expired descriptor accepted | `tests/channel/` `test_channel_establishment_rejects_expired_descriptor` | Covered |
| Descriptor validity window wrong | `src/facts.rs` `test_descriptor_validity` | Covered |
| Channel context non-commutative (peers can't find channel) | `src/facts.rs` `test_channel_context_is_commutative` | Covered |
| Descriptor context mismatch accepted | `src/facts.rs` `test_reducer_rejects_context_mismatch_for_descriptor` | Covered |
| PSK mismatch not detected | `tests/channel/` `test_handshake_psk_mismatch_detection` | Covered |
| Flood packet replayed | `src/flood/propagation.rs` `test_seen_nonces_check_and_mark` | Covered |
| Nonce tracker unbounded memory | `src/flood/propagation.rs` `test_seen_nonces_capacity_clear` | Covered |
| Missing capability allows connect | `tests/channel/` `test_missing_capability_blocks_connect` | Covered |
| Insufficient budget allows publish | `tests/channel/` `test_insufficient_flow_budget_blocks_publish` | Covered |
| E2E discovery → channel flow broken | `tests/channel/` `test_complete_discovery_to_channel_flow` | Covered |

## Boundaries
- Transport-level connections live in aura-transport.
- Runtime descriptor cache lives in aura-agent.
- Network effect implementations live in aura-effects.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
