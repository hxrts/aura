# RFC 131: Privacy Model Specification

**Related**: RFC 090 (Private DAOs), RFC 041 (Rendezvous), RFC 130 (Privacy Testing)

## Overview

Aura's privacy model is fundamentally consent-based. Privacy boundaries align with social relationships rather than technical boundaries, reflecting the principle that privacy violations occur when information crosses trust boundaries without consent, not when information flows between consenting parties.

This document provides a formal specification of Aura's privacy model, defining what information is visible to different adversaries, how privacy properties degrade under various attacks, and how the system maintains privacy through technical mechanisms like DKD, encrypted envelopes, and cover traffic.

## Privacy Philosophy

Traditional privacy systems treat all information disclosure as a privacy violation, forcing users to choose between complete isolation and complete exposure. Aura recognizes that privacy is relational: sharing information with trusted parties is not a privacy violation, it's the foundation of meaningful collaboration.

The system implements three core principles:

**Consensual Disclosure**: When you join a group or establish a relationship, you consent to sharing certain information with those parties. This is not a privacy leak, it's the basis for coordination. A group seeing that you're active, a friend knowing you're available, a collaborator accessing shared documents—these are features, not bugs.

**Contextual Identity**: Through Deterministic Key Derivation (DKD), your identity is context-specific. You present different identities in different contexts, and only parties within a relationship can link these identities. This prevents global identity correlation.

**Neighborhood Visibility**: In a fully peer-to-peer system, your immediate gossip neighbors necessarily observe some metadata, i.e. encrypted envelopes. This is acceptable because: (1) they cannot decrypt content, (2) they cannot determine ultimate sender/receiver due to onion routing, and (3) cover traffic obscures activity patterns.

## Privacy Boundaries

Aura defines three distinct privacy boundaries, each with different visibility properties:

### Within Relationship Boundary

When you establish a direct relationship with another device or join a group, you cross into the relationship boundary. Within this boundary, both parties have consented to share information necessary for coordination.

**Visible Information**: Participants can see your context-specific identity, your online/offline status, your participation in shared activities, and the content of messages you send. They can observe when you're active in the shared context and what actions you take. This visibility is by design, enabling collaboration.

**Privacy Guarantees**: Even within a relationship, participants cannot link your identity across different contexts unless you explicitly share that information. One social identity can be isolated from another. Your work persona doesn't leak into your personal life.

**Technical Mechanism**: DKD ensures that each relationship gets a unique identity derived from `(account_key, app_id, context_label)`. Even if two parties know you in different contexts, they cannot cryptographically link these identities without your cooperation.

### Within Neighborhood Boundary

Your neighborhood consists of devices that forward gossip traffic for you. These are either direct relationships or transitive connections through the web of trust. They see encrypted envelopes but cannot decrypt them.

**Visible Information**: Neighbors observe that encrypted traffic is passing through them. They see envelope sizes (fixed), rtag identifiers (rotating), and timing patterns. With sustained observation, they might infer that certain rtags receive frequent traffic, suggesting an active relationship. They can count the volume of traffic associated with particular rtags.

**Privacy Guarantees**: Neighbors cannot decrypt envelope content, determine the ultimate sender or receiver, or link rtags to real identities. Onion routing ensures that each hop only knows the previous and next hop, not the entire path. Rotating rtags prevent long-term tracking of a single relationship.

**Attack Surface**: The primary attack at this boundary is traffic analysis. A sophisticated adversary controlling multiple gossip hops might correlate timing patterns across hops to infer communication patterns. However, this requires: (1) controlling multiple strategic positions, (2) sustained observation over time, and (3) overcoming cover traffic and batching defenses.

### External Observer Boundary

External observers have no relationship with you and are not part of your gossip neighborhood. In Aura's threat model, external observers should learn nothing.

**Visible Information**: With Tor integration, external observers see only encrypted Tor traffic with no distinguishing characteristics. Without Tor, an ISP-level adversary might observe that you're connecting to known Aura nodes, but learns nothing about the content, participants, or structure of communications.

**Privacy Guarantees**: External observers cannot determine: which accounts exist, which devices belong to which accounts, what relationships exist, what groups you participate in, what content you access, or even that you're using Aura (with Tor). The system provides anonymity in the technical sense—external observers cannot distinguish Aura users from non-users.

**Technical Mechanism**: Tor provides transport-level anonymity, breaking the link between IP addresses and Aura identities. Fixed-size envelopes prevent size-based fingerprinting. The lack of a central directory or discovery service means there's no global metadata to leak.

## Privacy Layers

Aura's privacy model operates across five distinct layers, each with specific properties and threat models:

### Identity Privacy

Traditional systems leak identity through persistent identifiers. Even with pseudonyms, global identifiers allow correlation across contexts. Aura eliminates this through context-specific identities.

**Property**: An identity used in context A cannot be linked to an identity used in context B without explicit key material or cooperation from the account holder.

**Mechanism**: DKD derives context-specific keys using `derive(account_root, app_id, context_label)`. The derivation is deterministic but irreversible—knowing the derived key for context A reveals nothing about context B. Each app gets its own identity namespace, and within each app, different contexts get different identities.

**Threat Model**: An adversary who observes you in multiple contexts and suspects they're the same account cannot prove this cryptographically. The only attack vector is side-channel correlation—if you exhibit the same writing style, posting patterns, or behavioral fingerprints across contexts, a sophisticated adversary might probabilistically link them. This is fundamentally a social attack, not a technical one.

**Testing Metric** (from RFC 130): `identity_linkability_score` measures the probability that an observer can link identities across contexts. Score should remain below 0.05 (5% confidence) for any pair of contexts without collusion.

### Relationship Privacy

Your social graph is sensitive information. Who you communicate with reveals personal relationships, organizational structures, and social patterns. Aura keeps the relationship graph opaque to external observers while allowing consensual visibility within relationships.

**Property**: The existence of a relationship between two identities is only visible to: (1) the parties in the relationship, (2) parties who both parties have explicitly disclosed the relationship to (e.g., by both being in a shared group).

**Mechanism**: Relationships are established through out-of-band key exchange or web-of-trust introductions. There is no central directory. Relationship establishment happens through encrypted envelopes with ephemeral rendezvous points that are only discoverable by parties who know the pre-shared secret.

**Threat Model**: An external observer monitoring network traffic cannot determine that Alice and Bob have a relationship. They might observe that Alice's gossip neighborhood overlaps with Bob's (if they're neighbors in the web of trust), but cannot prove direct communication. A neighborhood adversary who controls gossip hops might correlate timing patterns, but cover traffic and batching degrade this attack.

**Group Relationships**: Groups create consensual disclosure zones. If Alice, Bob, and Carol are all in group G, then each member knows about the others' membership. This is not a privacy leak—it's the definition of a group. However, an external observer or neighborhood adversary cannot enumerate group membership without being invited.

**Testing Metric**: `relationship_inference_confidence` measures probability that an observer can infer the existence of a relationship. Should remain below 0.10 for neighborhood adversaries, and 0.01 for external observers.

### Group Privacy

Groups are micro-anonymity sets. Within a group, members are visible to each other (consensual disclosure). Outside the group, membership is hidden. The group itself operates as a threshold entity with its own group identity.

**Property**: Group membership is only visible within the group. External observers cannot enumerate members, determine group size, or even confirm a group's existence. Group communications are indistinguishable from person-to-person communications at the envelope level.

**Mechanism**: Groups have their own DKD-derived identity independent of member identities. Members hold shares of the group's threshold key. Group envelopes are encrypted to the group's public key, making them indistinguishable from person-to-person envelopes. The rtag for group traffic rotates independently.

**Anonymity Within Groups**: For privacy-sensitive operations, groups can operate in anonymity mode where actions are attributed to the group rather than individuals. A document edit shows "group G modified this" rather than "Alice modified this." This requires threshold signatures where k members approve but individual signers remain anonymous.

**Threat Model**: The primary attack is membership enumeration through traffic analysis. If an adversary observes that Alice, Bob, and Carol all receive messages shortly after a group event, they might infer shared membership. Defenses include: (1) randomized delivery delays, (2) cover traffic to all potential members, (3) batching deliveries to obscure correlation.

**Testing Metric**: `group_membership_inference` measures probability of inferring membership through traffic analysis. Should remain below k-anonymity bound: P(specific member | observation) ≤ 1/k where k is group size.

### Content Privacy

All content is end-to-end encrypted. Only parties with the appropriate key material can decrypt. This is table stakes for any privacy system.

**Property**: Message content, stored documents, and application data are encrypted such that only authorized parties (direct recipients, group members, or capability holders) can decrypt.

**Mechanism**: Messages use authenticated encryption (AES-GCM) with per-message keys derived from shared secrets. Stored content uses content-addressed encryption where the encryption key is derived from the content hash plus a secret salt known only to authorized parties. Capabilities grant access by providing the decryption key.

**Forward Secrecy**: Session keys are ephemeral. Compromise of long-term identity keys does not compromise past session content. Aura uses the Double Ratchet pattern for person-to-person messaging and threshold ratcheting for group communications.

**Threat Model**: The only content-level attack is brute force cryptanalysis or compromise of key material. Aura uses well-studied primitives (AES-256-GCM, Ed25519, HPKE) that provide 128-bit security. We assume adversaries cannot break these primitives or factor large numbers.

**Testing Metric**: Content privacy is binary—either the adversary has key material or they don't. Testing focuses on key management: `key_exposure_rate` measures how often keys leak through side channels (logs, crash dumps, memory dumps).

### Metadata Privacy

Content privacy is useless if metadata leaks communication patterns. Metadata includes: timing (when messages are sent), frequency (how often you communicate), size (how much data), and participation (who's active in a group).

**Property**: Metadata visible to different adversaries should be bounded. Within relationships, metadata visibility is consensual. At the neighborhood boundary, metadata should be noisy enough to prevent precise inference. For external observers, metadata should be completely opaque.

**Mechanism**: Aura employs multiple metadata privacy techniques:

**Fixed-Size Envelopes**: All envelopes are padded to a standard size (e.g., 16KB). Small messages waste bandwidth, but large messages are chunked. This prevents size-based fingerprinting.

**Batching**: Outgoing messages are batched and sent at fixed intervals rather than immediately. This obscures the precise timing of individual messages and reduces traffic correlation opportunities.

**Cover Traffic**: Devices periodically send dummy envelopes to random contacts or groups. The dummy traffic is indistinguishable from real traffic at the neighborhood level. Cover traffic adapts to actual usage patterns—if you normally send 10 messages per hour, cover traffic maintains approximately that rate during idle periods.

**Rotating Rtags**: Envelope routing tags rotate periodically. A long-lived conversation doesn't use the same rtag forever, preventing longitudinal tracking. Rotation happens on a schedule negotiated within the relationship.

**Onion Routing**: Envelopes are onion-routed through multiple hops. Each hop sees only the previous and next hop, not the full path. This prevents single points of observation from learning complete paths.

**Threat Model**: Metadata privacy degrades gracefully under increasingly powerful adversaries. A passive neighborhood observer might infer that "someone" is communicating frequently, but cannot identify specific parties. An active adversary who controls multiple gossip hops and correlates timing across hops has better inference capability, but faces computational challenges at scale. A global passive adversary (NSA-level) might perform traffic analysis across the entire network, but faces the challenge of distinguishing real traffic from cover traffic and dealing with onion routing's path obfuscation.

**Testing Metric**: `metadata_leakage` (from RFC 130) measures information leakage in bits. For timing: H(actual_send_time | observed_traffic) should be high (>4 bits). For participation: P(user active | observed neighborhood traffic) should be close to base rate (within 10%).

## Threat Model by Adversary Position

Privacy properties vary based on adversary capabilities and position in the network. Aura's threat model explicitly considers five adversary types:

### Individual Relationship Partner

**Capabilities**: An individual who has a direct relationship with you sees everything within that relationship context—your context-specific identity, the content you share, your activity patterns within that relationship. This is consensual disclosure.

**Cannot Observe**: They cannot see your activity in other contexts, link your identity across contexts, or determine your other relationships unless you explicitly share that information. They cannot access content or capabilities you haven't granted them.

**Attack Surface**: The attack here is social engineering or context correlation. If you accidentally reveal cross-context information (e.g., mention something from context A while in context B), they might infer the link. Technical mechanisms cannot prevent social-layer correlation.

**Mitigation**: User education and interface design. The UI clearly indicates which context you're operating in. Different contexts use different visual themes. The system prompts before sharing information that might leak cross-context details.

### Group Insider

**Capabilities**: A group member sees all group activity—member identities (within the group context), group content, participation patterns, and threshold operations. They can observe who's active, who proposes changes, and who approves operations.

**Cannot Observe**: They cannot determine members' identities outside the group context, access other groups those members participate in, or see person-to-person relationships between members that aren't disclosed within the group.

**Attack Surface**: Group members might collude to link identities across contexts if multiple members recognize the same behavioral patterns. A malicious group member might attempt to deanonymize threshold signatures by analyzing signing patterns or timing.

**Mitigation**: Groups enforce k-anonymity for sensitive operations. When anonymity is required, at least k members participate in threshold signing, and individual signers are not revealed. Signing rounds include random delays to prevent timing correlation.

### Gossip Neighborhood Participant

**Capabilities**: Devices in your gossip neighborhood forward encrypted envelopes. They observe envelope metadata—size (fixed), rtag (rotating), timing, and frequency. With sustained observation, they might infer traffic patterns: "rtag X receives frequent messages" or "this device is very active."

**Cannot Observe**: They cannot decrypt envelope content, determine ultimate sender or receiver due to onion routing, link rtags to real identities, or distinguish real traffic from cover traffic without statistical analysis over long periods.

**Attack Surface**: The primary attack is traffic correlation. If an adversary controls multiple hops in an onion route, they might correlate timing patterns to infer communication. If they observe your neighborhood over weeks, they might statistically filter out cover traffic and identify real communication patterns.

**Mitigation**: Onion routing prevents single-hop correlation. Cover traffic obscures activity patterns—the system maintains steady traffic rates even during idle periods. Batching and random delays break timing correlation. Rtag rotation limits tracking duration. The effectiveness of these defenses is quantitatively measured using the probabilistic observer model from RFC 130.

### Network-Level Observer

**Capabilities**: An adversary who can observe network traffic (ISP, nation-state) sees IP-level connections and packet timing. Without Tor, they can observe that you're connecting to known Aura nodes. With Tor, they see only that you're using Tor.

**Cannot Observe**: They cannot decrypt envelope content, determine which Aura accounts you control, see the structure of relationships or groups, or distinguish Aura traffic from other encrypted traffic when using Tor.

**Attack Surface**: Without Tor, confirmation attacks are possible—if they suspect you're account X, they can correlate your IP connectivity with account X's activity. With Tor, correlation requires breaking Tor's anonymity properties, which is a hard problem. Even with Tor compromised, fixed-size envelopes and cover traffic prevent precise correlation.

**Mitigation**: Tor integration eliminates IP-level correlation. All Aura network traffic should route through Tor in high-security mode. The system warns users if Tor is unavailable. For lower-security scenarios (e.g., trusted organizational networks), Tor can be disabled, but users are informed of the implications.

### Compromised Device

**Capabilities**: If an adversary compromises one device in a multi-device account, they gain access to: (1) that device's key share, (2) the account's journal state as synced to that device, (3) any cached content, and (4) the device's view of relationships and groups.

**Cannot Observe**: A single compromised device cannot: (1) perform threshold operations alone (requires M of N devices), (2) derive the account's root key (threshold secret sharing), (3) access content encrypted to other contexts not synced to that device, or (4) impersonate the account without threshold signing from other devices.

**Attack Surface**: The compromised device can observe all activity visible to that device, including journal updates from other devices. It can attempt to exfiltrate its key share. If the adversary compromises M of N devices, they gain full account control.

**Mitigation**: Threshold cryptography limits single-device compromise impact. Users can revoke compromised devices through resharing (which requires M devices, so assumes at least one honest device). Session epochs invalidate old credentials when compromise is detected. Platform-specific secure storage (Keychain, Secret Service) protects key shares from trivial extraction. In the future, secure enclaves (iOS Secure Enclave, TEE) provide hardware-backed key protection.

## Hub Node Problem and Mitigation

In any web-of-trust gossip network, some nodes naturally become hubs—highly connected nodes that forward traffic for many peers. Hub nodes are valuable for network efficiency but create privacy risks.

**Privacy Risk**: A hub node observes a large fraction of network traffic. While they cannot decrypt content, they see envelope metadata for many relationships. With statistical analysis, a hub might infer the network's communication graph structure, identify highly active accounts, or correlate timing patterns across multiple relationships.

**Mitigation Strategy**: Aura employs several techniques to limit hub node privacy impact:

**Onion Routing Diversity**: Routes are selected to minimize the fraction of a path observed by any single node. If node H is a hub, routes avoid using H for consecutive hops. The routing algorithm optimizes for low-overlap paths—if one path uses H, alternative paths avoid H where possible.

**Hub Awareness**: The system tracks node connectivity and identifies potential hubs. Users can choose to avoid routing through suspected hubs if privacy is a higher priority than efficiency. The routing algorithm accepts a privacy parameter: higher privacy settings avoid hubs even at the cost of longer routes or higher latency.

**Decoy Traffic to Hubs**: To prevent hubs from distinguishing real traffic from cover traffic, the system sends decoy envelopes through hubs at a steady rate. From the hub's perspective, it's always forwarding traffic, so the presence or absence of real traffic is obscured.

**Distributed Hub Role**: Rather than allowing single nodes to become mega-hubs, the system encourages distributed hub roles. Multiple well-connected nodes share the forwarding load. Routing algorithms prefer medium-connectivity nodes over extremely high-connectivity nodes when privacy matters.

**Hub Observation Limits**: Even as a hub, there are limits to what can be observed. Fixed-size envelopes prevent size correlation. Rotating rtags limit tracking duration. Batching and delays break timing correlation. Cover traffic adds noise. A hub might observe "lots of traffic," but extracting specific communication patterns requires sustained observation and statistical inference, which RFC 130's testing framework is designed to measure and bound.

## Cover Traffic Strategy

Cover traffic is essential for metadata privacy but expensive if done naively. Aura employs an adaptive, group-based strategy that balances privacy and efficiency.

**Adaptive Rate**: Cover traffic adapts to real usage patterns. If you typically send 20 messages per hour during work hours, the system maintains approximately that rate during idle periods. This prevents an adversary from distinguishing "active" from "idle" states by observing traffic rate changes.

**Group-Based Cover**: Rather than sending individual cover messages, the system leverages groups for efficient cover traffic. Groups naturally have steady traffic rates due to multiple members. By participating in groups, your individual traffic pattern is obscured within the group's aggregate pattern. A group of 10 members sending aggregate 100 messages/hour provides good cover for individual members sending 5-20 messages/hour.

**Scheduled Traffic**: Cover traffic sends at scheduled intervals rather than random times. This creates a predictable background rate that adversaries must filter out. Real messages are inserted into scheduled traffic slots, making them indistinguishable from cover traffic.

**Content-Indistinguishable Dummy Messages**: Cover traffic envelopes are cryptographically indistinguishable from real envelopes. They have proper rtags, are encrypted, and follow the same routing as real messages. Only the recipient can determine that an envelope is cover traffic (by attempting to decrypt and receiving a specific "dummy" marker).

**Efficiency Optimization**: Cover traffic is expensive. To minimize cost, the system uses several optimizations: (1) groups amortize cover traffic across members, (2) cover rate scales with actual usage (active users have more cover traffic than occasional users), (3) low-priority cover traffic can be batched and delayed, and (4) users can tune the cover traffic/privacy tradeoff based on their threat model.

**Measurement**: RFC 130's probabilistic observer model measures cover traffic effectiveness by quantifying `P(real traffic | observed envelope)`. Effective cover traffic keeps this probability close to 0.5—an observer cannot distinguish real from dummy traffic better than random guessing.

## Integration with RFC 130 Testing Framework

This specification defines what privacy properties Aura should provide. RFC 130 defines how to test whether those properties hold in practice. The integration works as follows:

**Privacy Properties as Test Invariants**: Each privacy property (identity unlinkability, relationship privacy, etc.) translates to a test invariant. For example, "identity unlinkability across contexts" becomes `assert!(observer.identity_linkability_score(context_a, context_b) < 0.05)`.

**Adversary Models as Test Observers**: Each adversary type (neighborhood participant, network observer, etc.) is implemented as a test observer with specific capabilities. Tests instantiate observers with varying capabilities and measure what they can infer.

**Probabilistic Bounds**: Privacy properties are probabilistic, not absolute. Tests measure inference confidence and verify it remains below specified bounds. For example, a neighborhood observer's confidence in inferring a relationship should stay below 10%.

**Simulation-Based Testing**: Tests run in the simulation framework (aura-sim) with deterministic time and network conditions. This allows reproducible measurement of privacy properties under controlled adversarial scenarios.

**Ground Truth Oracle**: The simulation maintains a test-only ground truth oracle that knows all relationships, identities, and communications. This oracle is strictly isolated from adversary observers and exists only for measuring what adversaries can infer versus what actually happened.

**Regression Testing**: Privacy tests are part of CI. Any change that degrades privacy properties (e.g., increases inference confidence beyond thresholds) fails the test. This prevents accidental privacy regressions.

## Implementation Guidance

Implementing Aura's privacy model requires careful attention across the entire stack:

**DKD Implementation**: Key derivation must be cryptographically sound. Use HKDF with domain separation. Never reuse keys across contexts. The derivation path should include: account root key (secret), app_id (public), context_label (public), and a domain separator (e.g., "aura.key.derive.v1"). Test that keys derived for different contexts are uncorrelated.

**Envelope Layer**: Envelopes must be fixed-size, encrypted, authenticated, and onion-routed. Padding must be non-distinguishable (use random bytes, not zeros). Rtags must rotate on a schedule negotiated in-band. Onion routing must select diverse paths. Test that envelopes are indistinguishable at the byte level.

**Cover Traffic**: Implement cover traffic at the envelope layer, not the application layer. Dummy envelopes should be generated by the transport layer and injected into scheduled traffic slots. Recipients must silently drop dummy envelopes without notifying the application layer. Test that cover traffic is indistinguishable from real traffic to network observers.

**Gossip Forwarding**: Nodes forward envelopes they cannot decrypt. Forwarding must be prompt (to prevent timing correlation based on delays) but batched (to prevent size-based correlation from small vs. large batches). Nodes should not log or store forwarded envelope metadata beyond what's needed for routing (rtag, next hop).

**Journal Privacy**: The CRDT journal contains sensitive metadata about account structure. Journal events should be encrypted when synced across untrusted paths. Within a relationship or group, journal events can be plaintext (consensual disclosure), but when syncing through a relay or storage provider, encrypt the journal with a key known only to account devices.

**Capability System**: Capabilities grant access to resources. When implementing capabilities, ensure they don't leak information to unauthorized parties. A capability should be opaque—possessing a capability grants access, but the capability itself doesn't reveal what resource it grants access to or who issued it. Use capabilities as bearer tokens with authenticated encryption.

**Tor Integration**: All network traffic should route through Tor by default. Provide a clear UI indication when Tor is disabled. Consider implementing a "high-security mode" that enforces Tor and refuses to operate without it. Test that Tor integration doesn't leak IP addresses through DNS queries or other side channels.

**Platform-Specific Key Storage**: Use platform-provided secure storage for key shares (Keychain on macOS/iOS, Secret Service on Linux, Keystore on Android). Never store keys in plaintext files or standard databases. Test key extraction resistance—an adversary with file system access but not root/keychain access should not be able to extract keys.

**Audit Logging**: For security-critical operations (key derivation, threshold signing, capability issuance), maintain audit logs. Audit logs should include: operation type, timestamp, device performing operation, and result (success/failure). Audit logs are stored in the journal and are visible to all account devices. This enables detection of compromised devices.

## Conclusion

Aura's privacy model is consent-based, context-specific, and relational. Privacy boundaries align with social relationships. Information shared within consensual relationships is not a leak—it's the foundation for collaboration. Privacy protections focus on preventing information from crossing boundaries without consent.

The model recognizes that perfect privacy is impossible in a functional system. Instead, it provides tunable privacy properties that degrade gracefully under increasingly powerful adversaries. Measurement and testing (RFC 130) ensure that privacy properties hold in practice, not just in theory.

Implementation requires careful attention to cryptographic details, metadata handling, and adversary modeling. The system should default to strong privacy (Tor, cover traffic, onion routing) but allow users to tune the privacy/efficiency tradeoff based on their threat model.
