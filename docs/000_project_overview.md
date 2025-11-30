# Project Overview

This document provides an overview of Aura's architecture and goals.

## Project Goals

Aura aims to demonstrate that web-of-trust architectures can be practical and user-aligned. Instead of anchoring trust to a centralized service or a single device, Aura treats the social network as the platform. Identity is expressed through opaque authorities plus relational contexts. This keeps social relationships at the center of security decisions instead of infrastructure providers.

The target is 20 close friends using the system twice weekly. This constraint forces delivery of something unique and valuable, not just technically interesting. Users must feel confident that their privacy expectations will not be violated, their data is durable, and the system is secure enough to trust with real relationships.

## Problem Statement

Most existing approaches cannot meet the requirements for a practical web-of-trust system. Centralized services violate peer-to-peer and alignment constraints. Eventually-consistent systems without strong semantics fail unpredictably during network partitions. Ad-hoc peer-to-peer protocols cannot safely evolve because protocol changes fragment the network.

 Aura's design solves these problems by combining threshold cryptography (FROST primitives in `aura-core::crypto::tree_signing`—the legacy `aura-frost` crate has been removed), [choreographic protocols](107_mpst_and_choreography.md), fact-based semilattices, [session types](107_mpst_and_choreography.md), [guard-chain-enforced effects](109_authorization.md), and deterministic simulation.

## Eight Constraints

Aura must meet eight constraints simultaneously. These constraints shape every architectural decision.

**Network as platform:** All coordination happens peer-to-peer through the social graph. There is no separate infrastructure layer.

**Privacy by design:** Information disclosure must be selective and consent-based.

**Cross-platform deployment:** The system must run on web (Chrome, Firefox, Safari via WebAssembly), mobile (iOS, Android), and desktop (macOS, Linux).

**Social-graph-based coordination:** Information flows through your social graph based on explicit choices. Peer discovery and storage are performed by your social network. The system has no servers, not even for peer discovery or bootstrapping.

**Offline-first operation:** Latency must not create friction during daily use. The app works in airplane mode and syncs seamlessly when connectivity returns.

**Decentralized secret storage:** Users must be able to store secrets without external backups. Most failure scenarios are recoverable through snapshots.

**No single point of failure:** The system must provide real security without depending on any single device or entity.

**Version compatibility:** Older clients must interact with newer ones within semantic version compatibility bounds.


## Architectural Solution

Aura's solution maps each constraint to a required capability and an architectural component.

| Constraint | Required Capability | Solution |
|-----------|---------------------|----------|
| Cross-platform | Protocol abstraction | WebAssembly and typed messages |
| Social-graph coordination | Peer discovery | Social Bulletin Board via web-of-trust |
| Offline-first | Offline-first operations | CRDTs and local-first architecture |
| Decentralized secrets | No single point of failure | Threshold cryptography M-of-N |
| No single point of failure | Real security | Threshold cryptography across devices, friends, guardians |
| Version compatibility | Safe evolution | Semantic versioning and types |
| Privacy by design | Selective disclosure | Content-addressed invites |
| Network as platform | Peer infrastructure | Peer-to-peer choreographic coordination |

Aura uses social trust through M-of-N cryptographic guarantees across devices, friends, and chosen guardians. The same foundations enable collaborative editing, secure messaging, decentralized storage, and multi-party computation.

## Three Architectural Pillars

Aura's architecture rests on three pillars that work together to meet all constraints.

**First pillar: Algebraic state management.** The [Journal](102_journal.md) is a fact-based CRDT that captures account and relational events. See [Theoretical Model](002_theoretical_model.md) for mathematical foundations. Facts form a join-semilattice and merge via set union. Capability frontiers are evaluated at runtime as a meet-semilattice using Biscuit tokens plus sovereign policy. Flow budgets only replicate `spent` counters. Limits are derived deterministically from current tokens and policy. This design preserves atomic consistency without storing privileged state.

**Second pillar: Protocol specification and safety.** [Multi-party session types (MPST)](107_mpst_and_choreography.md) specify distributed protocols from a global viewpoint. Automatic projection to local roles provides deadlock freedom and compile-time safety. Choreographic programming ensures that complex multi-party coordination is verifiable before deployment. Session types prevent entire classes of distributed protocol errors.

**Third pillar: Stateless effect system composition.** Effects are capabilities code can request without shared mutable state. See [Effect System and Runtime](106_effect_system_and_runtime.md) for implementation details. Effect traits live in `aura-core`. Stateless production handlers are in `aura-effects`. Handler composition infrastructure is in `aura-composition`. Multi-party orchestrators are in `aura-protocol`. Mock and stateful test handlers are in `aura-testkit`. The guard chain is explicit and ordered: `AuthorizationEffects` (Biscuit/policy evaluation) flows to `FlowBudgetEffects` (charge-before-send) to `LeakageEffects` (observer-class budgets) to `JournalEffects` (fact commit) to `TransportEffects`. Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`, emitting `EffectCommand` data that async interpreters execute in production or simulation. This sequencing eliminates deadlocks, enables deterministic testing, and keeps architectural boundaries clean.

## Implementation Architecture

These three pillars combine into an 8-layer architecture. The layers progress from interface definitions through user-facing applications. See [System Architecture](001_system_architecture.md) for the complete layer breakdown.

The layers are as follows:

1. Foundation (`aura-core`): Effect traits, domain types, cryptographic utilities, and error types.

2. Specification (Domain crates and `aura-mpst`): CRDT domains, capability systems, transport semantics, and session type definitions.

3. Implementation (`aura-effects` and `aura-composition`): Stateless production handlers and handler composition infrastructure.

4. Orchestration (`aura-protocol`): Multi-party coordination, guard chain, and Aura Consensus runtime.

5. Feature implementation: End-to-end protocol crates for authentication, secure messaging, recovery, relational contexts, rendezvous, and storage.

6. Runtime composition (`aura-agent`, `aura-simulator`): Complete system assembly and deterministic simulation.

7. User interface (`aura-cli`): CLI entry points.

8. Testing and tools (`aura-testkit`, `aura-quint`): Test fixtures, mock effect handlers, and simulation harnesses.

## Key Design Principles

These three pillars enable several key properties that solve the constraints.

**Monotone journals prevent divergence:** Facts merge via set union and never retract. Identical fact sets produce identical states across all replicas. This guarantees eventual consistency.

**Charge-before-send prevents authorization violations:** Every transport message is preceded by the guard chain. No packet is emitted without successful authorization, flow budget, and leakage checks. Fact commits are atomic.

**Session types prevent deadlock:** Multi-party session types ensure that distributed protocols cannot deadlock. Automatic projection provides compile-time safety.

**Deterministic reduction enables testing:** State reduction from fact journals is deterministic. Identical inputs produce identical outputs. This enables property-based testing and simulation.

**No circular dependencies:** Each architectural layer builds on lower layers without reaching back down. This enables independent testing, reusability, and clear responsibility boundaries. Layer 1 depends on nothing. Layer 2 depends only on Layer 1. This pattern continues through all 8 layers.

**Architecture answers one question per layer:** Foundation layer asks "what operations exist?" Specification layer asks "what does this mean?" Implementation layer asks "how do I implement and assemble effects?" Orchestration layer asks "how do I coordinate distributed protocols?" Each answer is independent and composable.

For mathematical foundations see [Theoretical Model](002_theoretical_model.md). For safety and liveness guarantees see [Distributed Systems Contract](004_distributed_systems_contract.md). For crate details see [Project Structure](999_project_structure.md).

## Documentation Index

Additional documentation covers specific aspects of the system. The Foundation category covers mathematical and architectural foundations. The Core Systems category covers each major component. The Developer Guides category provides practical guides for implementation. The Project Meta category covers project structure.

**Foundation**

[Theoretical Model](002_theoretical_model.md) provides mathematical foundations including formal calculus, algebraic types, and semilattice semantics.

[System Architecture](001_system_architecture.md) describes implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols.

[Privacy and Information Flow](003_information_flow_contract.md) documents the consent-based privacy framework with trust boundaries, flow budgets, and leakage tracking.

[Distributed Systems Contract](004_distributed_systems_contract.md) specifies safety and liveness guarantees, the synchrony model, latency bounds, and adversarial assumptions.

[System Invariants](005_system_invariants.md) documents safety invariants and verification properties across the system.

**Core Systems**

[Authority and Identity](100_authority_and_identity.md) describes the authority-centric identity model with opaque authorities and relational contexts.

[Identifiers and Boundaries](105_identifiers_and_boundaries.md) documents the identifier system and context isolation.

[Accounts and Commitment Tree](101_accounts_and_commitment_tree.md) covers the commitment tree structure for threshold identity management.

[Journal System](102_journal.md) documents the fact-based journal, validation rules, and deterministic reduction flows.

[Relational Contexts](103_relational_contexts.md) covers guardian bindings, recovery grants, and context-scoped journals.

[Consensus](104_consensus.md) describes the Aura Consensus protocol for strong agreement.

[Effect System and Runtime](106_effect_system_and_runtime.md) covers effect system architecture and runtime composition.

[MPST and Choreography](107_mpst_and_choreography.md) documents multi-party session types and choreographic programming.

[Transport and Information Flow](108_transport_and_information_flow.md) covers guard chain enforcement, secure channel lifecycle, and FlowBudget receipts.

[Authorization](109_authorization.md) describes the authorization flow from capabilities to Biscuit tokens.

[Rendezvous Architecture](110_rendezvous.md) covers context-scoped rendezvous envelopes and channel establishment.

[State Reduction](110_state_reduction.md) describes deterministic state reduction from fact journals to canonical state.

[Maintenance](111_maintenance.md) covers the distributed maintenance stack including snapshots and garbage collection.

[Asynchronous Message Patterns](112_amp.md) documents patterns for reliable asynchronous message coordination.

[Database Architecture](113_database.md) specifies the distributed database layer using journals, Biscuit queries, and CRDTs.

[Social Architecture](114_social_architecture.md) defines the social organization model using messages, blocks, and neighborhoods.

[Terminal UI Architecture](115_tui.md) specifies the ratatui-based TUI for the Aura CLI.

**Developer Guides**

[Hello World Guide](801_hello_world_guide.md) provides a starting point for developers new to Aura.

[Core Systems Guide](802_core_systems_guide.md) explains the core systems and how they work together.

[Coordination Guide](803_coordination_guide.md) covers choreographic protocol design and implementation patterns.

[Advanced Coordination Guide](804_advanced_coordination_guide.md) documents advanced techniques for distributed coordination.

[Development Patterns and Workflows](805_development_patterns.md) covers practical patterns for developing Aura systems, including code location guidance and typical implementation workflows.

[Testing Guide](805_testing_guide.md) describes property testing, simulation harnesses, and validation frameworks.

[Simulation Guide](806_simulation_guide.md) covers deterministic simulation for verification and debugging.

[Verification Guide](807_verification_guide.md) covers formal verification techniques and property validation.

[Maintenance Guide](808_maintenance_guide.md) explains distributed maintenance, snapshots, garbage collection, and system evolution.

**Project Meta**

[Project Structure](999_project_structure.md) provides a comprehensive crate structure overview with the dependency graph.

## System Features

### Maintenance and Over-the-Air Updates

Aura supports distributed maintenance and over-the-air updates through coordinated mechanisms.

Garbage collection runs with statistics tracking and device state cleanup. Over-the-air updates detect soft and hard forks with epoch fence enforcement. Snapshot coordination handles coordinated state capture. Cache invalidation integrates with all layers to ensure consistency.

### Guardian Recovery

Guardian-based recovery implements a four-level dispute escalation system for account recovery scenarios. See [Relational Contexts](103_relational_contexts.md) for guardian relationship management.

Severity levels are Low, Medium, High, and Critical. Each level has different escalation policies and auto-cancel logic. A persistent recovery effect_api maintains audit trails of all recovery operations for transparency and accountability.

Recovery operations can be visualized in the CLI with a recovery status dashboard that shows the current recovery state and evidence.

### Testing Framework

The testing framework provides comprehensive tools for verification and property-based validation.

An integration test suite covers multi-device coordination scenarios. Property-based testing works across core systems to catch edge cases. Deterministic simulation enables chaos injection and property verification without real distributed systems complexity.
