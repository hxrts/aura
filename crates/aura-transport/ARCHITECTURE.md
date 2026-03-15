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
- Compatible with Telltale session types.
- Canonical transport invariants use contract names: `InvariantSequenceMonotonic`,
  `InvariantContextIsolation`, `InvariantReceiptValidityWindow`, and
  `InvariantCrossEpochReplayPrevention`.

## Ownership Model

- `aura-transport` is primarily `Pure`.
- It defines transport-domain semantics and typed receipts, not `ActorOwned`
  connection ownership.
- Channel/session transfer semantics that require exclusivity should be carried
  as `MoveOwned` contracts in higher layers.
- Capability-gated send and receive authority should remain explicit at the
  typed boundary.
- Runtime services and `Observed` projections consume these contracts
  downstream; they do not redefine them here.

### Detailed Specifications

### InvariantSequenceMonotonic
Transport sequencing and context scoping must remain monotone with fact-backed send observability.

Enforcement locus:
- src transport envelope and channel logic maintain sequence and epoch constraints.
- Transport operations integrate with guard-chain requirements in higher layers.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-transport

Contract alignment:
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines `InvariantReceiptValidityWindow` and `InvariantCrossEpochReplayPrevention`.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantSequenceMonotonic` and `InvariantContextIsolation`.
## Boundaries
- No actual network I/O (use TransportEffects).
- Transport handlers live in aura-effects.
- Coordination lives in aura-protocol.
