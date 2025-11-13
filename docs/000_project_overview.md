# Project Overview

This document provides an overview of Aura's architecture and goals.

## Project Goals

Aura aims to demonstrate that web-of-trust architectures can be practical, delightful, and aligned with user interests rather than platform interests.

Secure Scuttlebutt represents the high-water mark for web-of-trust systems, but there remains a large untapped design space. Aura explores this space by making the network itself the platform.

The target is 20 close friends using the system twice weekly. This constraint forces delivery of something unique and valuable, not just technically interesting. Users must feel confident that their privacy expectations won't be violated, their data is durable, and the system is secure enough to trust with real relationships.

## Constraints and Solutions

Aura must meet eight constraints simultaneously. These constraints shape every architectural decision.

The system must run on web (Chrome, Firefox, Safari via WebAssembly), mobile (iOS, Android), and desktop (macOS, Linux). Information flows through your social graph based on explicit choices. Peer discovery and storage are performed by your social network. The system has no servers, not even for peer discovery or bootstrapping. All coordination happens through the social graph. Latency must not create friction during daily use. The app works in airplane mode and syncs seamlessly when connectivity returns. Users must be able to store secrets without external backups. Most failure scenarios are recoverable through snapshots. Older clients can interact with newer ones within semantic version compatibility bounds.

Most existing approaches cannot meet these constraints simultaneously. Centralized services violate P2P and alignment constraints. Eventually-consistent systems without strong semantics fail unpredictably during partitions. Ad-hoc P2P protocols cannot safely evolve because protocol changes fragment the network.

Aura's solution combines threshold cryptography, choreographic protocols, semilattice CRDTs, session types, and deterministic simulation. This foundation enables the following capabilities:

| Constraint                 | Required Capability         |Solution                    |
|----------------------------|-----------------------------|------------------------------------|
| Fully P2P                  | Peer discovery              | Social Bulletin Board via web-of-trust |
| Cross-platform             | Protocol abstraction        | WebAssembly and typed messages     |
| Daily performance          | Offline-first operations    | CRDTs and local-first architecture |
| Real security              | No single point of failure  | Threshold cryptography M-of-N      |
| Upgrade safety             | Version compatibility       | Semantic versioning and types      |
| Consent-based design       | Selective disclosure        | Content-addressed invites          |
| Testable before deploy     | Simulation framework        | Deterministic simulation           |
| Network as platform        | Peer infrastructure         | P2P choreographic coordination     |

Threshold identity validates that these capabilities work together. The current choice between trusting a single device or trusting a corporation is impossible. Aura uses social trust through M-of-N cryptographic guarantees across devices, friends, and chosen guardians. This approach demonstrates that the same foundations enable collaborative editing, secure messaging, decentralized storage, and multi-party computation.

## System Architecture

Aura's architecture rests on three pillars that work together to meet the constraints above.

The first pillar is algebraic state management. The Journal is a single CRDT that combines facts (join-semilattice) and capabilities (meet-semilattice) to maintain unified account, storage, and communication state. This ensures atomic consistency across all subsystems without centralized coordination.

The second pillar is protocol specification and safety. Multi-party session types (MPST) specify distributed protocols from a global viewpoint. Automatic projection to local roles provides deadlock freedom and compile-time safety. Choreographic programming ensures that complex multi-party coordination is verifiable before deployment.

The third pillar is stateless effect system composition. Effects are capabilities that code can request without shared mutable state. The system separates effect definitions (`aura-core`) from their stateless implementations (`aura-effects`) through isolated state services. This eliminates deadlocks, enables deterministic testing, and maintains clean architectural boundaries. The orchestration layer composes effects into protocols through deadlock-free coordination patterns.

These three pillars combine into an 8-layer architecture from interface definitions through user-facing applications. See [System Architecture](002_system_architecture.md) for the complete layer breakdown, [Theoretical Model](001_theoretical_model.md) for mathematical foundations, and [Crate Wiring](999_crate_wiring.md) for dependency details.

## Documentation Index

Additional documentation covers specific aspects of the system:

| Category | Document | Description |
|----------|----------|-------------|
| Foundation | [Theoretical Model](001_theoretical_model.md) | Mathematical foundation including formal calculus, algebraic types, and semilattice semantics |
| Foundation | [System Architecture](002_system_architecture.md) | Implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols |
| Foundation | [Distributed Applications](003_distributed_applications.md) | Concrete examples showing theory and architecture in practice |
| Foundation | [Information Flow Model](004_information_flow_model.md) | Consent-based privacy framework with trust boundaries aligned to social relationships |
| Core Systems | [Identity System](100_identity_system.md) | Threshold identity management using cryptographic commitments and distributed key derivation |
| Core Systems | [Authentication System](101_authentication_system.md) | Authentication and authorization architecture separating WHO and WHAT concerns |
| Core Systems | [Maintenance System](102_maintenance_system.md) | Distributed maintenance stack including snapshots, garbage collection, and over-the-air updates |
| Core Systems | [Flow Budget System](103_flow_budget_system.md) | Information flow budget system enforcing privacy limits and spam prevention |
| Core Systems | [Peer Discovery System](104_peer_discovery_system.md) | Peer discovery and connection setup specification |
| Core Systems | [Journal System](105_journal_system.md) | Journal and ledger implementation details |
| Trust and Privacy | [Web of Trust](200_web_of_trust.md) | Capability-based Web of Trust system for authorization and spam prevention |
| Trust and Privacy | [Trust Relationships](201_trust_relationships.md) | Trust relationship formation through cryptographic ceremonies |
| Trust and Privacy | [Capability System](202_capability_system.md) | Capability-based access control using meet-semilattice operations |
| Implementation | [Ratchet Tree](300_ratchet_tree.md) | Ratchet tree structure for threshold identity management |
| Implementation | [Identifier System](301_identifier_system.md) | Identifier system and context isolation |
| Implementation | [Choreography System](302_choreography_system.md) | Choreographic programming framework for distributed protocols |
| Reference | [Effect System API](500_effect_system_api.md) | Effect system interfaces and handler patterns reference |
| Reference | [CRDT Types Reference](501_crdt_types_reference.md) | CRDT types and semilattice operations reference |
| Reference | [Capability Patterns](502_capability_patterns.md) | Capability-based access control patterns reference |
| Reference | [Error Handling Reference](503_error_handling_reference.md) | Error handling patterns and types reference |
| Reference | [Configuration Reference](505_configuration_reference.md) | Configuration system reference |
| Testing and Verification | [Simulation and Verification](600_simulation_verification.md) | Deterministic simulation framework with Quint formal verification integration |
| Testing and Verification | [Testing and Debugging](601_testing_debugging.md) | Testing strategies and debugging tools for distributed systems |
| Developer Guides | [Getting Started Guide](800_getting_started_guide.md) | Development environment setup and building first applications |
| Developer Guides | [Effect System Guide](801_effect_system_guide.md) | Effect system architecture and handler patterns |
| Developer Guides | [CRDT Programming Guide](802_crdt_programming_guide.md) | CRDT design patterns and semilattice implementation |
| Developer Guides | [Protocol Development Guide](803_protocol_development_guide.md) | Choreographic programming and protocol composition |
| Developer Guides | [Deployment Guide](804_deployment_guide.md) | Production deployment patterns and security best practices |
| Project Meta | [Glossary](998_glossary.md) | Canonical definitions for architectural concepts |
| Project Meta | [Crate Wiring](999_crate_wiring.md) | Comprehensive crate structure overview with dependency graph |
