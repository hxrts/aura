# Project Overview

This document provides an overview of Aura's architecture and goals.

## Project Goals

Aura aims to demonstrate that web-of-trust architectures can be practical, delightful, and aligned with user interests rather than platform interests.

### Social Architecture as Design Space

Secure Scuttlebutt represents the high-water mark for web-of-trust systems, but there remains a large, untapped design space. Interesting emergent behaviors arise from bottom-up social architecture rather than top-down platform control. Aura explores this space by making the network itself the platform.

### Real Usage, Real Stakes

The target: 20 close friends using the system twice weekly. This constraint forces delivery of something unique and valuable, not just technically interesting. Users must feel confident that their privacy expectations won't be violated, their data is durable, and the system is secure enough to trust with real relationships.

### Network as Platform

Very few digital systems align platform interests with social network interests. Platforms are a product of their environment and how they operate as a business impacts how they operate as infrastructure. Aura directly couples the network with the infrastructure, making peers host both. When your friends are the platform, incentives align naturally.

### Start Small, Build for Scale

The project starts with friends - small, focused, and fun. But architectural choices must support eventual scale. Multi-platform support and over-the-air upgrades are foundational. Without them from day one, we'd need to reboot the entire system from scratch, fracturing the social network we built.

## Project Constraints

These constraints shape every architectural decision in Aura.

### Consent-Based Design

- **Selective disclosure**: Information flows through your social graph based on explicit choices
- **Cooperative utility**: Peer discovery and storage performed by your social network
- **Transparent trust trade-offs**: Inspired by principle of least authority, but broken intentionally when one kind of trust naturally implies another
- **Trust-weighted flow budgets**: Every relationship carries an information-flow budget governing privacy leakage and spam rate

### Fully Peer-to-Peer

- No servers, not even for peer discovery or bootstrapping
- All coordination happens through the social graph
- Network must be self-sustaining from initial invitation onward

### Cross-Platform from Day One

The system must run on web (Chrome, Firefox, Safari via WebAssembly), mobile (iOS, Android), and desktop (macOS, Linux). Multi-platform support enables the 20-friend network to actually form and persist.

### Performance for Daily Use

- **Fast enough for daily driver status**: Latency must not create friction
- **Offline-first UX advantage**: High-latency scenarios become UX features. The app works in airplane mode, syncs seamlessly when connectivity returns, never blocks on network operations.

### Real Security for Real Secrets

Users must be able to store secrets without external backups. Threshold cryptography prevents single-device compromise. Guardian recovery enables key recovery without escrow. Forward secrecy limits damage from historical compromises.

### Upgrade Safety

- **Low catastrophe risk**: No existential dread that an upgrade will partition the network
- **Snapshot recovery**: Most failure scenarios recoverable through snapshots
- **Ephemeral data tolerance**: System can start with ephemeral data so web of trust is primary durable artifact
- **Graceful degradation**: Older clients can interact with newer ones within semantic version compatibility bounds

## Why Aura?

The constraints above - fully P2P, cross-platform, offline-first, upgrade-safe, consent-based - collectively rule out most existing approaches:

- **Centralized services** violate P2P and alignment constraints
- **Blockchain consensus** violates performance constraints (high latency, can't work offline)
- **Eventually-consistent systems** without strong semantics fail unpredictably during partitions
- **Ad-hoc P2P protocols** can't safely evolve (protocol changes fragment the network)

Meeting all constraints simultaneously requires a principled foundation combining threshold cryptography, choreographic protocols, semilattice CRDTs, session types, and deterministic simulation.

### From Constraints to Capabilities

| Constraint                          | Required Capability                      | Aura's Solution                          |
|-------------------------------------|------------------------------------------|------------------------------------------|
| Fully P2P (no servers)              | Self-sustaining peer discovery           | Social Bulletin Board via web-of-trust   |
| Cross-platform (6 targets)          | Platform-agnostic protocol layer         | WebAssembly + typed messages             |
| Performance (daily driver)          | Offline-first, non-blocking ops          | CRDTs + local-first architecture         |
| Security (real secrets)             | No single point of compromise            | Threshold cryptography (M-of-N)          |
| Upgrade safety (low risk)           | Protocol versioning + compatibility      | Semantic versioning + typed messages     |
| Consent-based design                | Selective disclosure via social graph    | Content-addressed invites + relationship keys |
| Start small, scale later            | Protocols tested before deployment       | Deterministic simulation engine          |
| Network is platform                 | Peers host infrastructure                | P2P choreographic coordination           |

### Threshold Identity as Validation

Threshold identity validates that these capabilities work together for a real problem. Current identity systems force impossible choices: trust a single device (which can be lost or compromised) or trust a corporation (which can lock you out).

Aura's approach uses social trust through M-of-N cryptographic guarantees across devices, friends, and chosen guardians. If 20 close friends use this system twice weekly for identity and recovery, the same foundations enable collaborative editing, secure messaging, decentralized storage, multi-party computation, or any other application requiring distributed coordination through a social graph.

## Architecture: Whole System + Application Layers

Aura is divided into two architectural layers:

### Layer Map

- **Data Layer**: CRDT facts (join-semilattice) and capabilities (meet-semilattice), journal state, GC/snapshots
- **Process Layer**: MPST choreographies, projection, guard chain, effect execution
- **Edge Layer**: Rendezvous, forwarding, per-hop receipts, epoch rotation

### System Model (Foundation)

The system model defines the formal mathematical foundation:

**A. Core Algebraic Types**:
- `Cap` (meet-semilattice): Authority/capability refinement
- `Fact` (join-semilattice): Knowledge accumulation
- `Journal { facts: Fact, caps: Cap }`: Unified CRDT state
- Context types: Privacy partitions

**B. Effect Interfaces**:
- Core traits: `TimeEffects`, `CryptoEffects`, `StorageEffects`, `NetworkEffects`, `JournalEffects`, `ConsoleEffects`, `RandomEffects`
- Extended traits: `SystemEffects`, `LedgerEffects`, `ChoreographicEffects`, `TreeEffects`, `AgentEffects`
- Runtime system: `AuraEffectSystem` provides unified access

**C. Multi-Party Session Types (MPST)**:
- Global protocol specifications with capability guards
- Automatic projection to local roles
- Deadlock freedom by construction
- Extensions: journal-coupling, leakage budgets

**D. Privacy & Security Invariants**:
- Context isolation: no cross-context message flow
- Unlinkability: observer cannot distinguish contexts
- Capability soundness: `need(effect) ≤ caps` enforced
- CRDT convergence: monotone merge guarantees

This layer is implementation-agnostic: it specifies what must exist and what laws must hold, not how to build it.

### Application Layer (User-Facing Features)

The application layer builds on the whole system model to deliver user-facing functionality:

**Identity & Recovery**:
- Threshold account management (ratchet tree)
- Guardian-based recovery ceremonies
- Device addition/removal protocols
- Policy enforcement

**Storage & Sync**:
- Encrypted chunk storage with capability-based access
- CRDT synchronization across devices
- Garbage collection and compaction

**Social Coordination**:
- Invitation system (content-addressed links)
- Peer discovery via SBB
- Relationship management

**User Interfaces**:
- CLI tools for operators and testing

**Developer Tools**:
- Deterministic simulator
- Formal verification (Quint integration)
- Property-based testing

The application layer implements protocols as choreographies. For example, guardian recovery is a choreography using threshold signatures and journal merge. This division ensures formal correctness via the whole system model and concrete validation via choreographic protocol implementations.

## Documentation Index

Additional documentation covers specific aspects of the system:

### Foundation
- **[001_theoretical_model.md](001_theoretical_model.md)** - Mathematical foundation including formal calculus, algebraic types, and semilattice semantics
- **[002_system_architecture.md](002_system_architecture.md)** - Implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols
- **[003_distributed_applications.md](003_distributed_applications.md)** - Concrete examples showing theory and architecture in practice
- **[004_information_flow_model.md](004_information_flow_model.md)** - Consent-based privacy framework with trust boundaries aligned to social relationships

### Core Systems
- **[100_identity_system.md](100_identity_system.md)** - Threshold identity management using cryptographic commitments and distributed key derivation
- **[101_authentication_system.md](101_authentication_system.md)** - Authentication and authorization architecture separating WHO and WHAT concerns
- **[102_maintenance_system.md](102_maintenance_system.md)** - Distributed maintenance stack including snapshots, garbage collection, and over-the-air updates
- **[103_flow_budget_system.md](103_flow_budget_system.md)** - Information flow budget system enforcing privacy limits and spam prevention
- **[104_peer_discovery_system.md](104_peer_discovery_system.md)** - Peer discovery and connection setup specification
- **[105_journal_system.md](105_journal_system.md)** - Journal and ledger implementation details

### Trust and Privacy
- **[200_web_of_trust.md](200_web_of_trust.md)** - Capability-based Web of Trust system for authorization and spam prevention
- **[201_trust_relationships.md](201_trust_relationships.md)** - Trust relationship formation through cryptographic ceremonies
- **[202_capability_system.md](202_capability_system.md)** - Capability-based access control using meet-semilattice operations

### Implementation
- **[300_ratchet_tree.md](300_ratchet_tree.md)** - Ratchet tree structure for threshold identity management
- **[301_identifier_system.md](301_identifier_system.md)** - Identifier system and context isolation
- **[302_choreography_system.md](302_choreography_system.md)** - Choreographic programming framework for distributed protocols

### Reference
- **[500_effect_system_api.md](500_effect_system_api.md)** - Effect system interfaces and handler patterns reference
- **[501_crdt_types_reference.md](501_crdt_types_reference.md)** - CRDT types and semilattice operations reference
- **[502_capability_patterns.md](502_capability_patterns.md)** - Capability-based access control patterns reference
- **[503_error_handling_reference.md](503_error_handling_reference.md)** - Error handling patterns and types reference
- **[505_configuration_reference.md](505_configuration_reference.md)** - Configuration system reference

### Testing and Verification
- **[600_simulation_verification.md](600_simulation_verification.md)** - Deterministic simulation framework with Quint formal verification integration
- **[601_testing_debugging.md](601_testing_debugging.md)** - Testing strategies and debugging tools for distributed systems

### Developer Guides
- **[800_getting_started_guide.md](800_getting_started_guide.md)** - Development environment setup and building first applications
- **[801_effect_system_guide.md](801_effect_system_guide.md)** - Effect system architecture and handler patterns
- **[802_crdt_programming_guide.md](802_crdt_programming_guide.md)** - CRDT design patterns and semilattice implementation
- **[803_protocol_development_guide.md](803_protocol_development_guide.md)** - Choreographic programming and protocol composition
- **[804_deployment_guide.md](804_deployment_guide.md)** - Production deployment patterns and security best practices

### Project Meta
- **[998_glossary.md](998_glossary.md)** - Canonical definitions for architectural concepts
- **[999_crate_wiring.md](999_crate_wiring.md)** - Comprehensive crate structure overview with dependency graph
