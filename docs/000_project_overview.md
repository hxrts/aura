# Project Overview

Aura aims to demonstrate a practical web-of-trust architecture that adheres to the following constraints:

Network as platform: All coordination and information flow happens peer-to-peer through the social graph.

Privacy by design: Information disclosure must be selective and consent-based.

Cross-platform: The system must run on web (Chrome, Firefox, Safari via WebAssembly), mobile (iOS, Android), and desktop (macOS, Linux).

Mobile-first and Resilient: The system targets mobile devices as the primary substrate for all interaction and service provision. As such it must be highly tolerant of network partitions and device failure.

Social Recovery: Users must be able to store secrets in the network and survive catastrophic device failure.

Version Compatibility: Older clients must interact with newer ones within semantic version compatibility bounds.

In order to achieve these goals, Aura combines [threshold cryptography](101_accounts_and_commitment_tree.md), [choreographic programs that project to session typed protocols](107_mpst_and_choreography.md), [fact-based semilattices](102_journal.md), [session types](107_mpst_and_choreography.md), and [authorized effects](109_authorization.md).

## Implementation

These three pillars combine into an 8-layer architecture. The layers progress from interface definitions through user-facing applications. See [System Architecture](001_system_architecture.md) for the complete layer breakdown.

The layers are as follows:

1. Foundation (`aura-core`): Effect traits, domain types, cryptographic utilities, and error types.

2. Specification (`aura-journal`, `aura-wot`, `aura-verify`, `aura-store`, `aura-transport`, `aura-mpst`, `aura-macros`): CRDT domains, capability systems, transport semantics, session type runtime, and choreography DSL.

3. Implementation (`aura-effects`, `aura-composition`): Stateless production handlers and handler composition infrastructure.

4. Orchestration (`aura-protocol`): Multi-party coordination, guard chain, and Aura Consensus runtime.

5. Feature implementation (`aura-authenticate`, `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-relational`, `aura-rendezvous`, `aura-social`, `aura-sync`): End-to-end protocol crates for authentication, secure messaging, recovery, relational contexts, rendezvous, social topology, and synchronization.

6. Runtime composition (`aura-agent`, `aura-simulator`, `aura-app`): Complete system assembly, deterministic simulation, and portable application core.

7. User interface (`aura-terminal`): Terminal-based CLI and TUI entry points.

8. Testing and tools (`aura-testkit`, `aura-quint`): Test fixtures, mock effect handlers, and simulation harnesses.

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
