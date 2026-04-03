# Network Anonymity

This document defines Aura's network privacy and network anonymity model. It specifies the route-layer cryptographic objects, bootstrap re-entry records, hop processing rules, and reply-block semantics used by adaptive privacy routing.

This document complements [Transport and Information Flow](111_transport_and_information_flow.md), [Rendezvous](113_rendezvous.md), [Relational Contexts](114_relational_contexts.md), and [Social Architecture](115_social_architecture.md). Those documents define adjacent-peer transport, context-scoped discovery, relational trust facts, and social provisioning. This document defines the anonymous path layer that sits above those surfaces.

## 1. Scope

Aura provides two distinct network protection layers:

1. `Link encryption` protects one adjacent transport hop.
2. `Path encryption` protects the anonymous multi-hop route object carried across several adjacent hops.

Link encryption and path encryption solve different problems. Link encryption hides packet contents from the local network and from passive observers on one transport edge. Path encryption hides deeper route structure and reply-path structure from intermediate forwarding hops.

This document does not define application payload encryption. Application and context semantics remain protected by the existing Aura context and channel model.

## 2. Direct-Channel Baseline

Aura already uses adjacent-peer secure channels for direct transport. The current baseline is the Noise IKpsk2 25519 ChaChaPoly BLAKE2s pattern through `NoiseEffects` with the `snow` implementation.

The direct-channel baseline remains authoritative for adjacent-peer secure channels. Anonymous path routing does not replace that layer. Each adjacent hop on an anonymous route still runs over the existing link-protected channel model from [Transport and Information Flow](111_transport_and_information_flow.md).

## 3. Network Privacy Goals

Aura's route layer has the following goals:

- prevent an intermediate hop from reading deeper route state
- prevent an intermediate hop from deriving the final destination from its local peel result alone
- allow bounded stale-node re-entry without a singleton bootstrap service
- keep bootstrap hints signed, expiring, replay-bounded, and scope-limited
- keep reply paths typed and accountability-aware

Aura's route layer has the following non-goals:

- defeat a global passive observer
- create a globally enumerable neighborhood graph
- create a canonical shared Web-of-Trust topology map

## 4. Route-Layer Construction

Aura adopts a route-layer construction based on Curve25519, Aura's centralized KDF, and `ChaCha20-Poly1305`.

The route-layer construction uses the following rules:

1. Each anonymous path setup flow creates a fresh route identifier and fresh ephemeral route secret material.
2. Each hop derives a forward hop key stream and a backward hop key stream from the route secret material through Aura's centralized KDF.
3. Each hop encrypts or decrypts only its own layer with `ChaCha20-Poly1305`.
4. Each hop receives enough authenticated metadata to identify the next processing action, but not enough to reconstruct deeper route state.

Aura uses SURB-like reply blocks as Aura-native typed objects with explicit expiry, scope, and accountability semantics.

## 5. Link Encryption Versus Path Encryption

`Link encryption` uses the adjacent-peer secure channel. It protects one hop and one transport edge.

`Path encryption` uses the route-layer construction from section 4. It protects the route object carried inside the link-protected channel. Intermediate hops remove one route layer and learn only the immediate forwarding decision and the local accountability material for that hop.

The two layers must remain distinct:

- path-layer keys must not be reused as adjacent-peer channel keys
- adjacent-peer channel state must not be treated as route-hop state
- application or context semantic keys must not be treated as route-layer keys

## 6. Route-Layer Objects

The route layer uses the following typed objects:

- `BootstrapContactHint`
- `NeighborhoodReentryHint`
- bounded bootstrap introduction records
- `EstablishedPath`
- `MoveEnvelope`
- typed reply blocks

`BootstrapContactHint` records a remembered direct contact or prior provider that may help stale-node re-entry. It carries a scope, expiry, freshness data, and signed contact material. It does not represent canonical route truth.

`NeighborhoodReentryHint` records a board-published re-entry surface. It carries a neighborhood-scoped publication, expiry, replay bound, and signed route-surface material. It does not expose a globally enumerable topology map.

Bounded bootstrap introductions carry explicit introducer identity, introduced authority, scope, expiry, maximum remaining depth, and fan-out limits. They are trust and bootstrap evidence. They are not canonical shared route tiers.

`EstablishedPath` remains the reusable route object consumed by `Move`. `MoveEnvelope` remains the shared movement envelope family. Phase 7 replaces transparent peel visibility with encrypted peel processing without changing the accountable movement boundary.

## 7. Bootstrap and Re-Entry Surfaces

Aura supports stale-node re-entry through several overlapping bootstrap surfaces:

1. remembered direct contacts and prior providers
2. neighborhood discovery boards
3. bounded Web-of-Trust bootstrap introductions
4. rotating bootstrap relays or bridge providers

These surfaces are ordered inputs, not canonical shared route truth. Runtime selection may widen from one surface to the next when prior attempts fail or when freshness decays.

Aura explicitly rejects the following bootstrap designs:

- a singleton bootstrap authority
- a globally enumerable neighborhood adjacency map
- a canonical shared friends-of-friends graph

## 8. Neighborhood Discovery Boards

Neighborhood discovery boards publish signed, expiring, scope-limited re-entry hints. A board publication must include:

- the publishing authority
- the scoped neighborhood or re-entry domain
- an expiry time
- a replay-bounded publication identifier
- the advertised route-layer or move-surface public material

Board contents are advisory. Runtime caches may merge them. Runtime caches must not elevate them into canonical route truth. Runtime caches must not expose a stable global graph projection derived from board contents.

## 9. Bounded Bootstrap Introductions

Bootstrap introductions are Web-of-Trust evidence used for stale-node re-entry. Each introduction must include:

- introducer authority
- introduced authority
- scope
- expiry
- maximum remaining depth
- maximum fan-out

Introductions are valid only within their declared bounds. Runtime policy may consume them as discovery and permit input. Runtime policy must not publish a canonical shared introduction tier or transitive trust graph.

## 10. Hop Processing Rules

Every forwarding hop performs the following route-layer steps:

1. authenticate and decrypt the local hop layer
2. verify route identifier, hop position, expiry, and replay bound
3. derive the local forward or backward hop key stream
4. recover the next forwarding instruction or reply instruction
5. emit local accountability state and continue on the adjacent secure channel

A hop may learn:

- that it is on the route
- the previous hop on the adjacent edge
- the next hop on the adjacent edge
- local replay and expiry state

A hop may not learn:

- the full route
- deeper hop keys
- the final destination unless it is the exit hop
- the full reply path unless it is processing its own reply layer

## 11. Typed Reply Blocks

Aura uses typed reply blocks for backward anonymous delivery. A reply block is an Aura-native object with:

- scope
- route binding
- expiry
- replay bound
- backward hop material
- accountability linkage

Reply blocks are not borrowed Tor SURB packets. They are typed Aura objects that integrate with Aura movement, accountability, and retrieval-capability rotation rules.

Reply blocks must remain distinct from:

- application message payloads
- adjacent-peer secure channel state
- bootstrap trust records

## 12. Per-Boundary Leakage

Aura tracks privacy leakage by boundary. The route-layer design assumes leakage cannot be eliminated completely. The design instead constrains what each boundary can learn.

The main boundaries are:

- external observer of adjacent link traffic
- intermediate forwarding hop
- compromised subset of route hops
- stale-node bootstrap observer

An external observer may see timing, packet count, and adjacent link endpoints. An intermediate hop may see its adjacent predecessor and successor and its local peel result. A stale-node bootstrap observer may see limited board, introduction, or bridge use, but it must not recover a canonical shared topology map from that data alone.

## 13. Adversary Assumptions

Aura assumes the following adversary model:

- local passive observers exist
- some forwarding hops may be compromised
- bootstrap boards and bridge providers may be observed
- stale-node re-entry may happen after long offline gaps and physical movement

Aura does not assume a global passive adversary can be defeated. Aura does not assume that service relationships reveal nothing. Aura aims to reduce graph leakage and route leakage under partitioned socially rooted operation.

## 14. Construction Rationale

Aura keeps adjacent-peer Noise channels because they already fit the transport boundary and context model. Aura adds a route-layer construction because adjacent-peer channels alone do not hide deeper route structure from intermediate forwarding hops.

Aura chooses Curve25519, a centralized KDF surface, and `ChaCha20-Poly1305` because the construction is simple, auditable, and fits Aura's typed route-layer needs. The route layer needs explicit forward and backward hop streams, typed replay bounds, and typed reply blocks. A compact Aura-native construction is easier to align with these requirements than importing a foreign packet format.

## 15. Required Implementation Boundaries

The implementation must satisfy the following boundaries:

1. `aura-effects` owns hop crypto primitives
2. shared record types for bootstrap hints and re-entry hints remain authoritative typed objects
3. runtime-owned caches merge bootstrap records locally and expire them locally
4. `MoveEnvelope` remains the shared accountable movement boundary
5. `transparent_onion` remains a debug and simulation tool only

Production paths must use encrypted peel processing once Phase 7 lands. Transparent helper branches must not remain on production routing paths after the encrypted path is validated.
Transparent setup and header inspection objects remain quarantined behind the
explicit `transparent_onion` feature surface and must fail closed in release
production builds.

## 16. Summary

Aura uses existing Noise-based adjacent-peer channels for link encryption and a separate Aura-native route-layer construction for anonymous path encryption. Bootstrap and re-entry use overlapping signed and expiring surfaces instead of a singleton service. Reply blocks remain typed Aura objects. Runtime selection stays local and does not promote bootstrap provenance into canonical shared route truth.
