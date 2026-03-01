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

## Boundaries
- Transport-level connections live in aura-transport.
- Runtime descriptor cache lives in aura-agent.
- Network effect implementations live in aura-effects.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
