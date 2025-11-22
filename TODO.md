# Aura TODO List

This document tracks all unfinished work, placeholders, and architectural items requiring completion across the Aura codebase. Items are organized by priority and grouped by related work areas.

**Last Updated:** 2025-11-22
**Total Items:** 143+ work items remaining across all priorities
**Codebase Status:** 50 TODO/FIXME markers verified - 41 completed, 9+ still requiring work

## Priority Levels

- **🔴 CRITICAL:** Blocking functionality, safety issues, or architectural problems
- **🟠 HIGH:** Important features impacting major functionality
- **🟡 MEDIUM:** Improvements, refactorings, and technical debt
- **🟢 LOW:** Nice-to-haves, optimizations, and minor improvements

---

## 🔴 CRITICAL PRIORITY

### AMP Protocol & Consensus Integration

- [x] Evidence plumbing: `aura-protocol/src/consensus/amp.rs:L84` - Integrate evidence deltas tracking per message provenance
- [x] Empty transport layer: `aura-transport/src/amp.rs` - Implement AMP message protocol integration
- [x] Threshold signature verification: `aura-protocol/src/consensus/commit_fact.rs:L118` - Verify once FROST integration complete
- [x] FROST threshold signing coordination: `aura-journal/src/authority_state.rs:L30` - Implement actual coordination

---

## 🟠 HIGH PRIORITY

### Consensus & FROST Integration (12 items)

- [x] Verify prestate matches witness view: `aura-protocol/src/consensus/choreography.rs:L279`
- [x] Verify choreography result: `aura-protocol/src/consensus/choreography.rs:L349`
- [x] Replace placeholder FROST nonce generation: `aura-protocol/src/consensus/choreography.rs:L426`
- [x] Send consensus messages via transport: `aura-protocol/src/consensus/coordinator.rs:L119`
- [x] Collect real nonce commitments: `aura-protocol/src/consensus/coordinator.rs:L125`
- [x] Send signature request with aggregated nonces: `aura-protocol/src/consensus/coordinator.rs:L133`
- [x] Collect real signatures from participants: `aura-protocol/src/consensus/coordinator.rs:L137`
- [x] Aggregate signatures using actual FROST: `aura-protocol/src/consensus/coordinator.rs:L145`
- [x] Production gossip and network effects: `aura-protocol/src/consensus/coordinator.rs:L276-289`
- [x] Real convergence checks and FROST aggregation: `aura-protocol/src/consensus/coordinator.rs:L345-367`
- [x] FROST nonce commitment generation: `aura-protocol/src/consensus/witness.rs:L199-218`
- [x] Generate nonce commitment with actual FROST: `aura-protocol/src/consensus/witness.rs:L206`

### Tree Operations & Commitments (8 items)

- [x] Update tree structure and recompute commitments: `aura-journal/src/commitment_tree/authority_state.rs:L80`
- [x] Implement tree rebalancing: `aura-journal/src/commitment_tree/authority_state.rs:L137`
- [x] Invalidate cached key shares on tree changes: `aura-journal/src/commitment_tree/authority_state.rs:L169`
- [x] Implement proper tree commitment computation: `aura-journal/src/commitment_tree/authority_state.rs:L182`
- [x] Derive keys from tree structure: `aura-journal/src/commitment_tree/authority_state.rs:L213`
- [x] Track parent nodes for affected node calculation: `aura-journal/src/commitment_tree/application.rs:L448-449`
- [x] Replace simplified recomputation with efficient tree updates: `aura-journal/src/commitment_tree/application.rs:L520`
- [x] Derive device count from authority facts in TreeState: `aura-journal/src/journal_api.rs:L100`

### Synchronization Protocol Integration (10 items)

- [ ] Get actual authority ID from peer registration: `aura-sync/src/protocols/anti_entropy.rs:L232`
- [ ] Convert operations to journal deltas via effects: `aura-sync/src/protocols/anti_entropy.rs:L597, L602`
- [ ] Add URI support for artifacts: `aura-sync/src/services/maintenance.rs:L407`
- [ ] Map activation_epoch to IdentityEpochFence: `aura-sync/src/services/maintenance.rs:L408`
- [ ] Verify threshold signature during maintenance: `aura-sync/src/services/maintenance.rs:L418`
- [ ] Implement full journal_sync protocol integration: `aura-sync/src/services/sync.rs:L337`
- [ ] Implement peer synchronization using journal_sync: `aura-sync/src/services/sync.rs:L353`
- [ ] Implement actual peer synchronization via journal_sync: `aura-sync/src/services/sync.rs:L526`
- [ ] Track last_sync from metrics: `aura-sync/src/services/sync.rs:L387`
- [ ] Populate sync metrics: `aura-sync/src/services/sync.rs:L402-404` (requests_processed, errors, avg_latency)

### Rendezvous & Transport Implementation (13 items)

- [ ] Implement actual encryption using context keys: `aura-rendezvous/src/context/rendezvous.rs:L269`
- [ ] Implement guard chain serialization: `aura-rendezvous/src/context/rendezvous.rs:L286`
- [ ] Implement guard chain evaluation: `aura-rendezvous/src/context/rendezvous.rs:L292`
- [ ] Implement message forwarding via effects: `aura-rendezvous/src/context/rendezvous.rs:L320`
- [ ] Implement receipt signing and validation: `aura-rendezvous/src/context/rendezvous.rs:L334-341`
- [ ] Implement actual transport connection establishment: `aura-rendezvous/src/context/rendezvous.rs:L420`
- [ ] Integrate with aura-rendezvous DiscoveryService: `aura-sync/src/infrastructure/peers.rs:L291`
- [ ] Integrate with aura-transport to establish connection: `aura-sync/src/infrastructure/connections.rs:L317`
- [ ] Close connection via aura-transport: `aura-sync/src/infrastructure/connections.rs:L374, L394`
- [ ] Implement authorization checks: `aura-sync/src/protocols/namespaced_sync.rs:L134, L146`
- [ ] Implement pagination in sync protocol: `aura-sync/src/protocols/namespaced_sync.rs:L174`
- [ ] Process offers/answers and establish connections: `crates/aura-rendezvous/src/messaging/transport.rs:L298, L316, L326, L409, L454, L499`
- [ ] Binary STUN encoding per RFC 5389: `crates/aura-rendezvous/src/integration/connection.rs:L245`

### Authorization & Biscuit Integration (6 items)

- [ ] Extract delegation_depth from Biscuit evaluation: `crates/aura-protocol/src/guards/capability_guard.rs:L189`
- [ ] Use proper Biscuit token type instead of Vec<String>: `crates/aura-protocol/src/guards/mod.rs:L88`
- [ ] Replace with actual Biscuit token creation: `crates/aura-protocol/src/guards/execution.rs:L185`
- [ ] Delegate to actual effect system: `crates/aura-protocol/src/guards/effect_system_trait.rs:L75, L81, L87, L93`
- [ ] Implement Biscuit-based authorization integration: `crates/aura-protocol/src/guards/effect_system_bridge.rs:L15`
- [ ] Replace placeholder with actual token retrieval: `crates/aura-protocol/src/guards/send_guard.rs:L325`

### Time Operations Refactoring (5 items)

- [ ] Replace SystemTime::now() with TimeEffects: `aura-protocol/src/handlers/context/context.rs:L272, L396, L523`
- [ ] Add TimeEffects for session management: `aura-protocol/src/handlers/agent/session.rs:L70`
- [ ] Replace direct time calls with TimeEffects: `aura-protocol/src/guards/privacy.rs:L7`

### Peer & Connection Management (4 items)

- [ ] Proper token validation with root public key: `aura-sync/src/infrastructure/peers.rs:L348`
- [ ] Get page_size from effects or configuration: `aura-sync/src/protocols/namespaced_sync.rs:L250`
- [ ] Implement actual network exchange: `aura-sync/src/protocols/namespaced_sync.rs:L261`
- [ ] Integrate with transport layer: `aura-rendezvous/src/sbb/flooding.rs:L313`

### Journal & Semilattice (7 items)

- [ ] Implement actual FROST threshold signing coordination: `crates/aura-journal/src/authority_state.rs:L51`
- [ ] Type aliases and CRDT modules: `crates/aura-journal/src/semilattice/mod.rs:L33, L57, L70, L79, L92, L105, L123`
- [ ] Filter to only devices, count guardians separately: `crates/aura-journal/src/semilattice/journal_map.rs:L286, L294`
- [ ] Full OR-Set implementation: `crates/aura-journal/src/semilattice/types.rs:L102`
- [ ] Tree updates, rebalancing, cache invalidation: `crates/aura-journal/src/commitment_tree/authority_state.rs:L80, L137, L169, L182, L213`
- [ ] Replace facts field with Fact type: `crates/aura-core/src/authority.rs:L65, L70, L94, L111`
- [ ] Refactor ID generation to avoid random in From trait: `aura-journal/src/commitment_tree/attested_ops.rs:L40-42`

---

## 🟡 MEDIUM PRIORITY

### Architectural Refactoring - aura-protocol Layer Boundary Violations

Per architecture docs, Layer 4 (aura-protocol/Orchestration) should only contain multi-party operations. Currently violates boundaries.

**Phase 1: Safe Moves (No Circular Dependencies)**

- [ ] Move Biscuit authorization domain logic to `aura-wot/src/biscuit/authorization.rs`
  - From: `aura-protocol/src/authorization.rs`
  - Rationale: Domain logic belongs in Layer 2, not Layer 4 orchestration
- [ ] Redistribute message types from `aura-protocol/src/messages/`:
  - [ ] `social_types.rs`, `social_rendezvous.rs` → `aura-transport/src/messages/`
  - [ ] `crypto/` → `aura-verify/src/messages/`
  - [ ] `common_envelope.rs`, `common_error.rs` → `aura-core/src/messages/`
- [ ] Move test infrastructure to `aura-testkit`:
  - [ ] `handlers/memory/` → `aura-testkit/src/handlers/memory/`
  - [ ] `handlers/mock.rs` → `aura-testkit/src/handlers/mock.rs`

**Phase 2: Layer 6 Reference Cleanup**

- [ ] Remove upward dependencies from `aura-protocol` to `aura-agent` runtime types

### aura-relational Layer 5 Reorganization

Per architectural analysis, aura-relational needs reorganization to maintain proper Layer 5 boundaries by moving domain types to Layer 1 and consensus logic to Layer 4.

**Phase 1: Move Domain Types to aura-core**

- [x] Move `RelationalFact` enum to `aura-core/src/relational/fact.rs`
  - From: `aura-relational/src/lib.rs`
  - Success criteria: aura-core exports RelationalFact, GuardianBinding, RecoveryGrant variants
- [x] Move `GuardianBinding` struct to `aura-core/src/relational/guardian.rs`
  - From: `aura-relational/src/guardian.rs`
  - Success criteria: Pure domain type with no protocol logic, only data structure
- [x] Move `GuardianParameters` struct to `aura-core/src/relational/guardian.rs`
  - From: `aura-relational/src/guardian.rs`  
  - Success criteria: Configuration parameters (recovery_delay, notification_required, expiration)
- [x] Move `RecoveryGrant` struct to `aura-core/src/relational/recovery.rs`
  - From: `aura-relational/src/guardian.rs`
  - Success criteria: Pure domain type with account commitments and operation data
- [x] Move `RecoveryOp` enum to `aura-core/src/relational/recovery.rs`
  - From: `aura-relational/src/guardian.rs`
  - Success criteria: Operation types (ReplaceTree, AddDevice, etc.) with no implementation
- [x] Move `ConsensusProof` struct to `aura-core/src/relational/consensus.rs`
  - From: `aura-relational/src/consensus.rs`
  - Success criteria: Pure data structure with prestate_hash, operation_hash, signature, attesters
- [x] Create `aura-core/src/relational/mod.rs` module with public exports
  - Success criteria: All relational types available via `use aura_core::relational::*`

**Phase 2: Move Consensus Implementation to aura-protocol**

- [ ] Move `run_consensus()` function to `aura-protocol/src/relational_consensus.rs`
  - From: `aura-relational/src/consensus.rs`
  - Success criteria: Implementation delegates to existing consensus infrastructure
- [ ] Move `run_consensus_with_config()` function to `aura-protocol/src/relational_consensus.rs`
  - From: `aura-relational/src/consensus.rs`
  - Success criteria: Configuration-driven consensus with timeout and witness set management
- [ ] Move `ConsensusConfig` struct to `aura-protocol/src/relational_consensus.rs`
  - From: `aura-relational/src/consensus.rs`
  - Success criteria: Orchestration config (threshold, witnesses, timeout) in Layer 4
- [ ] Create thin consensus adapter in `aura-relational/src/consensus_adapter.rs`
  - Success criteria: Delegates to aura-protocol, no implementation logic in aura-relational

**Phase 3: Consolidate Guardian Types from aura-recovery**

- [ ] Audit `aura-recovery/src/` for duplicate guardian types
  - Success criteria: List all guardian-related types and their overlap with aura-relational
- [ ] Move `GuardianProfile` from aura-recovery to aura-core if different from GuardianBinding
  - Success criteria: Single authoritative guardian type hierarchy
- [ ] Consolidate guardian authentication logic in `aura-relational/src/authentication.rs`
  - From: `aura-authenticate/src/guardian_auth_relational.rs`
  - Success criteria: Guardian auth protocols centralized in relational crate

**Phase 4: Update Dependencies and Imports**

- [ ] Update aura-relational Cargo.toml dependencies
  - Add: `aura-protocol = { path = "../aura-protocol" }`
  - Success criteria: Can depend on aura-protocol without circular dependencies
- [ ] Update all import statements across codebase
  - Change: `use aura_relational::{GuardianBinding, ConsensusProof}` 
  - To: `use aura_core::relational::{GuardianBinding, ConsensusProof}`
  - Success criteria: All imports work, no compilation errors
- [ ] Update aura-recovery to use aura-core relational types
  - Success criteria: No duplicate guardian types, imports from aura-core
- [ ] Update aura-authenticate to use aura-core relational types
  - Success criteria: Guardian auth uses consolidated types from aura-core

**Phase 5: Verification and Cleanup**

- [ ] Run full test suite to verify no regressions
  - Success criteria: All existing tests pass with new structure
- [ ] Update documentation in `docs/103_relational_contexts.md`
  - Success criteria: Implementation section reflects new crate organization
- [ ] Update `docs/999_project_structure.md` Layer 5 description for aura-relational
  - Success criteria: Describes aura-relational as pure protocol implementation without domain types
- [ ] Verify proper layer boundaries maintained
  - Success criteria: Layer 1 (aura-core) has types, Layer 4 has consensus, Layer 5 has protocols

### Effect System Time & Configuration (5 items)

- [ ] Refactor SimulatedTimeHandler to avoid direct Instant::now() calls: `aura-effects/time.rs:L9`
- [ ] Use TimeEffects and RandomEffects in monitoring: `aura-effects/monitoring.rs:L9`
- [ ] Use TimeEffects and RandomEffects in metrics: `aura-effects/metrics.rs:L9`
- [ ] Migrate caching timing to TimeEffects: `aura-agent/caching.rs:L552`
- [ ] Environment variable loading in aura-sync config: `aura-sync/src/config.rs`

### Incomplete Integration Tasks (4 items)

- [ ] State snapshot/restoration in aura-simulator: `scenario.rs:L283 capture, L306 restore`
- [ ] Support for additional operation types in delta application: `crates/aura-protocol/src/guards/deltas.rs:L782`
- [ ] Data classification refinement: `crates/aura-protocol/src/guards/privacy.rs:L254`
- [ ] Implement background task management and operation cleanup: `aura-sync/src/services/maintenance.rs:L467, L486, L500-501`

### CLI & Integration Tests (3 items)

- [ ] Implement AMP CLI commands: `aura-cli/src/commands/amp.rs:L1` (currently rough stub)
- [ ] Refactor tests to use current API: `aura-agent/tests/integration_tests.rs:L16`
- [ ] Re-implement using current API: `aura-agent/tests/quick_keychain_test.rs:L26`

### Rendezvous & SBB Integration (3 items)

- [ ] Update tests with proper ContextId: `aura-rendezvous/tests/integration_tests.rs:L171, L219`
- [ ] Complete full encrypted SBB flooding: `aura-rendezvous/src/integration/sbb_system.rs:L207`
- [ ] Filtering rendezvous points: `crates/aura-rendezvous/src/discovery.rs:L547`

---

## 🟢 LOW PRIORITY

### Storage & Parity (1 item)

- [ ] Compute actual parity data: `crates/aura-store/src/chunk.rs:L255`

### Documentation & Configuration (3 items)

- [ ] Complete documentation for aura-sync feature: `aura-sync/src/lib.rs:L63`
- [ ] Create INTEGRATION.md documentation file: `aura-sync/src/lib.rs:L119`
- [ ] Fix evaluator path for nix environment: `aura-quint/src/evaluator.rs:L58`

### Placeholder Conversions & State Handling (2 items)

- [ ] Replace placeholder string conversions during reduction: `aura-journal/src/reduction.rs:L253, L318`
- [ ] Replace RotateEpoch placeholder with proper SnapshotFact: `aura-journal/src/commitment_tree/compaction.rs:L71`

### Test Infrastructure & Verification (5 items)

- [ ] Re-enable time and ledger effects tests: `crates/aura-protocol/tests/effect_handlers_test.rs:L204, L236`
- [ ] Create actual test keypair instead of dummy: `crates/aura-protocol/tests/common/helpers.rs:L74`
- [ ] Update guard_chain tests for new choreography: `tests/integration/guard_chain.rs:L59`
- [ ] Fix reduction pipeline leaf visibility: `tests/e2e/01_authority_lifecycle.rs:L51`
- [ ] Component restart trigger logic: `crates/aura-effects/src/system/monitoring.rs:L1165`

### Agent/Simulator Refactoring (1 item)

- [ ] Move AMP agent helpers into aura-testkit: Keep core agent stable, simulator scenarios import via facade

---

## Summary

**Total Work Items:** 143+ remaining

**By Priority:**
- 🔴 **Critical:** 4 items (AMP/Consensus)
- 🟠 **High:** 59 items (Consensus, trees, sync, rendezvous, authorization, time)
- 🟡 **Medium:** 18 items (Architecture refactoring, configuration, tests, integration)
- 🟢 **Low:** 12 items (Storage, docs, placeholders, test infrastructure)

**Key Remaining Work:**
- Consensus/FROST: 12 items
- Tree operations: 8 items
- Sync integration: 10 items
- Rendezvous/transport: 13 items
- Authorization/Biscuit: 6 items
- Architecture refactoring: 7 items
- Effect system refactoring: 5 items
