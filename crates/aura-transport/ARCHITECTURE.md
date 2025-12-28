# Aura Transport (Layer 2) - Architecture and Invariants

## Purpose
Define P2P communication abstractions and transport semantics with privacy-by-design
and authority-centric messaging.

## Inputs
- aura-core (domain types, effect traits, identifiers).

## Outputs
- Transport types: `Envelope`, `ScopedEnvelope`, `TransportConfig`.
- Connection types: `ConnectionId`, `ConnectionInfo`, `ConnectionState`.
- Privacy types: `PrivacyLevel`, `FrameHeader`, `FrameType`.
- Peer management: `PeerInfo`, `BlindedPeerCapabilities`.
- Context-scoped transport: `ContextTransportMessage`, `ContextTransportSession`.
- Fact types: `TransportFact` (state changes).
- AMP types: `AmpHeader`, `AmpRatchetState`.

## Invariants
- Privacy-by-design: mechanisms integrated into core types.
- Authority-centric: uses `AuthorityId` for cross-authority communication.
- Context-scoped: uses `ContextId` for relational context scoping.
- Compatible with rumpsteak-aura session types.

## Boundaries
- No actual network I/O (use TransportEffects).
- Transport handlers live in aura-effects.
- Coordination lives in aura-protocol.
