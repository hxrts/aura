# Project Overview

[Aura](https://github.com/hxrts/aura) is a fully peer-to-peer, private communication system that operates without dedicated servers. It uses a web-of-trust architecture to provide discovery, data availability, account recovery, and graceful async protocol evolution.

To accomplish this, Aura uses threshold cryptography so no single device holds complete keys. Network topology reflects social relationships, forming a web of trust that provides discovery, availability, and recovery. State converges through CRDT journals without central coordination. Session-typed choreographic protocols ensure safe multi-party execution.

## How Aura Works

In Aura, all actors are authorities. An authority is an opaque cryptographic actor that may represent a person, a device group, or a shared context. External observers see only public keys and signed operations. This enables unlinkable participation across contexts.

State is append-only facts in journals. Each authority maintains its own journal. Shared contexts have journals written by multiple participants. Facts accumulate through CRDT merge and views are derived by reduction.

Side effects flow through explicit traits. Cryptography, storage, networking, and time are accessed only through effect handlers. This enables deterministic simulation and cross-platform portability.

Multi-party coordination uses session-typed choreographies. A global protocol specifies message flow. Each party's local behavior is projected from the global view.

Authorization passes through a layered guard chain. Before any message leaves, capabilities are verified, flow budgets are charged, and facts are committed atomically.

Aura separates key generation from agreement. Fast paths provide immediate usability while durable shared state is always consensus-finalized.

For the complete architecture, see [System Architecture](001_system_architecture.md).

## Documentation Index

The documents below cover theory, technical components, implementation guidance, and project organization.

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
Canonical role/access terminology lives in [Theoretical Model](002_theoretical_model.md#shared-terms-and-notation): `Member`, `Participant`, `Moderator`, and `Full`/`Partial`/`Limited`.

[CLI and Terminal User Interface](116_cli_tui.md) specifies the CLI and iocraft-based TUI for Aura.

[Test Infrastructure Reference](117_testkit.md) documents the testkit crate, fixtures, and mock handlers.

[Simulation Infrastructure Reference](118_simulator.md) covers deterministic simulation architecture and fault injection.

[Testing Guide](805_testing_guide.md) describes conformance testing and cross-platform parity validation.

[Formal Verification Reference](119_verification.md) documents Quint specifications and Lean proofs.

**Developer Guides**

[Hello World Guide](801_hello_world_guide.md) provides a starting point for developers new to Aura.

[Development Patterns and Workflows](805_development_patterns_guide.md) explains core implementation patterns, time domain selection, code location guidance, and typical workflows.

[Coordination Guide](803_coordination_guide.md) covers choreographic protocol design and implementation patterns.

[Advanced Coordination Guide](804_advanced_coordination_guide.md) documents advanced techniques for distributed coordination.

[Testing Guide](805_testing_guide.md) describes property testing, simulation harnesses, and validation frameworks.

[Simulation Guide](806_simulation_guide.md) covers deterministic simulation for verification and debugging.

[Verification and MBT Guide](807_verification_guide.md) explains model-based testing workflows with Quint and Lean.

[Maintenance Guide](808_maintenance_guide.md) explains distributed maintenance, snapshots, garbage collection, and system evolution.

[Reactive Signals Guide](810_reactive_signals_guide.md) covers reactive signal patterns for UI state management.

[Runtime Harness Guide](813_runtime_harness_guide.md) covers end-to-end runtime validation with real Aura instances.

**Project Meta**

[Verification Coverage Report](998_verification_coverage.md) summarizes formal verification, model checking, and conformance testing coverage.

[Project Structure](999_project_structure.md) provides a comprehensive crate structure overview with the dependency graph.
