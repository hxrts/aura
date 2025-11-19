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

These three pillars combine into an 8-layer architecture from interface definitions through user-facing applications. See [System Architecture](001_system_architecture.md) for the complete layer breakdown, [Theoretical Model](001_theoretical_model.md) for mathematical foundations, [Distributed Systems Contract](004_distributed_systems_contract.md) for safety/liveness assumptions, and [Project Structure](999_project_structure.md) for dependency details.

## Documentation Index

Additional documentation covers specific aspects of the system:

| Category | Document | Description |
|----------|----------|-------------|
| Foundation | [Theoretical Model](001_theoretical_model.md) | Mathematical foundation including formal calculus, algebraic types, and semilattice semantics |
| Foundation | [System Architecture](001_system_architecture.md) | Implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols |
| Foundation | [Privacy & Information Flow](003_privacy_and_information_flow.md) | Consent-based privacy framework with trust boundaries, flow budgets, and leakage tracking |
| Foundation | [Distributed Systems Contract](004_distributed_systems_contract.md) | Safety/liveness guarantees, synchrony model, latency bounds, adversarial assumptions |
| Core Systems | [Authority & Identity](100_authority_and_identity.md) | Authority-centric identity model with opaque authorities and relational contexts |
| Core Systems | [Accounts & Ratchet Tree](101_accounts_and_ratchet_tree.md) | Ratchet tree structure for threshold identity management |
| Core Systems | [Journal System](102_journal.md) | Fact-based journal, validation, and deterministic reduction flows |
| Core Systems | [Relational Contexts](103_relational_contexts.md) | Guardian bindings, recovery grants, and context-scoped journals |
| Core Systems | [Consensus](104_consensus.md) | Aura Consensus protocol for strong agreement |
| Core Systems | [Effect System & Runtime](105_effect_system_and_runtime.md) | Effect system architecture and runtime composition |
| Core Systems | [MPST & Choreography](106_mpst_and_choreography.md) | Multi-party session types and choreographic programming |
| Core Systems | [Transport & Information Flow](107_transport_and_information_flow.md) | Guard chain enforcement, secure channel lifecycle, FlowBudget receipts |
| Core Systems | [Authorization Pipeline](108_authorization_pipeline.md) | Authorization flow from capabilities to Biscuit tokens |
| Core Systems | [Rendezvous Architecture](108_rendezvous.md) | Context-scoped rendezvous envelopes/descriptors and channel establishment |
| Core Systems | [Identifiers & Boundaries](109_identifiers_and_boundaries.md) | Identifier system and context isolation |
| Core Systems | [Maintenance](109_maintenance.md) | Distributed maintenance stack including snapshots and garbage collection |
| Core Systems | [State Reduction Flows](110_state_reduction_flows.md) | Deterministic state reduction from fact journals |
| Project Meta | [Project Structure](999_project_structure.md) | Comprehensive crate structure overview with dependency graph |
