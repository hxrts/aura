# Project Overview

This document provides an overview of Aura's architecture and goals.

## Project Goals

Aura aims to demonstrate that web-of-trust architectures can be practical and user-aligned. Instead of anchoring trust to a single device or centralized service, Aura treats the network as the platform and expresses identity through opaque authorities plus relational contexts. This keeps social relationships—rather than infrastructure providers—at the center of security decisions.

The target is 20 close friends using the system twice weekly. This constraint forces delivery of something unique and valuable, not just technically interesting. Users must feel confident that their privacy expectations won't be violated, their data is durable, and the system is secure enough to trust with real relationships.

## Constraints and Solutions

Aura must meet eight constraints simultaneously. These constraints shape every architectural decision.

The system must run on web (Chrome, Firefox, Safari via WebAssembly), mobile (iOS, Android), and desktop (macOS, Linux). Information flows through your social graph based on explicit choices. Peer discovery and storage are performed by your social network. The system has no servers, not even for peer discovery or bootstrapping. All coordination happens through the social graph. Latency must not create friction during daily use. The app works in airplane mode and syncs seamlessly when connectivity returns. Users must be able to store secrets without external backups. Most failure scenarios are recoverable through snapshots. Older clients can interact with newer ones within semantic version compatibility bounds.

Most existing approaches cannot meet these constraints simultaneously. Centralized services violate P2P and alignment constraints. Eventually-consistent systems without strong semantics fail unpredictably during partitions. Ad-hoc P2P protocols cannot safely evolve because protocol changes fragment the network.

Aura's solution combines threshold cryptography, choreographic protocols, fact-based semilattices, session types, guard-chain-enforced effects, and deterministic simulation. This foundation enables the following capabilities:

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

The first pillar is algebraic state management. The Journal is a fact-based CRDT: facts (join-semilattice) capture account and relational events, while capability frontiers are evaluated at runtime (meet-semilattice) using Biscuit tokens plus sovereign policy. Flow budgets only replicate `spent` counters; limits are derived deterministically from current tokens/policy, preserving atomic consistency without storing privileged state.

The second pillar is protocol specification and safety. Multi-party session types (MPST) specify distributed protocols from a global viewpoint. Automatic projection to local roles provides deadlock freedom and compile-time safety. Choreographic programming ensures that complex multi-party coordination is verifiable before deployment.

The third pillar is stateless effect system composition. Effects are capabilities code can request without shared mutable state. Effect traits live in `aura-core`, stateless handlers in `aura-effects`, and orchestrators in `aura-protocol`/`aura-agent`. The guard chain is explicit: `AuthorizationEffects` (Biscuit/policy evaluation) → `FlowBudgetEffects` (charge-before-send) → `LeakageEffects` (observer-class budgets) → `JournalEffects` (fact commit) → `TransportEffects`. This sequencing eliminates deadlocks, enables deterministic testing, and keeps architectural boundaries clean.

These three pillars combine into an 8-layer architecture from interface definitions through user-facing applications. See [System Architecture](docs_2/001_system_architecture.md) for the complete layer breakdown, [Theoretical Model](docs_2/001_theoretical_model.md) for mathematical foundations, [Distributed Systems Contract](docs_2/004_distributed_systems_contract.md) for safety/liveness assumptions, and [Project Structure](999_project_structure.md) for dependency details.

## Documentation Index

Additional documentation covers specific aspects of the system:

| Category | Document | Description |
|----------|----------|-------------|
| Foundation | [Theoretical Model](001_theoretical_model.md) | Mathematical foundation including formal calculus, algebraic types, and semilattice semantics |
| Foundation | [System Architecture](002_system_architecture.md) | Implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols |
| Foundation | [Information Flow](003_information_flow.md) | Consent-based privacy framework with trust boundaries aligned to social relationships |
| Core Systems | [Identity System](100_identity_system.md) | Threshold identity management using cryptographic commitments and distributed key derivation |
| Core Systems | [Authentication & Authorization System](101_auth_authz_system.md) | Authentication and authorization architecture separating WHO and WHAT concerns |
| Core Systems | [Maintenance System](102_maintenance_system.md) | Distributed maintenance stack including snapshots, garbage collection, and over-the-air updates |
| Core Systems | [Flow Budget System](docs_2/003_privacy_and_information_flow.md#flow-budget-system-reference) | Unified flow budget + leakage contract for privacy and spam resistance |
| Core Systems | [Peer Discovery & Transport](docs_2/107_transport_and_information_flow.md) | Guard chain enforcement, secure channel lifecycle, FlowBudget receipts |
| Core Systems | [Rendezvous Architecture](docs_2/108_rendezvous.md) | Context-scoped rendezvous envelopes/descriptors and channel establishment |
| Core Systems | [Journal System](docs_2/102_journal.md) | Fact-based journal, validation, and deterministic reduction flows |
| Trust and Privacy | [Web of Trust](200_web_of_trust.md) | Capability-based Web of Trust system for authorization and spam prevention |
| Trust and Privacy | [Trust Relationships](201_trust_relationships.md) | Trust relationship formation through cryptographic ceremonies |
| Trust and Privacy | [Capability System](202_capability_system.md) | Capability-based access control using meet-semilattice operations |
| Implementation | [Ratchet Tree](300_ratchet_tree.md) | Ratchet tree structure for threshold identity management |
| Implementation | [Identifier System](301_identifier_system.md) | Identifier system and context isolation |
| Reference | [Effects API](500_effects_api.md) | Legacy reference for effect traits/handlers; see `aura-agent` runtime docs for the new registry/builder |
| Reference | [Semilattice API](501_semilattice_api.md) | CRDT types and semilattice operations reference |
| Reference | [Choreography API](502_choreography_api.md) | Choreographic programming patterns reference |
| Reference | [Access Control API](503_access_control_api.md) | Capability/Biscuit-based access control reference |
| Reference | [Error Handling Reference](503_error_handling_reference.md) | Error handling patterns and types reference |
| Reference | [Configuration Reference](505_configuration_reference.md) | Configuration system reference |
| Reference | [Distributed Systems Contract](docs_2/004_distributed_systems_contract.md) | Safety/liveness guarantees, synchrony model, latency bounds, adversarial assumptions |
| Testing and Verification | [Simulation and Verification](600_simulation_verification.md) | Deterministic simulation framework with Quint formal verification integration |
| Testing and Verification | [Testing and Debugging](601_testing_debugging.md) | Testing strategies and debugging tools for distributed systems |
| Developer Guides | [Hello World Guide](801_hello_world_guide.md) | Development environment setup and building first applications |
| Developer Guides | [Core Systems Guide](802_core_systems_guide.md) | Core system architecture and patterns |
| Developer Guides | [Coordination Systems Guide](803_coordination_systems_guide.md) | Distributed coordination and protocol composition |
| Developer Guides | [Advanced Choreography Guide](804_advanced_choreography_guide.md) | Advanced choreographic programming patterns |
| Developer Guides | [Testing Guide](805_testing_guide.md) | Testing strategies for distributed systems |
| Developer Guides | [Simulation Guide](806_simulation_guide.md) | Deterministic simulation and verification |
| Project Meta | [Glossary](998_glossary.md) | Canonical definitions for architectural concepts |
| Project Meta | [Project Structure](999_project_structure.md) | Comprehensive crate structure overview with dependency graph |
