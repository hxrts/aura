# Network Anonymity

This document defines Aura's network privacy and network anonymity model. It specifies the route-layer cryptographic objects, bootstrap re-entry records, hop processing rules, and reply-block semantics used by adaptive privacy routing.

This document complements [Transport and Information Flow](111_transport_and_information_flow.md), [Rendezvous Architecture](113_rendezvous.md), [Relational Contexts](114_relational_contexts.md), and [Social Architecture](115_social_architecture.md). Those documents define adjacent-peer transport, context-scoped discovery, relational trust facts, and social provisioning. This document defines the anonymous path layer that sits above those surfaces.

## 1. Scope

Aura provides two distinct network protection layers. `Link encryption` protects one adjacent transport hop. `Path encryption` protects the anonymous multi-hop route object carried across several adjacent hops.

Link encryption and path encryption solve different problems. Link encryption hides packet contents from the local network and from passive observers on one transport edge. Path encryption hides deeper route structure and reply-path structure from intermediate forwarding hops.

This document does not define application payload encryption. Application and context semantics remain protected by the existing Aura context and channel model.

## 2. Direct-Channel Baseline

Aura already uses adjacent-peer secure channels for direct transport. The current baseline is the Noise IKpsk2 25519 ChaChaPoly BLAKE2s pattern through `NoiseEffects` with the `snow` implementation.

The direct-channel baseline remains authoritative for adjacent-peer secure channels. Anonymous path routing does not replace that layer. Each adjacent hop on an anonymous route still runs over the existing link-protected channel model from [Transport and Information Flow](111_transport_and_information_flow.md).

## 3. Network Privacy Goals

The route layer prevents an intermediate hop from reading deeper route state or deriving the final destination from its local peel result alone. It allows bounded stale-node re-entry without a singleton bootstrap service. Bootstrap hints stay signed, expiring, replay-bounded, and scope-limited. Reply paths remain typed and accountability-aware.

The route layer does not defeat a global passive observer. It does not create a globally enumerable neighborhood graph. It does not create a canonical shared Web-of-Trust topology map.

## 4. Route-Layer Construction

Aura adopts a route-layer construction based on Curve25519, Aura's KDF, and `ChaCha20-Poly1305`. Each anonymous path setup flow consumes manager-owned setup entropy before deriving the route identifier, replay-window identifier, or hop keys. The initiator derives one setup ephemeral route-layer public key for that setup. Each hop derives its setup peel key from its route-layer private key, the initiator setup ephemeral public key, the path id, the scope, its authority id, and its hop index under the `aura.route.setup.peel.v1` context.

Forward and backward path keys remain inside the encrypted hop-local setup payload. A hop receives those keys only after it proves it can peel its own setup layer with its private route key. Each hop encrypts or decrypts only its own layer with `ChaCha20-Poly1305`. Each hop receives enough authenticated metadata to identify the next processing action, but not enough to reconstruct deeper route state.

Aura uses SURB-like reply blocks as Aura-native typed objects with explicit expiry, scope, and accountability semantics.

## 4.1 Deployment Model

The service family model is always active. `Establish`, `Move`, and `Hold` are the normal service vocabulary for path creation, opaque movement, and custody.

`LocalRoutingProfile::passthrough()` is the pre-privacy routing baseline. It uses mixing depth `0`, delay `0`, cover rate `0`, and path diversity `1`. `Hold` remains available under passthrough because it is an availability service, not a routing-profile knob.

Production privacy uses encrypted path setup and encrypted `MoveEnvelope` processing with one fixed adaptive policy that users do not tune. Development and simulation may sweep policy constants, while production nodes use the evidence-backed constants shipped with the build. The current fixed policy uses path-diversity floor `2`, cover floor `2` packets per second, delay gain denominator `3`, neighborhood hold retention window `120s`, and retrieval-capability rotation beginning `10s` before expiry. Organic application and sync retrieval bytes are converted into canonical cover-packet units before the runtime computes the synthetic-cover gap.

## 5. Link Encryption Versus Path Encryption

`Link encryption` uses the adjacent-peer secure channel. It protects one hop and one transport edge.

`Path encryption` uses the route-layer construction from section 4. It protects the route object carried inside the link-protected channel. Intermediate hops remove one route layer and learn only the immediate forwarding decision and the local accountability material for that hop.

The two layers must remain distinct. Path-layer keys must not be reused as adjacent-peer channel keys. Adjacent-peer channel state must not be treated as route-hop state. Application or context semantic keys must not be treated as route-layer keys.

## 6. Route-Layer Objects

### 6.1 Bootstrap records

`BootstrapContactHint` records a remembered direct contact or prior provider that may help stale-node re-entry. It carries scope, expiry, freshness data, and signed contact material. It does not represent canonical route truth.

`NeighborhoodReentryHint` records a board-published re-entry surface. It carries a neighborhood-scoped publication, expiry, replay bound, and signed route-surface material. It does not expose a globally enumerable topology map.

`BootstrapIntroductionHint` carries introducer authority, introduced authority, scope, expiry, replay bound, maximum remaining depth, and maximum fan-out. It is trust and bootstrap evidence rather than a canonical shared route tier.

### 6.2 Path and movement objects

`EstablishedPath` is the reusable route object consumed by `Move`. `MoveEnvelope` is the shared movement envelope family. Encrypted setup objects carry the setup ephemeral public key needed for hop-local setup peel derivation. Encrypted peel processing is the production route-layer behavior and preserves the accountable movement boundary.

Typed reply blocks provide backward anonymous delivery. Their semantics are defined in section 11.

## 7. Bootstrap and Re-Entry Surfaces

Aura supports stale-node re-entry through several overlapping bootstrap surfaces. These include remembered direct contacts and prior providers, neighborhood discovery boards, bounded Web-of-Trust bootstrap introductions, and rotating bootstrap relays or bridge providers. These surfaces are ordered inputs, not canonical shared route truth. Runtime selection may widen from one surface to the next when prior attempts fail or when freshness decays.

Aura explicitly rejects bootstrap designs that rely on a singleton bootstrap authority, a globally enumerable neighborhood adjacency map, or a canonical shared friends-of-friends graph.

## 8. Neighborhood Discovery Boards

Neighborhood discovery boards publish signed, expiring, scope-limited re-entry hints using the `NeighborhoodReentryHint` record. A publication carries the publishing authority, the scoped neighborhood or re-entry domain, an expiry time, a replay-bounded publication identifier, and the advertised route-layer or move-surface public material.

Board contents are advisory. Runtime caches may merge them. Runtime caches must not elevate them into canonical route truth. Runtime caches must not expose a stable global graph projection derived from board contents.

## 9. Bounded Bootstrap Introductions

Bootstrap introductions are Web-of-Trust evidence used for stale-node re-entry. Each introduction must include introducer authority, introduced authority, scope, expiry, maximum remaining depth, and maximum fan-out.

Introductions are valid only within their declared bounds. Runtime policy may consume them as discovery and permit input. Runtime policy must not publish a canonical shared introduction tier or transitive trust graph.

## 10. Hop Processing Rules

Every forwarding hop performs the following route-layer steps:

1. derive the setup peel key from the setup ephemeral public key and the hop route-layer private key
2. authenticate and decrypt the local hop layer
3. verify route identifier, hop position, expiry, and replay bound
4. recover the local forward or backward hop key stream
5. recover the next forwarding instruction or reply instruction
6. emit local accountability state and continue on the adjacent secure channel

A hop may learn that it is on the route, the previous and next hops on the adjacent edges, and its local replay and expiry state. A hop may not learn the full route, deeper hop keys, the final destination unless it is the exit hop, or the full reply path unless it is processing its own reply layer.

## 11. Typed Reply Blocks

Aura uses typed reply blocks for backward anonymous delivery. The canonical record is `AccountabilityReplyBlock` and carries scope, expiry, nonce, an opaque single-use token, and a command-scoped accountability binding. Route binding is verifier-local state associated with the reply-block record and the submitted `EstablishedPathRef`. Backward hop material stays on the companion `EstablishedPath` and is not embedded in the reply block.

The opaque token is bound to path id, scope, reply-block kind, command scope, expiry, nonce, and a local token secret. Verification rejects cross-path use, cross-kind use, forged tokens built only from visible fields, and replay after the first valid witness submission. This keeps the wire-visible reply block from gaining a stable route-correlation field.

Each service family wraps the canonical record in a typed form. `MoveReceiptReplyBlock`, `HoldDepositReplyBlock`, `HoldRetrievalReplyBlock`, and `HoldAuditReplyBlock` integrate with Aura movement, accountability, and retrieval-capability rotation rules. Reply blocks are not borrowed Tor SURB packets and must remain distinct from application message payloads, adjacent-peer secure channel state, and bootstrap trust records.

## 11.1 Movement Scheduling

The runtime schedules protected movement through shared classes rather than separate transport families. Sync-blended traffic may wait for anti-entropy windows. Bounded-deadline replies carry accountability or control traffic with shorter deadlines. Synthetic cover fills the remaining cover floor.

Application traffic and sync-blended retrieval reduce the synthetic-cover gap after conversion into canonical cover-packet units. Accountability replies are measured separately in the current deployment model. They do not reduce the first-deployment synthetic cover floor.

## 12. Per-Boundary Leakage

Aura tracks privacy leakage by boundary. The route-layer design assumes leakage cannot be eliminated completely. The design instead constrains what each boundary can learn.

The main boundaries are an external observer of adjacent link traffic, an intermediate forwarding hop, a compromised subset of route hops, and a stale-node bootstrap observer.

An external observer may see timing, packet count, and adjacent link endpoints. An intermediate hop may see its adjacent predecessor and successor and its local peel result. A stale-node bootstrap observer may see limited board, introduction, or bridge use. It must not recover a canonical shared topology map from that data alone.

## 13. Adversary Assumptions

Aura assumes an adversary model in which local passive observers exist, some forwarding hops may be compromised, bootstrap boards and bridge providers may be observed, and stale-node re-entry may happen after long offline gaps and physical movement.

Aura does not assume a global passive adversary can be defeated. Aura does not assume that service relationships reveal nothing. Aura aims to reduce graph leakage and route leakage under partitioned socially rooted operation.

## 14. Construction Rationale

Aura keeps adjacent-peer Noise channels because they already fit the transport boundary and context model. Aura adds a route-layer construction because adjacent-peer channels alone do not hide deeper route structure from intermediate forwarding hops.

Aura chooses Curve25519, a centralized KDF surface, and `ChaCha20-Poly1305` because the construction is simple, auditable, and fits Aura's typed route-layer needs. The route layer needs explicit forward and backward hop streams, typed replay bounds, and typed reply blocks. A compact Aura-native construction is easier to align with these requirements than importing a foreign packet format.

## 15. Required Implementation Boundaries

The implementation must satisfy several boundaries. `aura-effects` owns hop crypto primitives, route-layer public key derivation, and setup key agreement. Shared record types for bootstrap hints and re-entry hints remain authoritative typed objects. Runtime-owned caches merge bootstrap records locally and expire them locally.

`AnonymousPathManager` owns setup entropy consumption, setup ephemeral public key publication, encrypted setup object construction, and anonymous established-path lifecycle. `HoldManager` owns verifier-local reply-block path binding and single-use token state. These runtime-owned services may expose typed records, but they must not push route-secret derivation or token verification into rendezvous descriptors or application payloads.

`MoveEnvelope` remains the shared accountable movement boundary. `transparent_onion` remains a debug and simulation tool only. Production paths use encrypted peel processing. Transparent setup and header inspection objects remain quarantined behind the explicit `transparent_onion` feature surface and must fail closed in release production builds.

## 16. Summary

Aura uses existing Noise-based adjacent-peer channels for link encryption and a separate Aura-native route-layer construction for anonymous path encryption. Bootstrap and re-entry use overlapping signed and expiring surfaces instead of a singleton service. Reply blocks remain typed Aura objects with verifier-local path binding and single-use keyed tokens. Runtime selection stays local and does not promote bootstrap provenance into canonical shared route truth.
