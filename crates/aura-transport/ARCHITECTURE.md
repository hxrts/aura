# Aura Transport (Layer 2)

## Purpose

Define P2P communication abstractions and transport semantics with privacy-by-design
and authority-centric messaging.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Transport types: `Envelope`, `ScopedEnvelope`, `TransportConfig` | Actual network I/O (use `TransportEffects`) |
| Connection types: `ConnectionId`, `ConnectionInfo`, `ConnectionState` | Transport handler implementations (live in `aura-effects`) |
| Privacy types: `PrivacyLevel`, `FrameHeader`, `FrameType` | Coordination logic (use `aura-protocol`) |
| Peer management: `PeerInfo`, `BlindedPeerCapabilities` | |
| Context-scoped transport: `ContextTransportMessage`, `ContextTransportSession` | |
| Fact types: `TransportFact` (state changes) | |
| AMP types: `AmpHeader`, `AmpRatchetState` | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Inbound | `aura-core` | Domain types, effect traits, identifiers |

## Invariants

- Privacy-by-design: mechanisms integrated into core types.
- Authority-centric: uses `AuthorityId` for cross-authority communication.
- Context-scoped: uses `ContextId` for relational context scoping.
- Compatible with Telltale session types.
- Canonical transport invariants use contract names: `InvariantSequenceMonotonic`,
  `InvariantContextIsolation`, `InvariantReceiptValidityWindow`, and
  `InvariantCrossEpochReplayPrevention`.

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

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-transport` is primarily `Pure`. It defines transport-domain semantics and
typed receipts, not `ActorOwned` connection ownership. Channel/session transfer
semantics requiring exclusivity are carried as `MoveOwned` contracts in higher
layers. `Observed` projections consume these contracts downstream.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/types.rs`, `src/envelope.rs`, `src/receipt.rs`, `src/privacy.rs` | `Pure` | Transport message, receipt, and privacy semantics. |
| `src/protocols/` | `Pure`, `MoveOwned` | Session/channel descriptors and protocol state are explicit values; protocol timeout/retry settings here are configuration data, not owner-run loops. |
| `src/facts.rs` | `Pure` | Fact-backed transport state transitions. |
| Actor-owned runtime state | none | Connection ownership and live peer state belong in higher layers. |
| Observed-only surfaces | none | Observation of transport state belongs in runtime/interface layers. |

### Capability-Gated Points

- Typed send/receive authority surfaces consumed by higher-layer guards
- Receipt and transport fact semantics used by higher-layer mutation/publication gates

## Testing

### Strategy

aura-transport defines wire protocol types. If serialization breaks, peers
can't communicate. If context isolation breaks, messages leak across
relationships. If epoch validation breaks, replay attacks succeed.

### Commands

```
cargo test -p aura-transport --test wire  # wire protocol contracts
cargo test -p aura-transport --lib        # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Envelope serialization changes | — | `tests/wire/envelope_roundtrip.rs` | covered |
| Connection state machine invalid | — | `tests/wire/envelope_roundtrip.rs` | covered |
| Context A message delivered in context B | InvariantContextIsolation | `tests/wire/context_isolation.rs` | covered (+ ScopedEnvelope fix) |
| AMP ratchet rejects valid messages | — | `tests/wire/amp_ratchet.rs` | covered |
| Old-epoch message accepted | InvariantCrossEpochReplayPrevention | `tests/wire/context_isolation.rs` | covered |
| Future non-pending epoch accepted | InvariantCrossEpochReplayPrevention | `tests/wire/context_isolation.rs` | covered |
| Generation outside window accepted | InvariantSequenceMonotonic | `tests/wire/context_isolation.rs` | covered (boundary tests) |
| Window boundary off-by-one | InvariantSequenceMonotonic | `src/amp.rs` inline | covered |
| Message key derivation non-deterministic | — | `src/amp.rs` inline | covered |
| Privacy level semantics wrong | — | `src/types/envelope.rs` inline | covered |

## References

- [Transport and Information Flow](../../docs/111_transport_and_information_flow.md)
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
