# Aura TODO List

This document tracks all unfinished work, placeholders, and architectural items requiring completion across the Aura codebase. Items are organized by category with priority reassessment based on architectural impact, security, and demo blockers.

---

## UX DEMO

**Tasks required for complete Bob's recovery demo. Foundation complete, focus on CLI and scenario integration.**

### 2. Bob's Recovery Demo Implementation

#### 2.1 Create Complete Bob-Focused Demo Workflow
- [ ] Implement Bob's recovery demo workflow (see docs/demo/cli_recovery.md)
**Status**: NOT STARTED
**Location**: `examples/cli_recovery_demo/` or `scenarios/integration/`
**Description**: End-to-end demo showing Bob's recovery journey with Alice and Charlie

**Components**:
1. Pre-setup phase (initialize Alice, Bob, Charlie accounts)
2. Bob's device loss scenario
3. Guardian recovery workflow
4. Post-recovery validation

**Dependencies**: 
- Scenario framework foundation (✅ DONE)
- Guard chain implementation (🔴 PENDING)
- Effect system determinism (🔴 PENDING)

**Acceptance Criteria**:
- Demo runs from start to finish without manual intervention
- All three participants (Alice, Bob, Charlie) functional
- Recovery workflow completes successfully
- Results match expected recovery state

#### 2.2 Implement CLI Demo Mode
- [ ] Create CLI demo commands and execution
**Status**: NOT STARTED
**Location**: `crates/aura-cli/src/handlers/demo.rs`
**Description**: Simple command-line demo execution following Bob's journey

**CLI Commands**:
- `aura demo setup` - Initialize three-party scenario
- `aura demo loss` - Simulate Bob's device loss
- `aura demo recover` - Execute recovery workflow
- `aura demo verify` - Validate recovery success

**Acceptance Criteria**:
- All demo commands work end-to-end
- Demo can be repeated with consistent results
- User sees clear progress indicators

#### 2.3 Implement Demo Reset and State Management
- [ ] Implement demo reset and state management
**Status**: NOT STARTED
**Description**: Reset Bob's account while preserving Alice/Charlie state

**Components**:
- Selective state reset mechanism
- Preserve guardian credentials across resets
- Re-initialize Bob's device cleanly

**Acceptance Criteria**:
- Demo repeatable 10+ times without conflicts
- Alice/Charlie state survives Bob's reset
- Recovery works identically each run

#### 2.4 Enhance Integration Testing for Demo
- [ ] Add comprehensive integration tests for demo workflow
**Status**: NOT STARTED
**Location**: Extend `tests/integration/`
**Description**: Validate demo workflow with comprehensive test coverage

**Tests Needed**:
- Chat workflow with three participants
- Bob-specific scenario variants
- Message sync and ordering validation
- Recovery state verification

**Acceptance Criteria**:
- Integration tests cover all demo scenarios
- Tests run deterministically (no flakes)
- Coverage > 80% for demo code paths

---

## 🟠 HIGH PRIORITY

**Core architecture and integration issues that block major functionality. Should be completed in first phase.**

### 3. Architecture Compliance Violations (from arch-check)

#### 3.1 Effect System Violations (CRITICAL)
- [ ] Fix direct time access violations (97 violations found)
**Status**: URGENT
**Location**: aura-invitation, aura-protocol, aura-quint, aura-recovery, aura-rendezvous, aura-simulator, aura-sync, aura-transport
**Description**: Direct usage of SystemTime::now(), Instant::now(), tokio::time calls instead of TimeEffects
**Solution**: Use effects.current_time() via TimeEffects trait
**Why Critical**: Time is impure and must be mockable for deterministic simulation
**Reference**: See [Effect System Guide](docs/106_effect_system_and_runtime.md#31-unified-time-traits) and [System Architecture](docs/001_system_architecture.md#35-impure-function-control)

- [ ] Fix direct randomness usage violations (6 violations found)
**Status**: URGENT
**Location**: aura-journal, aura-protocol consensus, aura-verify
**Description**: Direct usage of OsRng, rand::rngs instead of RandomEffects
**Solution**: Use effects.random_bytes() via RandomEffects trait
**Why Critical**: Randomness must be deterministic and controllable for simulation
**Reference**: See [Effect System Guide](docs/106_effect_system_and_runtime.md#1-effect-traits-and-categories) and [System Architecture](docs/001_system_architecture.md#35-impure-function-control)

- [ ] Fix direct filesystem access violations (8 violations found)
**Status**: URGENT
**Location**: aura-protocol factory, aura-simulator fuzzer
**Description**: Direct usage of tokio::fs calls instead of StorageEffects
**Solution**: Use effects.read_chunk()/write_chunk() via StorageEffects
**Reference**: See [Effect System Guide](docs/106_effect_system_and_runtime.md#1-effect-traits-and-categories) and [Testing Guide](docs/805_testing_guide.md#effect-system-compliance-in-tests)

- [ ] Fix direct network access violations (multiple violations)
**Status**: URGENT
**Location**: aura-composition, aura-rendezvous, aura-transport
**Description**: Direct network socket usage instead of NetworkEffects
**Solution**: Use NetworkEffects trait for all network operations
**Reference**: See [Effect System Guide](docs/106_effect_system_and_runtime.md#1-effect-traits-and-categories) and [System Architecture](docs/001_system_architecture.md#35-impure-function-control)

#### 3.2 Guard Chain Implementation
- [ ] Implement missing guard chain components: AuthorizationEffects and LeakageEffects
**Status**: MISSING
**Location**: `aura-protocol/src/guards/`
**Description**: Guard chain requires authorization → flow → leakage → journal → transport
**Why Critical**: Guard chain enforces authorization policies and flow budgets
**Reference**: See [System Architecture](docs/001_system_architecture.md#2-guard-chain-and-flow-budget-system) and [Effect System Guide](docs/106_effect_system_and_runtime.md#8-guard-chain-and-leakage-integration)

#### 3.3 Architecture Layer Violations  
- [ ] Fix aura-composition handler implementations (should be composition utilities only)
**Status**: VIOLATION
**Description**: Contains individual handler implementations instead of composition patterns
**Reference**: See [System Architecture](docs/001_system_architecture.md#33-handler-registration-and-interoperability-system) and [Effect System Guide](docs/106_effect_system_and_runtime.md#5-layers-and-crates)

- [ ] Implement missing RelationalEffects domain handler in aura-relational crate
**Status**: MISSING
**Description**: Domain crate should own its application logic
**Reference**: See [System Architecture](docs/001_system_architecture.md#32-effect-trait-classification) and [Effect System Guide](docs/106_effect_system_and_runtime.md#1-effect-traits-and-categories)

- [ ] Fix aura-journal direct handler instantiation (use composition pattern)
**Status**: VIOLATION
**Location**: `FlowBudgetHandler::new()` in effects.rs
**Reference**: See [System Architecture](docs/001_system_architecture.md#33-handler-registration-and-interoperability-system) and [Effect System Guide](docs/106_effect_system_and_runtime.md#2-handler-design)

### 4. CLI & Integration Test Refactoring

#### 4.1 Re-enable and Refactor Integration Tests
- [ ] Re-enable and refactor integration tests
**Status**: DISABLED
**Files**:
- `aura-agent/tests/integration_tests.rs` (disabled)
- `aura-agent/tests/quick_keychain_test.rs` (disabled)
- `tests/integration/guard_chain.rs` (test commented out)

**Action**: Refactor to use current API structure
**Reference**: See [Testing Guide](docs/805_testing_guide.md#integration-testing) and [Effect System Guide](docs/106_effect_system_and_runtime.md#6-testing-and-simulation)

**Acceptance Criteria**:
- All tests compile without errors
- Tests run and pass deterministically
- Coverage validates new API usage

#### 4.2 Convert Tests to #[aura_test] Macro
- [ ] Convert all async tests to use #[aura_test] macro
**Status**: IN PROGRESS
**Scope**: All async tests across codebase (currently use #[tokio::test])
**Reference**: See [Testing Guide](docs/805_testing_guide.md#the-aura_test-macro) and [System Architecture](docs/001_system_architecture.md#35-impure-function-control)

**Files to Update**:
- `crates/aura-agent/src/runtime/effects.rs`
- `crates/aura-agent/src/handlers/sessions/metadata.rs`
- `crates/aura-agent/src/handlers/sessions/coordination.rs`
- And 5+ more

**Acceptance Criteria**:
- All async tests use #[aura_test]
- Consistent tracing and timeout enforcement
- Tests include proper error context

### 5. Simulator Wiring - High Priority (Demo Blockers)

### 6. Architecture Compliance (Low Priority)

#### 6.1 Test Pattern Improvements  
- [ ] Replace Effects::test() usage with TestFixtures pattern in aura-verify
**Location**: `aura-verify` (device.rs, threshold.rs)
**Description**: Tests using Effects::test() directly instead of TestFixtures
**Solution**: Use aura-testkit TestFixtures exclusively
**Reference**: See [Testing Guide](docs/805_testing_guide.md#test-fixtures) and [Effect System Guide](docs/106_effect_system_and_runtime.md#6-testing-and-simulation)

#### 6.2 Test Infrastructure Improvements
- [ ] Add internal test_utils.rs modules to reduce test duplication
**Location**: aura-journal, aura-wot, aura-verify, aura-store, aura-transport, aura-effects  
**Description**: Multiple crates have test modules that could benefit from shared test utilities
**Solution**: Create internal test_utils.rs files for code reuse
**Reference**: See [Testing Guide](docs/805_testing_guide.md#module-organization) and [System Architecture](docs/001_system_architecture.md#71-effect-handler-development)

#### 6.3 Protocol Completeness
- [ ] Complete aura-recovery protocol patterns implementation (1/3 patterns implemented)
**Status**: PARTIAL
**Description**: Recovery crate partially implements expected recovery patterns
**Solution**: Implement missing recovery protocol components
**Reference**: See [System Architecture](docs/001_system_architecture.md#72-protocol-design-patterns) and [Testing Guide](docs/805_testing_guide.md#integration-testing)

---

## 🟡 MEDIUM PRIORITY

**Simulator wiring extensions and protocol implementations. Less critical for demo but important for comprehensive testing.**

### 7. Simulator Wiring - Extended Protocol Support

#### 7.1 Session/Epoch Choreographies (4 items)
- [ ] Implement session/epoch choreographies
**Missing**:
- `session_establishment` - Full session init
- `session_operation` - In-session protocol
- `presence_ticket_distribution` - Ticket management
- `epoch_increment` - Epoch transitions

#### 7.2 CRDT/Journal Flows (6 items)
- [ ] Implement CRDT and journal flow choreographies
**Missing**:
- `crdt_init/initialization/update` - CRDT operations
- `journal_broadcast` - Journal dissemination
- `concurrent_ops` - Concurrent operation handling
- `journal_repair` - Repair from divergence
- `state_convergence` - Convergence validation
- Related: Anti-entropy flows

#### 7.3 Broadcast/Coordination (3 items)
- [ ] Implement broadcast and coordination choreographies
**Missing**:
- `broadcast_gather` - Gather phase
- `multi_round_protocol/coordination` - Multi-round support
- `group_initialization/group_broadcast` - Group formation

#### 7.4 Group Safety & Invariants (4 items)
- [ ] Implement group safety and invariant choreographies
**Missing**:
- `group_communication` - Safe group messaging
- `choreographic_safety` - Deadlock-freedom verification
- `threshold_security` - Threshold invariants
- `counter_init/increment` - Counter protocol

### 8. Agent Runtime Stubs

#### 8.1 Replace Runtime Adapter Stubs
- [ ] Implement runtime adapter components (tree ops, OTA, migration, choreography)
**Location**: `crates/aura-agent/src/runtime/`
**Status**: STUBS (tree.rs, ota_orchestration.rs, migration.rs contain minimal code)

**Components**:
1. **TreeOperations** - Merkle tree management
   - Tree updates from facts
   - Proof generation
   - Proof validation

2. **OtaOrchestrator** - Over-the-air update coordination
   - Update proposal handling
   - Device state tracking
   - Rollback mechanisms

3. **MigrationCoordinator** - State migration
   - Device switching
   - Key sharing during migration
   - Consistency maintenance

4. **ChoreographyAdapter** - Protocol adaptation
   - Multi-party coordination
   - Role projection
   - Message routing

#### 8.2 CLI Threshold/DKD Wiring
- [ ] Complete CLI threshold and DKD protocol wiring
**Location**: `crates/aura-cli/src/handlers/threshold.rs`
**Status**: PARTIAL (config parsing done, protocol wiring incomplete)

**Missing**:
- DKD protocol execution integration
- Real result extraction from effects
- Effect system bridging

### 9. Protocol & Coordination Handlers

#### 9.1 Guardian Recovery Enhancements
- [ ] Enhance guardian recovery with FROST and commitment verification
**Location**: `crates/aura-cli/src/handlers/recovery.rs`
**Issues**:
- FROST material handling (placeholder at line 687)
- Guardian mapping simplifications
- Commitment verification stubs

#### 9.2 Agent Handlers (auth, system)
- [ ] Implement agent authentication and system handlers
**Location**: `crates/aura-protocol/src/handlers/agent/`
**Issues**:
- Capability/token parsing simplified
- Journal/credential sync stubs
- Biometric authentication missing

#### 9.3 Effect System Handler Chains
- [ ] Implement effect system handler chains and relational effects
**Missing**:
- RelationalEffects domain handler
- Additional operation type support in deltas
- Data classification refinement

### 10. Test Infrastructure

#### 10.1 Effect Test Patterns
- [ ] Refactor tests to use TestFixtures pattern
**Issue**: Tests using Effects::test() directly instead of TestFixtures
**Files**: aura-verify/device.rs, threshold.rs
**Solution**: Use aura-testkit TestFixtures exclusively

#### 10.2 Effect System Compliance Validation
- [ ] Validate effect system compliance across handlers
**Tasks**:
- Review biometric authentication handlers
- Validate network monitoring implementation
- Verify metrics effect usage
- Check simulation handler implementations

---

## 🟢 LOW PRIORITY

**Documentation, optimizations, and non-blocking improvements. Can be addressed after core functionality complete.**

### 11. Documentation & Configuration

#### 11.1 Feature Documentation
- [ ] Complete aura-sync feature documentation: `aura-sync/src/lib.rs:L63`
- [ ] Create INTEGRATION.md guide: `aura-sync/src/lib.rs:L119`

#### 11.2 Configuration & Constants
- [ ] Replace magic numbers with named constants
- [ ] Add flow limit constants
- [ ] Complete environment detection

#### 11.3 Build & Environment
- [ ] Fix evaluator path for nix environment: `aura-quint/src/evaluator.rs:L58`
- [ ] Run full dependency analysis with cargo

### 12. Code Quality & Refactoring

#### 12.1 Macro Adoption
- [ ] Use #[aura_error_types] for structured errors
- [ ] Consider #[aura_effect_handlers] for handlers
- [ ] Use choreography! macro for manual async protocols

#### 12.2 Minor Implementation Issues
- [ ] Placeholder FROST nonce handling → real SigningShare generation
- [ ] Secure storage handlers → production implementations
- [ ] Simulation handlers → full trait implementations
- [ ] Compute actual parity data for storage
- [ ] Placeholder string conversions in journal reduction
- [ ] Replace RotateEpoch placeholder with SnapshotFact

### 13. Storage & Testing

#### 13.1 Storage Optimization
- [ ] Compute actual parity data: `crates/aura-store/src/chunk.rs:L255`

#### 13.2 Test Infrastructure
- [ ] Re-enable time/ledger effect tests
- [ ] Create actual test keypair (not dummy)
- [ ] Fix reduction pipeline leaf visibility
- [ ] Component restart trigger logic

#### 13.3 Testkit Compliance
- [ ] Validate AMP agent helpers placement
- [ ] Audit all aura-testkit usage

### 14. Enhanced User Experience (TUI) - POSTPONED

#### 14.1 Ratatui TUI Implementation
- [ ] Add ratatui dependency and create TUI module
- [ ] Implement Bob's TUI with all screens
- [ ] Create Alice's guardian TUI interface

### 15. Advanced Automation - POSTPONED

#### 15.1 Human-Agent Demo Mode
- [ ] Implement Bob as real user, Alice/Charlie automated
- [ ] Integrate simulator for automated agents
- [ ] Implement demo orchestration with TUI

#### 15.2 Demo Presentation Interface
- [ ] Create demo presentation with visual progression
- [ ] Add technical overlays and demo controls
