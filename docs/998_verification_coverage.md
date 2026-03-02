# Verification Coverage Report

This document provides an overview of the formal verification, model checking, and conformance testing infrastructure in Aura.

## Summary Metrics

| Metric | Count |
|--------|-------|
| Quint Specifications | 41 |
| Quint Invariants | 191 |
| Quint Temporal Properties | 11 |
| Quint Type Definitions | 366 |
| Lean Source Files | 38 |
| Lean Theorems | 118 |
| Conformance Fixtures | 4 |
| ITF Trace Harnesses | 8 |
| Testkit Tests | 111 |
| Bridge Modules | 4 |
| CI Verification Gates | 10 |

## Verification Layers

### Layer 1: Quint Specifications

Formal protocol specifications in `verification/quint/` organized by subsystem.

| Subsystem | Files | Contents |
|-----------|-------|----------|
| Root | 11 | core, authorization, recovery, invitation, interaction, leakage, sbb, time_system, transport, epochs, cli_recovery_demo |
| consensus/ | 4 | core, liveness, adversary, frost |
| journal/ | 3 | core, counter, anti_entropy |
| keys/ | 3 | dkg, dkd, resharing |
| sessions/ | 4 | core, choreography, groups, locking |
| amp/ | 1 | channel |
| liveness/ | 3 | connectivity, timing, properties |
| harness/ | 8 | amp_channel, counter, dkg, flows, groups, locking, recovery, resharing |
| tui/ | 4 | demo_recovery, flows, signals, state |

Harness modules generate ITF traces on demand via `just quint-generate-traces`. Traces are not checked into the repository. CI runs `just ci-conformance-itf` to generate traces and replay them through Rust handlers.

#### Key Specifications

| Specification | Purpose | Key Properties |
|---------------|---------|----------------|
| `consensus/core.qnt` | Fast-path consensus protocol | UniqueCommitPerInstance, CommitRequiresThreshold, EquivocatorsExcluded |
| `consensus/liveness.qnt` | Progress guarantees | ProgressUnderSynchrony, RetryBound, CommitRequiresHonestParticipation |
| `consensus/adversary.qnt` | Byzantine tolerance | ByzantineThreshold, EquivocationDetected, HonestMajorityCanCommit |
| `consensus/frost.qnt` | Threshold signatures | Share aggregation, commitment validation |
| `journal/core.qnt` | CRDT journal semantics | NonceUnique, FactsOrdered, NonceMergeCommutative, LamportMonotonic |
| `journal/anti_entropy.qnt` | Sync protocol | FactsMonotonic, EventualConvergence, VectorClockConsistent |
| `authorization.qnt` | Guard chain security | NoCapabilityWidening, ChargeBeforeSend |
| `time_system.qnt` | Timestamp ordering | TimeStamp domain semantics and comparison |

### Layer 2: Rust Integration

Files implementing Quint-Rust correspondence and model-based testing.

#### Core Integration (`aura-core`)
- `effects/quint.rs` - `QuintMappable` trait for bidirectional type mapping
- `effects/mod.rs` - Effect trait definitions with Quint correspondence

#### Quint Crate (`aura-quint`)
- `runner.rs` - `QuintRunner` with property caching and verification statistics
- `properties.rs` - `PropertySpec`, `PropertySuite`, and property categorization
- `evaluator.rs` - `QuintEvaluator` subprocess wrapper for Quint CLI
- `handler.rs` - Effect handler integration

#### Lean-Quint Bridge (`aura-quint`)

Cross-validation modules for Lean↔Quint correspondence:

| Module | Purpose |
|--------|---------|
| `bridge_export.rs` | Export Quint state to Lean-readable format |
| `bridge_import.rs` | Import Lean outputs back to Quint structures |
| `bridge_format.rs` | Shared serialization format definitions |
| `bridge_validate.rs` | Cross-validation assertions and checks |

#### Simulator Integration (`aura-simulator/src/quint/`)

17 modules implementing generative simulation:

| Module | Purpose |
|--------|---------|
| `action_registry.rs` | Maps Quint action names to Rust handlers |
| `state_mapper.rs` | Bidirectional state conversion (Rust <-> Quint JSON) |
| `generative_simulator.rs` | Orchestrates ITF trace replay with property checking |
| `itf_loader.rs` | Parses ITF traces from Quint model checking |
| `itf_fuzzer.rs` | Model-based fuzzing with coverage analysis |
| `trace_converter.rs` | Converts between trace formats |
| `simulation_evaluator.rs` | Evaluates properties during simulation |
| `properties.rs` | Property extraction and classification |
| `domain_handlers.rs` | Domain-specific action handlers |
| `amp_channel_handlers.rs` | AMP reliable channel handlers |
| `byzantine_mapper.rs` | Byzantine fault strategy mapping |
| `chaos_generator.rs` | Chaos/fault scenario generation |
| `aura_state_extractors.rs` | Aura-specific state extraction |
| `cli_runner.rs` | CLI integration for Quint verification |
| `ast_parser.rs` | Quint AST parsing for analysis |
| `mod.rs` | Module exports and re-exports |
| `types.rs` | Shared type definitions |

#### Differential Verification (`aura-simulator`)
- `differential_tester.rs` - Cross-implementation parity testing between Quint models and Rust handlers

#### Consensus Verification (`aura-consensus`)
- `core/verification/mod.rs` - Verification module facade
- `core/verification/quint_mapping.rs` - Consensus type mappings

### Layer 3: Lean Proofs

Lean 4 mathematical proofs in `verification/lean/` providing formal guarantees.

#### Type Modules (10 files)

| Module | Content |
|--------|---------|
| `Types/ByteArray32.lean` | 32-byte hash representation (6 theorems) |
| `Types/OrderTime.lean` | Opaque ordering tokens (4 theorems) |
| `Types/TimeStamp.lean` | 4-variant time enum |
| `Types/FactContent.lean` | Structured fact types |
| `Types/ProtocolFacts.lean` | Protocol-specific fact types |
| `Types/AttestedOp.lean` | Attested operation types |
| `Types/TreeOp.lean` | Tree operation types |
| `Types/Namespace.lean` | Authority/Context namespaces |
| `Types/Identifiers.lean` | Identifier types |
| `Types.lean` | Type module aggregation |

#### Domain Modules (9 files)

| Module | Purpose |
|--------|---------|
| `Domain/Consensus/Types.lean` | Consensus message types (8 definitions) |
| `Domain/Consensus/Frost.lean` | FROST signature types |
| `Domain/Journal/Types.lean` | Fact and Journal structures |
| `Domain/Journal/Operations.lean` | merge, reduce, factsEquiv (1 theorem) |
| `Domain/FlowBudget.lean` | Budget types and charging |
| `Domain/GuardChain.lean` | Guard types and evaluation |
| `Domain/TimeSystem.lean` | Timestamp comparison |
| `Domain/KeyDerivation.lean` | Key derivation types |
| `Domain/ContextIsolation.lean` | Context isolation model |

#### Proof Modules (14 files, 118 theorems)

**Consensus Proofs:**

| Module | Theorems | Content |
|--------|----------|---------|
| `Proofs/Consensus/Agreement.lean` | 3 | No two honest parties commit different values |
| `Proofs/Consensus/Validity.lean` | 7 | Only valid proposals can be committed |
| `Proofs/Consensus/Equivocation.lean` | 5 | Detection soundness and completeness |
| `Proofs/Consensus/Evidence.lean` | 8 | Evidence CRDT semilattice properties |
| `Proofs/Consensus/Frost.lean` | 12 | FROST share aggregation safety |
| `Proofs/Consensus/Liveness.lean` | 3 | Progress under timing assumptions (axioms) |
| `Proofs/Consensus/Adversary.lean` | 7 | Byzantine model bounds |
| `Proofs/Consensus/Summary.lean` | - | Master consensus claims bundle |

**Infrastructure Proofs:**

| Module | Theorems | Content |
|--------|----------|---------|
| `Proofs/Journal.lean` | 14 | CRDT semilattice (commutativity, associativity, idempotence) |
| `Proofs/FlowBudget.lean` | 5 | Charging correctness |
| `Proofs/GuardChain.lean` | 7 | Guard evaluation determinism |
| `Proofs/TimeSystem.lean` | 8 | Timestamp ordering properties |
| `Proofs/KeyDerivation.lean` | 3 | PRF isolation proofs |
| `Proofs/ContextIsolation.lean` | 16 | Context separation and bridge authorization |

#### Entry Points (4 files)
- `Aura.lean` - Top-level documentation
- `Aura/Proofs.lean` - Main reviewer entry with all Claims bundles
- `Aura/Assumptions.lean` - Cryptographic axioms (FROST unforgeability, hash collision resistance, PRF security)
- `Aura/Runner.lean` - CLI for differential testing

### Layer 4: Conformance Testing

Deterministic parity validation infrastructure in `aura-testkit`.

#### Conformance Fixtures

| Fixture | Purpose |
|---------|---------|
| `consensus.json` | Consensus protocol conformance |
| `sync.json` | Synchronization protocol conformance |
| `recovery.json` | Guardian recovery conformance |
| `invitation.json` | Invitation protocol conformance |

#### Conformance Modules

| Module | Purpose |
|--------|---------|
| `conformance.rs` | Artifact loading, replay, and verification |
| `conformance_diff.rs` | Law-aware comparison with envelope classifications |

#### Effect Envelope Classifications

| Class | Effect Kinds | Comparison Rule |
|-------|--------------|-----------------|
| `strict` | handle_recv, handle_choose, handle_acquire, handle_release | Byte-exact match required |
| `commutative` | send_decision, invoke_step | Order-insensitive under normalization |
| `algebraic` | topology_event | Reduced via domain-normal form |

## Verified Invariants

### Consensus Invariants

| Invariant | Location |
|-----------|----------|
| `InvariantUniqueCommitPerInstance` | consensus/core.qnt |
| `InvariantCommitRequiresThreshold` | consensus/core.qnt |
| `InvariantCommittedHasCommitFact` | consensus/core.qnt |
| `InvariantEquivocatorsExcluded` | consensus/core.qnt |
| `InvariantProposalsFromWitnesses` | consensus/core.qnt |
| `InvariantProgressUnderSynchrony` | consensus/liveness.qnt |
| `InvariantRetryBound` | consensus/liveness.qnt |
| `InvariantCommitRequiresHonestParticipation` | consensus/liveness.qnt |
| `InvariantQuorumPossible` | consensus/liveness.qnt |
| `InvariantByzantineThreshold` | consensus/adversary.qnt |
| `InvariantEquivocationDetected` | consensus/adversary.qnt |
| `InvariantCompromisedNoncesExcluded` | consensus/adversary.qnt |
| `InvariantHonestMajorityCanCommit` | consensus/adversary.qnt |

### Journal Invariants

| Invariant | Location |
|-----------|----------|
| `InvariantNonceUnique` | journal/core.qnt |
| `InvariantFactsOrdered` | journal/core.qnt |
| `InvariantFactsMatchNamespace` | journal/core.qnt |
| `InvariantLifecycleCompletedImpliesStable` | journal/core.qnt |
| `InvariantNonceMergeCommutative` | journal/core.qnt |
| `InvariantLamportMonotonic` | journal/core.qnt |
| `InvariantReduceDeterministic` | journal/core.qnt |
| `InvariantPhaseRegistered` | journal/counter.qnt |
| `InvariantCountersRegistered` | journal/counter.qnt |
| `InvariantLifecycleStatusDefined` | journal/counter.qnt |
| `InvariantOutcomeWhenCompleted` | journal/counter.qnt |
| `InvariantFactsMonotonic` | journal/anti_entropy.qnt |
| `InvariantFactsSubsetOfGlobal` | journal/anti_entropy.qnt |
| `InvariantVectorClockConsistent` | journal/anti_entropy.qnt |
| `InvariantEventualConvergence` | journal/anti_entropy.qnt |
| `InvariantDeltasFromSource` | journal/anti_entropy.qnt |
| `InvariantCompletedSessionsConverged` | journal/anti_entropy.qnt |

### Temporal Properties

| Property | Location |
|----------|----------|
| `livenessEventualCommit` | consensus/core.qnt |
| `safetyImmutableCommit` | consensus/core.qnt |
| `authorizationSoundness` | authorization.qnt |
| `budgetMonotonicity` | authorization.qnt |
| `flowBudgetFairness` | authorization.qnt |
| `canAlwaysExit` | tui/state.qnt |
| `modalEventuallyCloses` | tui/state.qnt |
| `insertModeEventuallyExits` | tui/state.qnt |
| `InvariantLeakageBounded` | leakage.qnt |
| `InvariantObserverHierarchyMaintained` | leakage.qnt |
| `InvariantBudgetsPositive` | leakage.qnt |

## CI Verification Gates

Automated verification lanes wired into CI pipelines.

### Core Verification

| Gate | Command | Purpose |
|------|---------|---------|
| Property Monitor | `just ci-property-monitor` | Runtime property assertion monitoring |
| Choreography Parity | `just ci-choreo-parity` | Session type projection consistency |
| Quint Typecheck | `just ci-quint-typecheck` | Quint specification type safety |

### Conformance Gates

| Gate | Command | Purpose |
|------|---------|---------|
| Conformance Policy | `just ci-conformance-policy` | Policy rule validation |
| Conformance Contracts | `just ci-conformance-contracts` | Contract satisfaction checks |
| Golden Fixtures | `conformance_golden_fixtures` | Deterministic replay against known-good traces |

### Formal Methods

| Gate | Command | Purpose |
|------|---------|---------|
| Lean Build | `just ci-lean-build` | Compile Lean proofs |
| Lean Completeness | `just ci-lean-check-sorry` | Check for incomplete proofs (sorry) |
| Lean-Quint Bridge | `just ci-lean-quint-bridge` | Cross-validation between Lean and Quint |
| Kani BMC | `just ci-kani` | Bounded model checking for unsafe code |

## Related Commands

### Quint Verification

```bash
just quint-typecheck-all        # Typecheck all specifications
just quint-verify-models        # Model check with Apalache (CI)
just quint-generate-traces      # Generate ITF traces
just quint-check-types          # Check Quint-Rust type drift
just quint-verify spec invs     # Verify specific spec/invariants
```

### Lean Verification

```bash
just lean-build                 # Build Lean proofs
just lean-status                # Show proof status summary
just lean-oracle-build          # Build differential testing oracle
just lean-check                 # Check for incomplete proofs
```

### Conformance Testing

```bash
just verify-conformance         # Run ITF conformance tests
just ci-conformance             # Full conformance gate (CI)
just ci-conformance-strict      # Native/WASM parity lane
just ci-conformance-diff        # Threaded/cooperative differential lane
```

### Combined Verification

```bash
just verify-all                 # Run all verification (Lean + Quint + conformance)
just ci-quint-typecheck         # CI: Quint typecheck gate
just ci-quint-verify            # CI: Model checking gate
just ci-lean-build              # CI: Lean build gate
```

## Related Documentation

- [Formal Verification Reference](120_verification.md) - Architecture and specification patterns
- [Conformance and Parity Reference](119_conformance.md) - Conformance testing infrastructure
- [Verification and MBT Guide](807_verification_guide.md) - Practical verification workflows
- [Simulation Infrastructure Reference](118_simulator.md) - Generative simulation details
- [Testing Guide](805_testing_guide.md) - Testing infrastructure overview
