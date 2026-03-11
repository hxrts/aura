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

### 1. Foundation

[Theoretical Model](002_theoretical_model.md) establishes the formal calculus, algebraic types, and semilattice semantics underlying the system.

[System Architecture](001_system_architecture.md) describes the 8-layer architecture, effect patterns, and choreographic protocol structure.

[Privacy and Information Flow Contract](003_information_flow_contract.md) specifies consent-based privacy with trust boundaries, flow budgets, and leakage tracking.

[Distributed Systems Contract](004_distributed_systems_contract.md) defines safety and liveness guarantees, synchrony assumptions, and adversarial tolerance.

### 2. Core Systems

[Cryptography](100_crypto.md) documents primitives, key derivation, threshold signatures, and VSS schemes.

[Identifiers and Boundaries](101_identifiers_and_boundaries.md) defines the identifier types and their privacy-preserving properties.

[Authority and Identity](102_authority_and_identity.md) describes opaque authorities, commitment trees, and relational context structure.

[Journal](103_journal.md) specifies fact-based journals, validation rules, and deterministic reduction flows.

[Authorization](104_authorization.md) covers capability semantics, Biscuit token integration, and guard chain authorization.

[Effect System](105_effect_system.md) documents effect traits, handler design, and context propagation.

[Runtime](120_runtime.md) describes lifecycle management, guard chain execution, and service composition.

[Harness UX Determinism Design Note](121_harness_ux_determinism.md) defines the shared UX contract, authoritative observation rules, and browser freshness model for parity-critical harness flows.

[Consensus](106_consensus.md) specifies single-shot agreement for non-monotone operations with witness attestation.

[Operation Categories](107_operation_categories.md) defines A/B/C operation tiers, K1/K2/K3 key generation, and agreement levels.

[MPST and Choreography](108_mpst_and_choreography.md) covers multi-party session types and choreographic protocol projection.

[Transport and Information Flow](109_transport_and_information_flow.md) specifies guard chain enforcement, secure channels, and flow receipts.

[Aura Messaging Protocol (AMP)](110_amp.md) documents reliable async messaging with acknowledgment and ordering patterns.

[Rendezvous Architecture](111_rendezvous.md) covers context-scoped peer discovery and encrypted envelope exchange.

[Relational Contexts](112_relational_contexts.md) specifies guardian bindings, recovery grants, and cross-authority journals.

[Database Architecture](113_database.md) defines the query layer using journals, Biscuit predicates, and CRDT views.

[Social Architecture](114_social_architecture.md) describes the three-tier model of messages, homes, and neighborhoods.

[Distributed Maintenance Architecture](115_maintenance.md) covers snapshots, garbage collection, and system evolution.

[CLI and Terminal User Interface](116_cli_tui.md) specifies command-line and TUI interfaces for Aura operations.

[Test Infrastructure Reference](117_testkit.md) documents test fixtures, mock handlers, and scenario builders.

[Simulation Infrastructure Reference](118_simulator.md) covers deterministic simulation with virtual time and fault injection.

[Formal Verification Reference](119_verification.md) describes Quint model checking and Lean theorem proving integration.

### 3. Developer Guides

[Getting Started Guide](801_hello_world_guide.md) provides a starting point for developers new to the codebase.

[Effects and Handlers Guide](802_effects_guide.md) covers the algebraic effect system, handler implementation, and platform support.

[Choreography Development Guide](803_choreography_guide.md) explains choreographic protocol design, CRDTs, and distributed coordination.

[Testing Guide](804_testing_guide.md) covers test patterns, fixtures, conformance testing, and runtime harness.

[Harness UX Determinism Design Note](121_harness_ux_determinism.md) explains the determinism model for shared-flow harness execution, revisioned observation, and trace conformance.

[Simulation Guide](805_simulation_guide.md) explains deterministic simulation for debugging and property verification.

[Verification Guide](806_verification_guide.md) documents Quint model checking and Lean proof workflows.

[System Internals Guide](807_system_internals_guide.md) covers guard chain internals, service patterns, and reactive scheduling.

[Distributed Maintenance Guide](808_maintenance_guide.md) covers operational concerns including snapshots and system upgrades.

### 4. Project Meta

[UX Flow Coverage Report](997_ux_flow_coverage.md) tracks harness and scenario coverage for user-visible flows.

[Verification Coverage Report](998_verification_coverage.md) tracks formal verification status across Quint specs and Lean proofs.

[Project Structure](999_project_structure.md) documents the 8-layer crate architecture and dependency relationships.
