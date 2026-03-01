# Project Overview

[Aura](https://github.com/hxrts/aura) is a fully peer-to-peer, private communication system that operates without dedicated servers. It uses a web-of-trust architecture to provide discovery, data availability, account recovery, and graceful async protocol evolution.

Aura achieves this by using [threshold cryptography](102_authority_and_identity.md) to distribute trust, [session typed protocols](108_mpst_and_choreography.md) to coordinate peers safely, a CRDT-based [distributed journal](103_journal.md), and [authorized effects](104_authorization.md) to enforce capability and privacy boundaries.

## Implementation

These three pillars combine into an 8-layer architecture. The layers progress from interface definitions through user-facing applications. See [System Architecture](001_system_architecture.md) for the complete layer breakdown.

The layers are as follows:

1. Foundation (`aura-core`): Effect traits, domain types, cryptographic utilities, and error types.

2. Specification (`aura-journal`, `aura-authorization`, `aura-signature`, `aura-store`, `aura-transport`, `aura-mpst`, `aura-macros`): CRDT domains, capability systems, transport semantics, session type runtime, and choreography DSL.

3. Implementation (`aura-effects`, `aura-composition`): Stateless production handlers and handler composition infrastructure.

4. Orchestration (`aura-protocol` + `aura-guards`, `aura-consensus`, `aura-amp`, `aura-anti-entropy`): Multi-party coordination, guard chain, consensus, AMP, and anti-entropy.

5. Feature implementation (`aura-authentication`, `aura-chat`, `aura-invitation`, `aura-maintenance`, `aura-recovery`, `aura-relational`, `aura-rendezvous`, `aura-social`, `aura-sync`): End-to-end protocol crates for authentication, secure messaging, recovery, maintenance, relational contexts, rendezvous, social topology, and synchronization.

6. Runtime composition (`aura-agent`, `aura-simulator`, `aura-app`): Complete system assembly, deterministic simulation, and portable application core.

7. User interface (`aura-terminal`): Terminal-based CLI and TUI entry points.

8. Testing and tools (`aura-testkit`, `aura-quint`): Test fixtures, mock effect handlers, and simulation harnesses.

Aura separates key generation (K1/K2/K3) from agreement and finality (A1/A2/A3). Fast paths using provisional or coordinator modes provide immediate usability. Durable shared state is always consensus-finalized.

## Documentation Index

Additional documentation covers specific aspects of the system. The Foundation category covers mathematical and architectural foundations. The Core Systems category covers each major component. The Developer Guides category provides practical guides for implementation. The Project Meta category covers project structure.

**Foundation**

[Theoretical Model](002_theoretical_model.md) provides mathematical foundations including formal calculus, algebraic types, and semilattice semantics.

[System Architecture](001_system_architecture.md) describes implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols.

[Privacy and Information Flow](003_information_flow_contract.md) documents the consent-based privacy framework with trust boundaries, flow budgets, and leakage tracking.

[Distributed Systems Contract](004_distributed_systems_contract.md) specifies safety and liveness guarantees, the synchrony model, latency bounds, and adversarial assumptions.

[System Invariants](005_system_invariants.md) documents safety invariants and verification properties across the system.

**Core Systems**

[Cryptographic Architecture](100_crypto.md) documents cryptographic primitives, key derivation, and threshold signature schemes.

[Authority and Identity](102_authority_and_identity.md) describes the authority-centric identity model with opaque authorities, relational contexts, and commitment tree structure.

[Identifiers and Boundaries](101_identifiers_and_boundaries.md) documents the identifier system and context isolation.

[Operation Categories](107_operation_categories.md) defines the A/B/C operation classification, ceremony contract for key rotations and membership changes, and how ceremonies relate to optimistic CRDT operations.

[Journal System](103_journal.md) documents the fact-based journal, validation rules, deterministic reduction flows, and flow budgets.

[Relational Contexts](112_relational_contexts.md) covers guardian bindings, recovery grants, and context-scoped journals.

[Consensus](106_consensus.md) describes the Aura Consensus protocol for strong agreement.

[Effect System and Runtime](105_effect_system_and_runtime.md) covers effect system architecture and runtime composition.

[MPST and Choreography](108_mpst_and_choreography.md) documents multi-party session types and choreographic programming.

[Transport and Information Flow](109_transport_and_information_flow.md) covers guard chain enforcement, secure channel lifecycle, and FlowBudget receipts.

[Authorization](104_authorization.md) describes the authorization flow from capabilities to Biscuit tokens.

[Rendezvous Architecture](111_rendezvous.md) covers context-scoped rendezvous envelopes and channel establishment.

[Maintenance](115_maintenance.md) covers the distributed maintenance stack including snapshots and garbage collection.

[Asynchronous Message Patterns](110_amp.md) documents patterns for reliable asynchronous message coordination.

[Database Architecture](113_database.md) specifies the distributed database layer using journals, Biscuit queries, and CRDTs.

[Social Architecture](114_social_architecture.md) defines the social organization model using messages, homes, and neighborhoods.

[CLI and Terminal User Interface](116_cli_tui.md) specifies the CLI and iocraft-based TUI for Aura.

[Test Infrastructure Reference](117_testkit.md) documents the testkit crate, fixtures, and mock handlers.

[Simulation Infrastructure Reference](118_simulator.md) covers deterministic simulation architecture and fault injection.

[Conformance and Parity Reference](119_conformance.md) describes conformance testing and cross-platform parity validation.

[Formal Verification Reference](120_verification.md) documents Quint specifications and Lean proofs.

**Developer Guides**

[Hello World Guide](801_hello_world_guide.md) provides a starting point for developers new to Aura.

[Development Patterns and Workflows](805_development_patterns_guide.md) explains core implementation patterns, time domain selection, code location guidance, and typical workflows.

[Coordination Guide](803_coordination_guide.md) covers choreographic protocol design and implementation patterns.

[Advanced Coordination Guide](804_advanced_coordination_guide.md) documents advanced techniques for distributed coordination.

[Testing Guide](805_testing_guide.md) describes property testing, simulation harnesses, and validation frameworks.

[Simulation Guide](806_simulation_guide.md) covers deterministic simulation for verification and debugging.

[Verification and MBT Guide](807_verification_guide.md) explains model-based testing workflows with Quint and Lean.

[Maintenance Guide](808_maintenance_guide.md) explains distributed maintenance, snapshots, garbage collection, and system evolution.

[Lean-Quint Bridge Guide](810_lean_quint_bridge.md) documents the bridge between Lean proofs and Quint specifications.

[Reactive Signals Guide](810_reactive_signals_guide.md) covers reactive signal patterns for UI state management.

[Recovery TUI Demo](811_recovery_tui_demo.md) provides a walkthrough of the CLI recovery demo with the simulator.

[Protocol Pipeline Guide](812_protocol_pipeline.md) documents the protocol development pipeline from specification to implementation.

[Runtime Harness Guide](813_runtime_harness_guide.md) covers end-to-end runtime validation with real Aura instances.

**Project Meta**

[Verification Coverage Report](998_verification_coverage.md) summarizes formal verification, model checking, and conformance testing coverage.

[Project Structure](999_project_structure.md) provides a comprehensive crate structure overview with the dependency graph.
