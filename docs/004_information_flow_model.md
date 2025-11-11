# Privacy and Information Flow Model

Aura's model is fundamentally consent-based and articulated as a unified information-flow system. Privacy boundaries align with social relationships rather than technical perimeters. Violations occur when information crosses trust boundaries without consent. Acceptable flows consume explicitly budgeted headroom.

This document specifies Aura's privacy and information-flow model as one regulatory system covering privacy leakage and spam together. It defines what information is visible to different observers, how properties degrade under attack, and how enforcement works via a monotone semilattice-based budget. Per-context per-peer flow headroom is charged before any transport side effect. See [Information Flow Budget](103_flow_budget_system.md) for the full budget specification.

## Privacy Philosophy

Traditional privacy systems treat all information disclosure as a privacy violation, forcing users to choose between complete isolation and complete exposure. Aura recognizes that privacy is relational. Sharing information with trusted parties is not a privacy violation. It is the foundation of meaningful collaboration.

The system implements three core principles. Consensual Disclosure means when you join a group or establish a relationship, you consent to sharing certain information with those parties. This is not a privacy leak. It is the basis for coordination. A group seeing that you are active, a friend knowing you are available, a collaborator accessing shared documents are features, not bugs.

Contextual Identity means through Deterministic Key Derivation you present different identities in different contexts. Only parties within a relationship can link these identities. This prevents global identity correlation. Neighborhood Visibility means in a peer-to-peer system immediate gossip neighbors observe some metadata in encrypted envelopes. This is acceptable because they cannot decrypt content, context isolation prevents linking across contexts, and an information-flow budget hard-limits observable rate and volume per context so metadata exposure remains bounded. Optional cover mechanisms can further reduce inference but are not required for 1.0.

## Unified Information-Flow Budget

Aura enforces privacy leakage limits and spam resistance with the same monotone mechanism. For each context and peer pair, the journal stores a budget fact:

```
FlowBudget { limit: u64, spent: u64, epoch: Epoch }
```

The `spent` field is join-monotone with merge of max. The `limit` field is meet-monotone with merge of min. Epochs gate replenishment. Before any transport effect, `FlowGuard` charges `cost` to the context and peer pair. If `spent` plus `cost` exceeds `limit`, the send is blocked locally and no packet is emitted with no observable without charge.

For multi-hop forwarding, relays validate a signed per-hop receipt from the previous hop, then charge their own budget before forwarding. Limits update deterministically via journal facts using web-of-trust inputs. All replicas converge on the same values.

### Budget Formulas

Let `limit(ctx, peer)` compute as the meet over deterministic components:

```
limit(ctx, peer) = base(ctx) ⊓ policy(ctx) ⊓ role(ctx, peer)
                   ⊓ relay_factor(ctx) ⊓ device_health(peer)
```

The `base(ctx)` is context class default like relationship group or rendezvous. The `policy(ctx)` is local sovereign policy set by the account which can only shrink. The `role(ctx, peer)` is per-peer role within context like owner member or guest. The `relay_factor(ctx)` reduces limit for high-degree hubs to mitigate amplification via meet-only. The `device_health(peer)` shrinks when peer is marked unhealthy or overloaded.

Each term is encoded as a lattice element with a natural order and merges via meet. This ensures convergence and prevents widening through reorganizations.

### Receipts and Epochs

Per-hop receipts are required for forwarding and are bound to the epoch:

```rust
Receipt {
  ctx: ContextId,
  src: DeviceId,
  dst: DeviceId,
  epoch: Epoch,
  cost: u32,
  nonce: u64,
  prev: Hash32,
  sig: Signature,
}
```

The acceptance window is current epoch only. Rotation triggers when journal fact `Epoch(ctx)` increments due to time-based policy or administrative event. Rotations are monotone and converge by meet on the max epoch. On rotation, `spent(ctx, *)` resets and receipts cannot be reused across epochs.

### Fairness and Liveness

At most one outstanding reservation exists per context and device. Provide a minimum headroom floor for active relationships to prevent starvation under hub throttling. Optional jitter on sensitive protocols like guardian recovery and rendezvous reduces timing correlation without violating monotonicity.

This unifies who may talk, how often, and with how much metadata leakage under the same laws as capabilities and facts. See [Information Flow Budget](103_flow_budget_system.md) for implementation details and [Capability System](202_capability_system.md) for capability-based access control.

## Privacy Boundaries

Aura defines four distinct privacy boundaries each with different visibility properties and budget dimensions.

### Relationship Boundary

When you establish a direct relationship with another device or join a group, you cross into the relationship boundary. Within this boundary both parties have consented to share information necessary for coordination.

Participants can see your context-specific identity, your online or offline status, your participation in shared activities, and the content of messages you send. They can observe when you are active in the shared context and what actions you take. This visibility is by design enabling collaboration.

Even within a relationship, participants cannot link your identity across different contexts unless you explicitly share that information. One social identity can be isolated from another. Your work persona does not leak into your personal life. DKD ensures that each relationship gets a unique identity derived from account key, app ID, and context label. Even if two parties know you in different contexts, they cannot cryptographically link these identities without your cooperation.

Each context and peer pair maintains a flow budget in the journal. Every send charges `spent` before any transport effect. If `spent` plus `cost` exceeds `limit`, the send is blocked locally and no packet is emitted. This bounds metadata exposure for the relationship and prevents spam by construction.

### Neighborhood Boundary

Your neighborhood consists of devices that forward gossip traffic for you. These are either direct relationships or transitive connections through the web of trust. They see encrypted envelopes but cannot decrypt them.

Neighbors observe that encrypted traffic passes through them. They see envelope sizes which are fixed, rtag identifiers which are rotating, and timing patterns. With sustained observation they might infer that certain rtags receive frequent traffic suggesting an active relationship. They can count the volume of traffic associated with particular rtags.

Neighbors cannot decrypt envelope content, determine the ultimate sender or receiver, or link rtags to real identities. Onion routing ensures that each hop only knows the previous and next hop, not the entire path. Rotating rtags prevent long-term tracking of a single relationship.

The primary attack at this boundary is traffic analysis. A sophisticated adversary controlling multiple gossip hops might correlate timing patterns across hops to infer communication patterns. However this requires controlling multiple strategic positions, sustained observation over time, and overcoming batching and budget constraints that limit observable rate.

Forwarding uses per-hop receipts. A relay only forwards after validating the upstream receipt and charging its own context and next hop budget. Otherwise it drops locally. Receipts and counters are scoped to the same context, so budget state does not leak to unrelated observers.

### Group Boundary

Group participants see group-scoped identities, content, and threshold operations. Leakage accounting uses the group dimension and is constrained by `limit(ctx).group` where applicable.

### External Boundary

External observers have no relationship with you and are not part of your gossip neighborhood. In Aura's threat model, external observers should learn nothing.

With Tor integration, external observers see only encrypted Tor traffic with no distinguishing characteristics. Without Tor, an ISP-level adversary might observe that you are connecting to known Aura nodes but learns nothing about the content, participants, or structure of communications. Information-flow budgets reduce total emission opportunities and bound observable rate further limiting inference.

External observers cannot determine which accounts exist, which devices belong to which accounts, what relationships exist, what groups you participate in, what content you access, or even that you are using Aura with Tor. The system provides anonymity in the technical sense where external observers cannot distinguish Aura users from non-users.

Tor provides transport-level anonymity breaking the link between IP addresses and Aura identities. Fixed-size envelopes prevent size-based fingerprinting. The lack of a central directory or discovery service means there is no global metadata to leak.

## Privacy Layers

Aura's privacy model operates across five distinct layers each with specific properties and threat models.

### Identity Privacy

Traditional systems leak identity through persistent identifiers. Even with pseudonyms, global identifiers allow correlation across contexts. Aura eliminates this through context-specific identities.

An identity used in context A cannot be linked to an identity used in context B without explicit key material or cooperation from the account holder. DKD derives context-specific keys using `derive(account_root, app_id, context_label)`. The derivation is deterministic but irreversible knowing the derived key for context A reveals nothing about context B. Each app gets its own identity namespace. Within each app different contexts get different identities.

An adversary who observes you in multiple contexts and suspects they are the same account cannot prove this cryptographically. The only attack vector is side-channel correlation. If you exhibit the same writing style, posting patterns, or behavioral fingerprints across contexts, a sophisticated adversary might probabilistically link them. This is fundamentally a social attack, not a technical one.

The testing metric `identity_linkability_score` measures the probability that an observer can link identities across contexts. Score should remain below 0.05 (5% confidence) for any pair of contexts without collusion.

### Relationship Privacy

Your social graph is sensitive information. Who you communicate with reveals personal relationships, organizational structures, and social patterns. Aura keeps the relationship graph opaque to external observers while allowing consensual visibility within relationships.

The existence of a relationship between two identities is only visible to the parties in the relationship and to parties who both parties have explicitly disclosed the relationship to like by both being in a shared group. Relationships are established through out-of-band key exchange or web-of-trust introductions. There is no central directory. Relationship establishment happens through encrypted envelopes with ephemeral rendezvous points only discoverable by parties who know the pre-shared secret.

An external observer monitoring network traffic cannot determine that Alice and Bob have a relationship. They might observe that Alice's gossip neighborhood overlaps with Bob's if they are neighbors in the web of trust but cannot prove direct communication. A neighborhood adversary who controls gossip hops might correlate timing patterns but cover traffic and batching degrade this attack.

Groups create consensual disclosure zones. If Alice, Bob, and Carol are all in group G then each member knows about the others' membership. This is not a privacy leak. It is the definition of a group. However an external observer or neighborhood adversary cannot enumerate group membership without being invited.

The testing metric `relationship_inference_confidence` measures probability that an observer can infer the existence of a relationship. Should remain below 0.10 for neighborhood adversaries and 0.01 for external observers.

### Group Privacy

Groups are micro-anonymity sets. Within a group members are visible to each other through consensual disclosure. Outside the group membership is hidden. The group itself operates as a threshold entity with its own group identity.

Group membership is only visible within the group. External observers cannot enumerate members, determine group size, or even confirm a group's existence. Group communications are indistinguishable from person-to-person communications at the envelope level.

Groups have their own DKD-derived identity independent of member identities. Members hold shares of the group's threshold key. Group envelopes are encrypted to the group's public key making them indistinguishable from person-to-person envelopes. The rtag for group traffic rotates independently.

For privacy-sensitive operations groups can operate in anonymity mode where actions are attributed to the group rather than individuals. A document edit shows group G modified this rather than Alice modified this. This requires threshold signatures where k members approve but individual signers remain anonymous.

The primary attack is membership enumeration through traffic analysis. If an adversary observes that Alice, Bob, and Carol all receive messages shortly after a group event they might infer shared membership. Defenses include randomized delivery delays, cover traffic to all potential members, and batching deliveries to obscure correlation.

The testing metric `group_membership_inference` measures probability of inferring membership through traffic analysis. Should remain below k-anonymity bound where P(specific member | observation) is at most 1/k where k is group size.

### Content Privacy

All content is end-to-end encrypted. Only parties with the appropriate key material can decrypt. This is table stakes for any privacy system.

Message content, stored documents, and application data are encrypted such that only authorized parties can decrypt. Authorized parties include direct recipients, group members, or capability holders. Messages use authenticated encryption with AES-GCM with per-message keys derived from shared secrets. Stored content uses content-addressed encryption where the encryption key is derived from the content hash plus a secret salt known only to authorized parties. Capabilities grant access by providing the decryption key. For details on capability-based access control, see [Capability System](202_capability_system.md).

Session keys are ephemeral. Compromise of long-term identity keys does not compromise past session content. Aura uses the Double Ratchet pattern for person-to-person messaging and threshold ratcheting for group communications.

The only content-level attack is brute force cryptanalysis or compromise of key material. Aura uses well-studied primitives like AES-256-GCM, Ed25519, and HPKE that provide 128-bit security. We assume adversaries cannot break these primitives or factor large numbers.

Content privacy is binary. Either the adversary has key material or they do not. Testing focuses on key management where `key_exposure_rate` measures how often keys leak through side channels like logs, crash dumps, or memory dumps.

### Metadata Privacy

Content privacy is useless if metadata leaks communication patterns. Metadata includes timing when messages are sent, frequency how often you communicate, size how much data, and participation who is active in a group.

Metadata visible to different adversaries should be bounded. Within relationships metadata visibility is consensual. At the neighborhood boundary metadata should be noisy enough to prevent precise inference. For external observers metadata should be completely opaque.

Aura employs multiple metadata privacy techniques with the information-flow budget as the foundation. Per-context per-peer budgets bound observable rate and volume. All sends charge headroom before any packet is emitted. Exhausted budgets block locally preventing further observables in that context.

All envelopes are padded to a standard size like 16KB. Small messages waste bandwidth but large messages are chunked. This prevents size-based fingerprinting. Outgoing messages are batched and sent at fixed intervals rather than immediately. This obscures the precise timing of individual messages and reduces traffic correlation opportunities.

Dummy envelopes are indistinguishable from real envelopes when cover traffic is implemented. When implemented cover adapts to usage and is layered on top of budget enforcement. It never bypasses charge-before-send. Envelope routing tags rotate periodically. A long-lived conversation does not use the same rtag forever preventing longitudinal tracking. Rotation happens on a schedule negotiated within the relationship.

Envelopes are onion-routed through multiple hops. Each hop sees only the previous and next hop not the full path. This prevents single points of observation from learning complete paths.

Metadata privacy degrades gracefully under increasingly powerful adversaries. A passive neighborhood observer might infer that someone is communicating frequently but cannot identify specific parties. An active adversary who controls multiple gossip hops and correlates timing across hops has better inference capability but faces computational challenges at scale. A global passive adversary might perform traffic analysis across the entire network but faces the challenge of distinguishing real traffic from cover traffic and dealing with onion routing's path obfuscation.

The testing metric `metadata_leakage` measures information leakage in bits. For timing H(actual_send_time | observed_traffic) should be high above 4 bits. For participation P(user active | observed_neighborhood_traffic) should be close to base rate within 10%.

## Implementation Metrics

Aura 1.0 ties privacy guarantees to the FlowBudget system described in [Information Flow Budget](103_flow_budget_system.md). Each protocol must meet measurable targets.

Per-context activity requires `spent(ctx)` not exceed `limit(ctx)` per epoch enforced by FlowGuard charge before every transport call. Leakage per observer class requires `L(τ, class)` not exceed `Budget(class)` enforced by `LeakageEffects::record_leakage` guard. Timing dispersion requires high-sensitivity protocols like guardian recovery and rendezvous add at least 2 seconds jitter to observable sends via optional delay guard in choreography. Reservation fairness requires at most one outstanding FlowBudget reservation per context and device enforced by device allocator.

Future releases may add cover traffic or adaptive flow costs but this baseline keeps the threat model quantitative.

## Threat Model by Adversary Position

Privacy properties vary based on adversary capabilities and position in the network. Aura's threat model explicitly considers five adversary types.

### Individual Relationship Partner

An individual who has a direct relationship with you sees everything within that relationship context. You share your context-specific identity, the content you share, your activity patterns within that relationship. This is consensual disclosure. They cannot see your activity in other contexts, link your identity across contexts, or determine your other relationships unless you explicitly share that information. They cannot access content or capabilities you have not granted them.

The attack here is social engineering or context correlation. If you accidentally reveal cross-context information like mentioning something from context A while in context B, they might infer the link. Technical mechanisms cannot prevent social-layer correlation. User education and interface design mitigate this. The UI clearly indicates which context you operate in. Different contexts use different visual themes. The system prompts before sharing information that might leak cross-context details.

### Group Insider

A group member sees all group activity including member identities within the group context, group content, participation patterns, and threshold operations. They can observe who is active, who proposes changes, and who approves operations. They cannot determine members' identities outside the group context, access other groups those members participate in, or see person-to-person relationships between members not disclosed within the group.

Group members might collude to link identities across contexts if multiple members recognize the same behavioral patterns. A malicious group member might attempt to deanonymize threshold signatures by analyzing signing patterns or timing. Groups enforce k-anonymity for sensitive operations. When anonymity is required at least k members participate in threshold signing and individual signers are not revealed. Signing rounds include random delays to prevent timing correlation.

### Gossip Neighborhood Participant

Devices in your gossip neighborhood forward encrypted envelopes. They observe envelope metadata like size which is fixed, rtag which is rotating, timing, and frequency. With sustained observation they might infer traffic patterns like rtag X receives frequent messages or this device is very active. They cannot decrypt envelope content, determine ultimate sender or receiver due to onion routing, link rtags to real identities, or distinguish real traffic from cover traffic without statistical analysis over long periods.

The primary attack is traffic correlation. If an adversary controls multiple hops in an onion route they might correlate timing patterns to infer communication. If they observe your neighborhood over weeks they might statistically filter out cover traffic and identify real communication patterns. Onion routing prevents single-hop correlation. Cover traffic obscures activity patterns. The system maintains steady traffic rates even during idle periods. Batching and random delays break timing correlation. Rtag rotation limits tracking duration. The effectiveness of these defenses is quantitatively measured using the probabilistic observer model.

### Network-Level Observer

An adversary who can observe network traffic like an ISP or nation-state sees IP-level connections and packet timing. Without Tor they can observe that you are connecting to known Aura nodes. With Tor they see only that you are using Tor. They cannot decrypt envelope content, determine which Aura accounts you control, see the structure of relationships or groups, or distinguish Aura traffic from other encrypted traffic when using Tor.

Without Tor confirmation attacks are possible. If they suspect you are account X they can correlate your IP connectivity with account X's activity. With Tor correlation requires breaking Tor's anonymity properties which is a hard problem. Even with Tor compromised fixed-size envelopes and cover traffic prevent precise correlation. Tor integration eliminates IP-level correlation. All Aura network traffic should route through Tor in high-security mode. The system warns users if Tor is unavailable. For lower-security scenarios like trusted organizational networks Tor can be disabled but users are informed of the implications.

### Compromised Device

If an adversary compromises one device in a multi-device account they gain access to that device's key share, the account's journal state as synced to that device, any cached content, and the device's view of relationships and groups. A single compromised device cannot perform threshold operations alone which requires M of N devices, derive the account's root key since threshold secret sharing applies, access content encrypted to other contexts not synced to that device, or impersonate the account without threshold signing from other devices.

The compromised device can observe all activity visible to that device including journal updates from other devices. It can attempt to exfiltrate its key share. If the adversary compromises M of N devices they gain full account control. Threshold cryptography limits single-device compromise impact. Users can revoke compromised devices through resharing which requires M devices so assumes at least one honest device. Session epochs invalidate old credentials when compromise is detected. Platform-specific secure storage like Keychain or Secret Service protects key shares from trivial extraction. In the future secure enclaves like iOS Secure Enclave or TEE provide hardware-backed key protection.

## Hub Node Problem and Mitigation

In any web-of-trust gossip network some nodes naturally become hubs with high connectivity that forward traffic for many peers. Hub nodes are valuable for network efficiency but create privacy risks.

A hub node observes a large fraction of network traffic. While they cannot decrypt content they see envelope metadata for many relationships. With statistical analysis a hub might infer the network's communication graph structure, identify highly active accounts, or correlate timing patterns across multiple relationships.

Aura employs several techniques to limit hub node privacy impact. Routes are selected to minimize the fraction of a path observed by any single node. If node H is a hub routes avoid using H for consecutive hops. The routing algorithm optimizes for low-overlap paths. If one path uses H alternative paths avoid H where possible.

The system tracks node connectivity and identifies potential hubs. Users can choose to avoid routing through suspected hubs if privacy is a higher priority than efficiency. The routing algorithm accepts a privacy parameter. Higher privacy settings avoid hubs even at the cost of longer routes or higher latency.

To prevent hubs from distinguishing real traffic from cover traffic the system sends decoy envelopes through hubs at a steady rate. From the hub's perspective it is always forwarding traffic so the presence or absence of real traffic is obscured.

Rather than allowing single nodes to become mega-hubs the system encourages distributed hub roles. Multiple well-connected nodes share the forwarding load. Routing algorithms prefer medium-connectivity nodes over extremely high-connectivity nodes when privacy matters.

Even as a hub there are limits to what can be observed. Fixed-size envelopes prevent size correlation. Rotating rtags limit tracking duration. Batching and delays break timing correlation. Cover traffic adds noise. A hub might observe lots of traffic but extracting specific communication patterns requires sustained observation and statistical inference which testing frameworks measure and bound.

Per-hop budgets and receipt validation bound the rate at which any hub can forward traffic for a given context. When limits tighten via meet, forwarding stops locally reducing the volume of metadata a hub can observe and preventing spam amplification.

## Cover Traffic Strategy

Cover traffic can strengthen metadata privacy but is expensive if done naively. For Aura 1.0, automatic cover traffic is out of scope. The unified information-flow budget provides the primary privacy and spam enforcement. The strategy below describes a future layer that can be added without changing the underlying ledger or calculus.

Cover traffic adapts to real usage patterns. If you typically send 20 messages per hour during work hours the system maintains approximately that rate during idle periods. This prevents an adversary from distinguishing active from idle states by observing traffic rate changes.

Rather than sending individual cover messages the system leverages groups for efficient cover traffic. Groups naturally have steady traffic rates due to multiple members. By participating in groups your individual traffic pattern is obscured within the group's aggregate pattern. A group of 10 members sending aggregate 100 messages per hour provides good cover for individual members sending 5 to 20 messages per hour.

Cover traffic sends at scheduled intervals rather than random times. This creates a predictable background rate that adversaries must filter out. Real messages are inserted into scheduled traffic slots making them indistinguishable from cover traffic. Cover traffic envelopes are cryptographically indistinguishable from real envelopes. They have proper rtags, are encrypted, and follow the same routing as real messages. Only the recipient can determine that an envelope is cover traffic by attempting to decrypt and receiving a specific dummy marker.

Cover traffic is expensive. To minimize cost the system uses several optimizations. Groups amortize cover traffic across members. Cover rate scales with actual usage. Active users have more cover traffic than occasional users. Low-priority cover traffic can be batched and delayed. Users can tune the cover traffic and privacy tradeoff based on their threat model.

Effective cover traffic keeps the probability of distinguishing real from dummy traffic close to 0.5. An observer cannot distinguish real from dummy traffic better than random guessing.

## Integration with RFC 130 Testing Framework

This specification defines what privacy properties Aura should provide. RFC 130 defines how to test whether those properties hold in practice.

Each privacy or flow property like identity unlinkability or relationship privacy translates to a test invariant. For example identity unlinkability across contexts becomes `assert!(observer.identity_linkability_score(context_a, context_b) < 0.05)`. Budget exhaustion emitting no observable becomes `assert!(!observer.saw_packet_after_budget_exhaustion())`.

Each adversary type like neighborhood participant or network observer is implemented as a test observer with specific capabilities. Tests instantiate observers with varying capabilities and measure what they can infer.

Privacy properties are probabilistic not absolute. Tests measure inference confidence and verify it remains below specified bounds. For example a neighborhood observer's confidence in inferring a relationship should stay below 10%. Flow properties are deterministic at the point of enforcement. Charging must succeed before any send. When charging fails no packet may be emitted.

Tests run in the simulation framework with deterministic time and network conditions. This allows reproducible measurement of privacy properties and flow enforcement under controlled adversarial scenarios. The simulation maintains a test-only ground truth oracle that knows all relationships, identities, and communications. This oracle is strictly isolated from adversary observers and exists only for measuring what adversaries can infer versus what actually happened.

Privacy tests are part of CI. Any change that degrades privacy properties like increases inference confidence beyond thresholds fails the test. This prevents accidental privacy regressions.

## Implementation Guidance

Implementing Aura's privacy model requires careful attention across the entire stack.

Key derivation must be cryptographically sound. Use HKDF with domain separation. Never reuse keys across contexts. The derivation path should include account root key which is secret, app_id which is public, context_label which is public, and a domain separator like aura.key.derive.v1. Test that keys derived for different contexts are uncorrelated.

Envelopes must be fixed-size, encrypted, authenticated, and onion-routed. Padding must be non-distinguishable using random bytes not zeros. Rtags must rotate on a schedule negotiated in-band. Onion routing must select diverse paths. Test that envelopes are indistinguishable at the byte level.

All transport calls must pass through a `FlowGuard` that charges the per-context per-peer budget before any network side effect. When charging fails the choreography must branch locally and emit no packet. For multi-hop forwarding attach and validate per-hop receipts. See [Information Flow Budget](103_flow_budget_system.md) for the data model and algorithms.

When implemented, cover traffic should live at the envelope layer not the application layer. Dummy envelopes must be cryptographically indistinguishable and injected into scheduled slots. Recipients should silently drop dummy envelopes. Treat cover as additive. It must never bypass budget charging.

Nodes forward envelopes they cannot decrypt. Forwarding must be prompt to prevent timing correlation based on delays but batched to prevent size-based correlation from small versus large batches. Nodes should not log or store forwarded envelope metadata beyond what is needed for routing like rtag and next hop.

The CRDT journal contains sensitive metadata about account structure. Journal events should be encrypted when synced across untrusted paths. Within a relationship or group journal events can be plaintext through consensual disclosure. When syncing through a relay or storage provider encrypt the journal with a key known only to account devices.

Capabilities grant access to resources. When implementing capabilities ensure they do not leak information to unauthorized parties. A capability should be opaque. Possessing a capability grants access but the capability itself does not reveal what resource it grants access to or who issued it. Use capabilities as bearer tokens with authenticated encryption.

All network traffic should route through Tor by default. Provide a clear UI indication when Tor is disabled. Consider implementing a high-security mode that enforces Tor and refuses to operate without it. Test that Tor integration does not leak IP addresses through DNS queries or other side channels.

Use platform-provided secure storage for key shares like Keychain on macOS or iOS, Secret Service on Linux, or Keystore on Android. Never store keys in plaintext files or standard databases. Test key extraction resistance. An adversary with file system access but not root or keychain access should not be able to extract keys.

For security-critical operations like key derivation, threshold signing, and capability issuance maintain audit logs. Audit logs should include operation type, timestamp, device performing operation, and result like success or failure. Audit logs are stored in the journal and are visible to all account devices. This enables detection of compromised devices.

## Conclusion

Aura's model is consent-based, context-specific, and relational expressed as a unified information-flow system. Privacy boundaries align with social relationships. Information shared within consensual relationships is not a leak. It is the foundation for collaboration. Protections focus on preventing information from crossing boundaries without consent and on bounding what observers can learn through rate and volume controls.

The model recognizes that perfect privacy is impossible in a functional system. Instead it provides tunable privacy properties that degrade gracefully under increasingly powerful adversaries. Measurement and testing ensure that privacy properties hold in practice not just in theory.

Implementation requires careful attention to cryptographic details, metadata handling, and adversary modeling. For 1.0, the information-flow budget is the primary enforcement surface. Optional mechanisms like Tor, onion routing, and cover traffic strengthen privacy further and can be layered without changing the core calculus.

See [Building on Aura](800_building_on_aura.md) for current API usage patterns and the implementation in [`crates/aura-protocol/src/guards/`](../crates/aura-protocol/src/guards/) for the guard implementations.
