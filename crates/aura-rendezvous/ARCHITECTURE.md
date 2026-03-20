# Aura Rendezvous (Layer 5)

## Purpose

Peer discovery and channel establishment protocol including descriptor exchange,
flood propagation, and LAN discovery for P2P connectivity.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Descriptor facts, reducers, and validation | Transport-level connections (aura-transport) |
| Channel establishment and handshake protocol | Runtime descriptor cache (aura-agent) |
| Flood propagation and replay protection | Network effect implementations (aura-effects) |
| LAN discovery | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers, capability types |
| Incoming | aura-journal | Fact infrastructure, capability refs |
| Outgoing | — | `RendezvousFact`, `RendezvousFactReducer` for descriptor facts |
| Outgoing | — | `RendezvousProtocol` for discovery message exchange |
| Outgoing | — | `RendezvousService` for descriptor lifecycle management |
| Outgoing | — | `RendezvousDescriptor` for peer addressing |
| Outgoing | — | `NewChannelProtocol` for channel establishment |
| Outgoing | — | `FloodPropagation`, `PacketBuilder`, `PacketCrypto` for flood-based discovery |
| Outgoing | — | `LanDiscovery` for local network peer discovery |

## Invariants

- Descriptor facts must reduce under their matching `ContextId`.
- Channel establishment requires valid, non-expired descriptors.
- Flood packets use nonce-based replay protection.

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

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-rendezvous` combines `Pure` descriptor semantics with explicit `MoveOwned`
channel-establishment authority where exclusivity matters.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| descriptor facts/reducers and validation logic | `Pure` | Deterministic descriptor semantics and reduction. |
| channel establishment, handshake, and protocol state | `MoveOwned` | Exclusive channel-establishment authority remains explicit and typed. |
| `RendezvousService` local caches/handshakers | local service-owned mutation | Local descriptor cache and handshake registry are service-local state. |
| `FloodPropagation` topology/budget/nonce state | bounded coordination state | Uses injected topology references and local flood bookkeeping. |
| long-lived discovery runtime ownership | none local | Ongoing peer/discovery ownership belongs in higher-layer runtime services. |

### Capability-Gated Points

- descriptor publication and channel-establishment publication
- retry/lifecycle outcomes consumed by higher-layer runtime/interface flows

## Testing

### Strategy

Channel lifecycle correctness and descriptor validity are the primary concerns.
Integration tests in `tests/channel/` verify end-to-end flows from descriptor
publication through handshake completion. Inline tests verify channel state
machine transitions, fact reduction, flood deduplication, and protocol
serialization.

### Commands

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
| Handshake produces mismatched channels | `tests/channel/` `test_handshake_initiator_responder_flow` | Covered |
| Epoch advancement doesn't trigger rotation | `tests/channel/` `test_channel_manager_epoch_advancement` | Covered |
| E2E discovery → channel flow broken | `tests/channel/` `test_complete_discovery_to_channel_flow` | Covered |
| Flood key derivation non-unique | `src/flood/packet.rs` `test_derive_key_different_inputs` | Covered |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Rendezvous](../../docs/113_rendezvous.md)
- [Operation Categories](../../docs/109_operation_categories.md)
