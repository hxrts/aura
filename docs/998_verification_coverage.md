# Verification Coverage Report

This document provides an overview of the formal verification, model checking, and conformance testing infrastructure in Aura.

## Verification Boundary Statement

Aura keeps consensus and CRDT domain proof ownership in Quint models and Lean theorems.
Telltale parity lanes validate runtime conformance behavior from replay artifacts.
Telltale parity success does not count as new domain theorem coverage.
See [Formal Verification Reference](120_verification.md#assurance-summary) for the assurance classification and limits.

## Summary Metrics

| Metric | Count |
|--------|-------|
| Quint Specifications | 42 |
| Quint Invariants | 191 |
| Quint Temporal Properties | 11 |
| Quint Type Definitions | 362 |
| Lean Source Files | 38 |
| Lean Theorems | 118 |
| Conformance Fixtures | 4 |
| ITF Trace Harnesses | 9 |
| Testkit Tests | 113 |
| Bridge Modules | 4 |
| CI Verification Gates | 11 |
| Telltale Parity Modules | 1 |
| Bridge Pipeline Fixtures | 3 |

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
| harness/ | 9 | amp_channel, counter, dkg, flows, groups, locking, recovery, resharing, semantic_observation_smoke |
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
- `telltale_parity.rs` - Telltale-backed parity boundary, canonical surface mapping, and report artifact generation

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

#### Consensus Proofs

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

#### Infrastructure Proofs

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

## Contract Coverage Mapping

This section maps the contract clauses in [Privacy and Information Flow Contract](003_information_flow_contract.md) and [Distributed Systems Contract](004_distributed_systems_contract.md) to the current verification and assurance evidence.

Coverage status uses three classes:

- `Verified`: directly covered by Quint invariants, Lean proofs, or both
- `Conformance-backed`: covered by replay, parity, or deterministic conformance lanes rather than domain theorem proofs
- `Specified only`: documented as a contract requirement, but not yet directly mapped to a proof or conformance artifact in this report

### Privacy and Information Flow Contract Coverage

| Contract Area | Status | Evidence |
|---------------|--------|----------|
| Context-specific identity separation | `Verified` | `Aura.Proofs.KeyDerivation`, `Aura.Proofs.ContextIsolation` |
| Budgeted send invariant | `Verified` | `authorization.qnt`, `transport.qnt`, `Aura.Proofs.FlowBudget`, `Aura.Proofs.GuardChain` |
| Epoch-scoped receipt validity | `Verified` | `epochs.qnt` |
| Observer-budgeted metadata leakage | `Verified` | `leakage.qnt` |
| Cross-context isolation | `Verified` | `Aura.Proofs.ContextIsolation`, `transport.qnt` |
| Physical vs logical time privacy boundary | `Verified` | `time_system.qnt`, `Aura.Proofs.TimeSystem` |
| Error-channel privacy boundary | `Specified only` | No direct proof or conformance mapping recorded here |
| Retrieval not identity-addressed | `Specified only` | No direct proof or conformance mapping recorded here |
| Custody remains opaque and non-authoritative | `Specified only` | No direct proof or conformance mapping recorded here |
| Accountability evidence verified before local consequences | `Specified only` | No direct proof or conformance mapping recorded here |
| External observer protection level varies by deployment mode | `Specified only` | No direct proof or conformance mapping recorded here |

### Distributed Systems Contract Coverage

| Contract Area | Status | Evidence |
|---------------|--------|----------|
| Journal CRDT laws | `Verified` | `journal/core.qnt`, `Aura.Proofs.Journal` |
| Consensus agreement and validity | `Verified` | `consensus/core.qnt`, `Aura.Proofs.Consensus.Agreement`, `Aura.Proofs.Consensus.Validity` |
| Fault-bound consensus safety assumptions | `Verified` | `consensus/adversary.qnt`, `Aura.Proofs.Consensus.Adversary` |
| Evidence CRDT laws | `Verified` | `Aura.Proofs.Consensus.Evidence` |
| Equivocation detection | `Verified` | `consensus/adversary.qnt`, `Aura.Proofs.Consensus.Equivocation` |
| FROST threshold safety | `Verified` | `consensus/frost.qnt`, `Aura.Proofs.Consensus.Frost` |
| Context isolation | `Verified` | `transport.qnt`, `Aura.Proofs.ContextIsolation` |
| Anti-entropy convergence | `Verified` | `journal/anti_entropy.qnt` |
| Fast-path and fallback liveness under assumptions | `Verified` | `consensus/liveness.qnt`, `Aura.Proofs.Consensus.Liveness` |
| Invitation lifecycle safety | `Verified` | `invitation.qnt` |
| Cross-protocol deadlock freedom | `Verified` | `interaction.qnt` |
| Operation-scoped and journal consistency model | `Verified` | `journal/core.qnt`, `journal/anti_entropy.qnt`, `consensus/core.qnt` |
| Runtime conformance against formal artifacts | `Conformance-backed` | ITF trace replay, Telltale parity, conformance fixtures |
| Onion accountability witness return and verification | `Specified only` | No direct proof or conformance mapping recorded here |
| Hold availability and custody-failure boundaries | `Specified only` | No direct proof or conformance mapping recorded here |
| Failure-class boundaries and local-only failure | `Specified only` | No direct proof or conformance mapping recorded here |
| Error-channel privacy requirements | `Specified only` | No direct proof or conformance mapping recorded here |

## CI Verification Gates

Automated verification lanes wired into CI pipelines.

### Core Verification

| Gate | Command | Purpose |
|------|---------|---------|
| Property Monitor | `just ci-property-monitor` | Runtime property assertion monitoring |
| Simulator Telltale Parity | `just ci-simulator-telltale-parity` | Artifact-driven telltale vs Aura simulator differential comparison |
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

### CI Artifacts

Conformance artifacts upload to CI for failure triage:

```
artifacts/conformance/
├── native_coop/
│   └── scenario_seed_artifact.json
├── wasm_coop/
│   └── scenario_seed_artifact.json
└── diff_report.json
```

The diff report highlights specific mismatches for investigation.

Telltale parity and bridge lanes emit additional artifacts:

```
artifacts/telltale-parity/
└── report.json

artifacts/lean-quint-bridge/
├── bridge.log
├── bridge_discrepancy_report.json
└── report.json
```

`artifacts/telltale-parity/report.json` uses schema `aura.telltale-parity.report.v1`.
`artifacts/lean-quint-bridge/bridge_discrepancy_report.json` uses schema `aura.lean-quint-bridge.discrepancy.v1`.

## Bridge Pipeline Fixtures

`aura-quint` bridge pipeline checks use deterministic fixture inputs:

| Fixture | Purpose |
|---------|---------|
| `positive_bundle.json` | Expected consistent cross-validation outcome |
| `negative_bundle.json` | Expected discrepancy detection outcome |
| `quint_ir_fixture.json` | Export/import pipeline coverage for Quint IR |

Fixtures live in `crates/aura-quint/tests/fixtures/bridge/`.

## Related Documentation

- [Formal Verification Reference](120_verification.md) - Architecture and specification patterns
- [Verification and MBT Guide](806_verification_guide.md) - Practical verification workflows
- [Simulation Infrastructure Reference](119_simulator.md) - Generative simulation details
- [Testing Guide](804_testing_guide.md) - Testing infrastructure and conformance testing
