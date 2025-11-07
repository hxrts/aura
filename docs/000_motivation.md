# 000 · Motivation & Goals

## Project Goals

Aura aims to demonstrate that web-of-trust architectures can be practical, delightful, and aligned with user interests rather than platform interests.

### Social Architecture as Design Space

Secure Scuttlebutt represents the high-water mark for web-of-trust systems, but there remains a large, untapped design space. Interesting emergent behaviors arise from bottom-up social architecture rather than top-down platform control. Aura explores this space by making the network itself the platform. Aimin to create a system where users feel aligned with both their social network and the infrastructure supporting it.

### Real Usage, Real Stakes

The target: 20 close friends using the system twice weekly. This constraint forces us to deliver something unique and valuable, not just technically interesting. Users must feel confident that:
- Their privacy expectations won't be violated
- Their data is durable and won't disappear
- The system is secure enough to trust with real relationships and real information

### Network as Platform

Very few digital systems align platform interests with social network interests. Platforms are a product of their environment and how they operate as a business impacts how they operate as infrastructure. Aura directly couples the twork with the infrastructure, making peers host both the digital and physical infrastructure. When your friends are the platform, incentives align naturally.

### Start Small, Build for Scale

The project starts with friends—small, focused, and fun. But architectural choices must support eventual scale. Multi-platform support and over-the-air upgrades aren't "nice to have" features; they're foundational. Without them from day one, we'd need to reboot the entire system from scratch, fracturing the social network we built.

## Project Constraints

These constraints shape every architectural decision in Aura.

### Consent-Based Design

- **Selective disclosure**: Information flows through your social graph based on explicit choices, not platform defaults
- **Cooperative utility**: Peer discovery and storage are performed by your social network, not centralized services
- **Transparent trust trade-offs**: We take inspiration from the principle of least authority, but break it intentionally when one kind of trust naturally implies another (e.g., guardians for recovery likely implies trust for relay). When the principle is broken, the choice is deliberate and security implications are weighed carefully.

### Fully Peer-to-Peer

- No servers, not even for peer discovery or bootstrapping
- All coordination happens through the social graph
- Network must be self-sustaining from initial invitation onward

### Cross-Platform from Day One

The system must run on:
- **Web**: Chrome, Firefox, Safari (via WebAssembly)
- **Mobile**: iOS, Android
- **Desktop**: macOS, Linux

This isn't a "later phase" concern—it's a foundational requirement. Multi-platform support enables the 20-friend network to actually form and persist.

### Performance for Daily Use

- **Fast enough for daily driver status**: Latency must not create friction in common operations
- **Offline-first UX advantage**: High-latency scenarios become UX features, not bugs. The app works in airplane mode, syncs seamlessly when connectivity returns, and never blocks on network operations.

### Real Security for Real Secrets

Users must be able to store secrets without external backups (aside from snapshots). This means:
- Threshold cryptography prevents single-device compromise
- Guardian recovery enables key recovery without escrow
- Forward secrecy limits damage from historical compromises

### Upgrade Safety

- **Low catastrophe risk**: No existential dread that an upgrade will partition the network
- **Snapshot recovery**: Most failure scenarios are recoverable through snapshots
- **Ephemeral data tolerance**: System can start with ephemeral data so the web of trust is the primary durable artifact
- **Graceful degradation**: Older clients can interact with newer ones within semantic version compatibility bounds

## Why Aura?

The constraints above—fully P2P, cross-platform, offline-first, upgrade-safe, consent-based—collectively rule out most existing approaches:

- **Centralized services** violate P2P and alignment constraints (platform interests diverge from user interests)
- **Blockchain consensus** violates performance constraints (high latency, can't work offline)
- **Eventually-consistent systems** without strong semantics fail unpredictably during partitions
- **Ad-hoc P2P protocols** can't safely evolve (protocol changes fragment the network)

Meeting all constraints simultaneously requires a principled foundation. Aura combines:
- **Threshold cryptography** for security without single points of failure
- **Choreographic protocols** for deadlock-free coordination across platforms
- **Semilattice CRDTs** for offline operation with automatic convergence
- **Session types** for safe protocol evolution and cross-version compatibility
- **Deterministic simulation** for testing Byzantine faults before deployment

This isn't overengineering—it's the minimal architecture that satisfies the constraints while enabling emergent social behaviors.

### From Constraints to Capabilities

The project constraints directly motivate each architectural choice:

| Constraint                          | Required Capability                      | Aura's Solution                          |
|-------------------------------------|------------------------------------------|------------------------------------------|
| Fully P2P (no servers)              | Self-sustaining peer discovery           | Social Bulletin Board via web-of-trust   |
| Cross-platform (6 targets)          | Platform-agnostic protocol layer         | WebAssembly + typed messages             |
| Performance (daily driver)          | Offline-first, non-blocking ops          | CRDTs + local-first architecture         |
| Security (real secrets)             | No single point of compromise            | Threshold cryptography (M-of-N)          |
| Upgrade safety (low risk)           | Protocol versioning + compatibility      | Semantic versioning + typed messages     |
| Consent-based design                | Selective disclosure via social graph    | Invitation system + relationship keys    |
| Start small, scale later            | Protocols tested before deployment       | Deterministic simulation engine          |
| Network is platform                 | Peers host infrastructure                | P2P choreographic coordination           |

### Threshold Identity as Validation

Threshold identity validates that these capabilities work together for a real problem. Current identity systems force impossible choices: trust a single device (which can be lost or compromised) or trust a corporation (which can lock you out).

Aura's approach uses social trust—your devices, friends, and chosen guardians—with M-of-N cryptographic guarantees:
- No single device can act alone (threshold signatures)
- No corporate gatekeeper (fully P2P)
- Recovery through real relationships (guardian system)
- Works offline (CRDT-based account state)
- Evolves safely (versioned protocol messages)

If 20 close friends use this system twice weekly for identity and recovery, the same foundations enable collaborative editing, secure messaging, decentralized storage, multi-party computation, or any other application requiring distributed coordination through a social graph.

## 1.0 Feature Set

We consider the simplified 1.0 “shipped” when:

1. **Identity Core**
   - Account identity is threshold-based: no single device can act alone (M-of-N participation required).
   - Identity root is implemented via ratchet tree structure with cryptographic commitments.
   - Threshold Ed25519 signatures (FROST) attest all high-impact identity operations.
   - Deterministic key derivation (DKD) issues app-specific keys from identity secrets.
   - Session epoch system prevents leaked keys from probing active devices.
   - Each identity operates as a private, role-based hierarchy with explicit capability grants.

2. **Effect System & Protocol Infrastructure**
   - Algebraic effects architecture separates effect definitions from concrete handlers.
   - Effect traits (CryptoEffects, TimeEffects, TransportEffects, etc.) provide injectable interfaces.
   - Middleware system enables composable cross-cutting concerns: tracing, metrics, security, caching.
   - Handlers for production, mock handlers for testing, simulation handlers for deterministic testing.
   - Unified context management allows effects to be executed with consistent configuration.

3. **Session Type System & Choreographic Protocols**
   - Session types provide compile-time safety for distributed protocol state machines.
   - Typestate pattern prevents invalid operation sequences across multiple participants.
   - Choreographic DSL allows protocols to be written from a global viewpoint, automatically projected to local roles.
   - Protocol composition enables sub-protocols as building blocks for complex coordination.
   - Deadlock freedom guaranteed by construction (no cyclic waits in session structure).

4. **Wire Format & Typed Messages**
   - Unified message envelope format for all protocol communication (crypto, social, recovery).
   - Typed message system enables safe serialization/deserialization with version compatibility.
   - Deterministic serialization ensures cryptographic commitments are consistent across platforms.
   - SSB envelope structure with headers and authenticated ciphertexts for secure messaging.
   - Version negotiation via replicated `DeviceMetadata` with supported protocol manifests.
   - Semantic versioning (MAJOR.MINOR.PATCH) distinguishes soft forks (backwards-compatible) from hard forks (consensus-breaking).
   - Protocol upgrade system allows safe evolution of message types over time.

5. **Semilattice CRDT State Management**
   - Meet and join semilattice traits provide precise mathematical semantics for replicated state.
   - Join operations (∨) for accumulative growth; meet operations (∧) for constraint satisfaction.
   - CRDT handlers implemented as effect handlers enforce convergence laws: commutativity, associativity, idempotence.
   - State-based (CvRDT), delta-based (Δ-CRDT), and operation-based (CmRDT) implementations.
   - Effect handler integration: `CvHandler`, `DeltaHandler`, `CmHandler`, `MvHandler` coordinate state through effect system.
   - Session-typed choreographic protocols for CRDT synchronization (anti-entropy, delta gossip, operation broadcast).
   - Choreography ensures deadlock-free merge coordination with compile-time verification of message ordering.
   - Meet-based constraint protocols coordinate capability restriction and policy intersection across participants.
   - Property-based testing ensures CRDT laws are upheld across all types.

6. **Ratchet Tree & Journal System**
   - Left-balanced binary tree (LBBT) with deterministic node indices for threshold identity management.
   - Leaf nodes represent devices and guardians; branch nodes specify threshold policies (M-of-N).
   - Journal ledger stores immutable `TreeOp` entries that record all tree mutations (add/remove/rotate).
   - Commitment derivation provides tamper-evident tree snapshots with ordered children.
   - Threshold-signed `TreeOp` entries attest all tree changes with multi-signatures.
   - Intent pool (OR-set) stages proposed operations for high availability during network partitions.
   - Share contribution rounds coordinate threshold operations via session-typed choreography.
   - Forward secrecy through epoch-based rotation: old shares invalid after secret refresh.
   - Deterministic CRDT merging ensures replicas converge to identical tree state.

7. **Authenticated CRDT Ledger**
   - Account state (devices, guardians, policies, session epoch) is replicated through a signed CRDT ledger.
   - Threshold-signed events guard high-impact changes; device-scoped signatures cover high-churn fields.
   - Private social graph: relationships between accounts remain confidential.
   - Unified account ledger integrates ratchet tree, capabilities, revocations, and relationship keys.

8. **Deterministic Simulator**
   - Injectable effects enable production code to run unmodified in a deterministic, in-process harness.
   - Single, seeded PRNG provides deterministic randomness across all participants.
   - Controllable simulated time allows testing of timeouts, epochs, and protocol deadlines.
   - Deterministic network simulation with configurable latency and partition rules.
   - Fault injection hooks allow Byzantine behavior, message corruption, and selective drops without modifying protocol code.
   - 100% reproducible test failures by reusing simulation seeds.

9. **Invitation System**
   - Out-of-band invitation flow grounds all relationships in real-world trust (Signal, QR codes, physical proximity).
   - Invitation tokens bind relationship type (guardian, friend, group member) to specific capabilities.
   - Time-bounded invitations with expiration prevent indefinite credential exposure.
   - Acceptance ceremony establishes bidirectional relationship keys and mutual authentication.
   - Invitation revocation prevents compromised or unused invitations from being claimed.
   - Works across all relationship types: guardians, friends, group members, device additions.

10. **Peer Discovery & Rendezvous**
   - Social Bulletin Board (SBB) protocol for inter-account communication and peer discovery.
   - SBB envelopes flooded across transitively trusted peers enables private, invitation-based discovery.
   - Relationship keys (k_box, k_tag, k_psk) establish encrypted channels between accounts.
   - Web-of-trust model grounds discovery in social connections, not global registries.
   - Unified journal state model integrates SBB envelopes with account state for consistency.

11. **Transport Protocol Implementation**
   - At least one concrete transport protocol implementation for peer-to-peer communication.
   - Transport abstraction layer enables protocol-agnostic choreography and effects.
   - Initial target: Memory transport for testing, TCP for local network, WebSocket for cross-platform compatibility.
   - Transport middleware supports message authentication, encryption, and capability enforcement.
   - Connection management handles reconnection, backoff, and peer liveness detection.
   - Satisfies cross-platform constraint: must work across web (WebSocket), mobile (TCP/WebSocket), and desktop (TCP).

12. **Basic Garbage Collection & Ledger Compaction**
   - Threshold-signed snapshots enable safe pruning of historical CRDT events.
   - Snapshot proposals, approvals, and completion tracked in journal events.
   - Write fencing prevents new events during snapshot epochs.
   - High-water marks track pruned event boundaries for new peer synchronization.
   - Snapshot availability guarantees ensure replicas can reconstruct state without original events.

13. **Recovery & Policy**
    - Guardian-based recovery leverages invitation system to establish guardian relationships.
    - Recovery ceremony with mandatory cooldown (48h) allows dispute/veto windows for safety.
    - Policy system covers a minimal rule set (e.g., "require native device for high-risk ops").
    - Capability tokens are generated from policy decisions so transports can enforce them offline.

14. **User Interface (Web)**
    - At least one functional user interface for accessing the system.
    - Initial target: Web interface (Chrome, Firefox, Safari via WebAssembly).
    - Interface must support core workflows: account creation, device management, invitation flows, guardian setup.
    - Mobile interfaces (iOS, Android) required soon after 1.0 to reach critical mass of 20 friends.
    - Cross-platform constraint means web UI validates platform-agnostic protocol layer.
    - Interface demonstrates that choreographic protocols and CRDT state work in practice.

## Out of Scope (for 1.0)

- Distributed storage beyond basic capability-driven primitives
- Multiple transport protocols (BLE mesh, WebRTC, etc.) - only one required
- Advanced erasure coding and replication strategies
- Complex policy constructs (service signers, fine-grained rate limits)
- Full automation of cross-app integrations
- Mobile native interfaces (iOS, Android) - web interface sufficient for 1.0
