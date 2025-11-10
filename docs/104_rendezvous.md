# 104 · Rendezvous 1.0 Specification

## Scope

This document defines the rendezvous system that ships with Aura 1.0. The goal is to provide dependable peer discovery and connection setup between accounts while reusing the primitives specified in `001_theoretical_foundations.md`, `002_system_architecture.md`, `003_distributed_applications.md`, and `004_info_flow_model.md`. The design favors simplicity over maximal privacy. Future releases can swap in richer privacy mechanics without rewriting the interfaces described here.

## Components

Each device runs a Rendezvous Manager. The manager exposes `establish_channel(AccountId)` and returns a transport handle that implements the existing `TransportEffects` trait set. Internally the manager coordinates three subsystems:

* **Envelope Store** keeps the most recent rendezvous offers and answers for every relationship in the unified journal (`sbb_envelopes`, `rendezvous_descriptors`). Entries are CRDT facts and follow the join rules in the whole-system calculus.
* **Neighbor Flooder** pushes new envelopes to current neighbors using the same gossip loop that replicates the journal. Flooding is per-context; only peers with the capability to see a relationship receive its envelopes.
* **Handshake Adapter** consumes envelopes, charges flow-budget counters, and runs the data-plane handshake (Noise IKpsk2 over QUIC in v1.0). The adapter wires the resulting secure channel back into the standard transport stack.

## Transport Strategy & NAT Traversal

Aura 1.0 uses a **relay-first with STUN assistance** strategy for NAT traversal. This achieves 95%+ connectivity without complex ICE coordination, TURN servers, or WebRTC.

### Supported Transports

1. **QUIC** (primary for desktop/mobile)
   - UDP-based, multiplexed streams, built-in encryption (TLS 1.3)
   - Best for direct peer-to-peer connections
   - Supports simple hole-punching via STUN

2. **WebSocket** (required for web browsers)
   - Runs over HTTP/HTTPS (firewall-friendly, port 80/443)
   - Required for WebAssembly/browser clients
   - Can relay through guardians/friends

3. **In-Memory** (testing only)
   - Deterministic transport for simulation and unit tests

### Connection Establishment Priority

When `establish_channel` is invoked, the following connection attempts are made in priority order:

1. **Direct QUIC** (if local network or one peer has public IP) - ~30% of cases
2. **QUIC via STUN reflexive address** (using NAT-mapped endpoint) - ~40-50% of cases  
3. **WebSocket relay** (via guardian/friend from social graph) - ~20-30% of cases

Each attempt has a 2-second timeout before falling back to the next method. The first successful connection is used.

### STUN Integration

STUN (Session Traversal Utilities for NAT) discovers the external IP:port mapping assigned by NATs:

- Devices query public STUN servers (e.g., `stun.l.google.com:19302`) on startup
- Reflexive address is cached with 5-minute TTL
- Included in `RendezvousDescriptor` as additional transport hint
- STUN failures are non-fatal (direct or relay still work)

### Simple Hole-Punching

For QUIC connections through NATs, a simple simultaneous open protocol is used:

1. Both peers include a `punch_nonce` in their offer/answer
2. Both peers simultaneously send small UDP packets to each other's reflexive address
3. Packets contain: `PREFIX || nonce || ephemeral_pub || mac`
4. NATs create bidirectional mappings when packets cross
5. QUIC handshake completes using the punched path

This handles cone NATs and port-restricted NATs without full ICE machinery. Symmetric NATs fall through to relay.

### Contact-Mediated Relay

When direct connections fail, guardians or friends can relay encrypted streams:

- **End-to-end encrypted**: Relay sees only ciphertext, cannot decrypt
- **Capability-gated**: Requires `RelayCapability` in guardian/friend caps
- **Flow-budget enforced**: Relay traffic counts against sender's flow budget
- **Selection heuristic**: Prefer guardians over friends

The relay protocol uses `RelayStream` messages in the SBB envelope system:
```
struct RelayStream {
    stream_id: Uuid,
    action: StreamAction,  // Open, Data, Close
    ciphertext: Vec<u8>,
}
```

### Out of Scope (Post-1.0)

Explicitly deferred to future releases:
- Full ICE/STUN/TURN coordination (trickle ICE, candidate prioritization)
- WebRTC transport (requires full ICE machinery)
- Raw TCP transport (QUIC is strictly better; WebSocket covers firewall-friendly fallback)
- Guardian-hosted TURN servers (optimization, not requirement)
- Onion routing / Tor integration
- BLE mesh / local discovery

## Data Structures

The journal records two structs per relationship:

```
struct RendezvousEnvelope {
    context: ContextId,     // RID or GID
    role: OfferOrAnswer,
    epoch: EpochId,
    counter: u32,
    flow_cost: u32,
    payload_cid: Cid,       // content addressed blob
    publisher: DeviceId,
    signature: Signature,   // device key
}

struct RendezvousDescriptor {
    context: ContextId,
    transport_hints: Vec<TransportHint>,  // Multiple hints in priority order
    punch_nonce: Option<[u8; 32]>,        // For hole-punching coordination
    handshake_psk: Uint256,               // derived from RID
    validity: Interval,
    issuer: DeviceId,
    signature: Signature,
}

enum TransportHint {
    QuicDirect { addr: SocketAddr },                  // Local endpoint
    QuicReflexive { addr: SocketAddr },               // STUN-discovered
    WebSocketRelay { guardian_id: GuardianId },       // Relay via guardian
    WebSocketDirect { url: String },                  // Direct WebSocket (browser)
}
```

Payload bytes live in the encrypted blob store and are addressed by `payload_cid`. The envelope header is cleartext so gossip can deduplicate, but the payload is encrypted with `K_box` derived from the pairwise RID. `flow_cost` expresses how much FlowBudget must be available before forwarding. The ledger entry `FlowBudget{spent,limit,epoch}` is keyed by `(context, neighbor_device)` and is updated exactly once per successful forward.

## Protocol Flow

1. **Offer publication**: Device A needs to reach account B. A derives the current `context = RID_AB`, increments its rendezvous counter using the existing counter choreography, and builds a `RendezvousDescriptor` with its reachable transports. A encrypts the descriptor with `K_box` and writes a new `RendezvousEnvelope{Offer}` fact plus the payload blob. Before the fact enters the journal, the manager charges `flow_cost` against every neighbor that will see the update. If any charge fails, publication is deferred until the budget replenishes.

2. **Replication**: Neighbors pull the updated journal, validate the envelope signature, and store the blob locally. Flow-budget receipts are appended so downstream relays can prove that the upstream hop already charged its ledger slot. This prevents budget laundering and keeps the accounting monotone.

3. **Answer publication**: Some device in account B decrypts the offer (it knows `K_box` because it belongs to RID_AB). B selects its preferred transport hint, encrypts a response descriptor with the same context key, and publishes a `RendezvousEnvelope{Answer}` fact. The flow-budget logic mirrors the offer path.

4. **Handshake**: When A receives the answer, it extracts the PSK and transport hint, then dials B via QUIC. Both sides run Noise IKpsk2 with the context PSK bound into the transcript, which produces the channel keys used by `TransportEffects`. The handshake result is stored as `ConnectionDescriptor{context, peer_device, channel_id, epoch}` so other subsystems can reuse the live path.

5. **Maintenance**: Devices prune envelopes outside the validity window but keep the CRDT histories to avoid conflicts. When a new device joins the account, the existing devices replay the most recent descriptors so the newcomer gains immediate reachability. When the trust graph changes, capability revocations remove the ability to gossip envelopes for that context.

## Interfaces

The Rendezvous Manager exposes two async functions:

```
async fn publish_descriptor(ctx: ContextId, transport: TransportHint) -> AuraResult<()>;
async fn establish_channel(account: AccountId) -> AuraResult<SecureChannel>;
```

`publish_descriptor` is used internally by device lifecycle code whenever a transport hint changes. `establish_channel` drives the full flow described above; it resolves the target account’s RID, ensures fresh envelopes exist, waits for an answer, and hands back a `SecureChannel` (thin wrapper over a QUIC connection plus metadata). Errors bubble up as structured `RendezvousError` variants: `Budget`, `Timeout`, `Auth`, `Transport`, `Permission`.

### SecureChannel Lifecycle

`SecureChannel` is a thin abstraction that higher-level protocols consume through `TransportEffects`. Its lifecycle is:

1. **Creation**: `establish_channel` dials the remote transport hint, runs Noise IKpsk2, and stores the negotiated QUIC connection plus metadata (`context`, `peer_device`, `epoch`, `channel_id`).
2. **Reuse**: The channel is registered with the account’s transport manager. Subsequent protocols (sync, recovery, storage) obtain the channel by context ID. Only one active channel per `(context, peer_device)` exists at a time.
3. **Refresh**: When the rendezvous descriptor expires or FlowBudget reservations change, the channel is gracefully torn down and `establish_channel` is rerun to refresh keys.
4. **Teardown**: On account logout or epoch rotation, `SecureChannel::close()` is invoked. This drains outstanding QUIC streams, releases FlowBudget reservations, and removes the entry from the transport manager.

This abstraction guarantees that every subsystem shares the same authenticated transport without reimplementing handshakes.

## Privacy and Spam Considerations

Version 1.0 leaks envelope headers (context, epoch, counter) to any neighbor authorized to replicate that relationship. This is acceptable for the initial release. Real payloads remain encrypted end-to-end, and the flooding process charges FlowBudget counters so abusive senders throttle themselves. The leakage budget annotations from `001` still apply: every envelope carries `[leak: (ℓ_ext, ℓ_ngh, ℓ_grp)]`, and the runtime records usage through `LeakageEffects`. Future privacy upgrades (onion routing, cover traffic, guardian caching) can sit on top of this ledger without changing the public interface.

## Evolution Path

The spec separates control-plane envelopes from transport handshakes, keeps all persistent state in the journal, and makes FlowBudget part of the core contract. That means later versions can add:

* alternative dissemination strategies (onion, gossip trees) by swapping the Neighbor Flooder,
* multi-account answers (group RIDs) by extending `RendezvousDescriptor`,
* cover traffic by emitting dummy envelopes that charge flow budgets,
* guardian-backed caching by introducing a new capability rather than rewriting the protocol.

Because every operation is already modeled as a monotone fact plus effect guard, these changes remain compatible with the calculus defined in the foundational docs.
