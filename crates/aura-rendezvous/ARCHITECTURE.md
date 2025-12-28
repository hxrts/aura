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

## Boundaries
- Transport-level connections live in aura-transport.
- Runtime descriptor cache lives in aura-agent.
- Network effect implementations live in aura-effects.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
