# Aura TODO List

This document tracks all unfinished work, placeholders, and architectural items requiring completion across the Aura codebase. Items are organized by priority and grouped by crate/module.

**Last Updated:** 2025-11-21 (Updated: Verified codebase status - many items completed or changed, file structure refactored)
**Total Items:** 145+ substantive work items (125+ completed across all phases, 11 codebase TODOs remaining in active work)
**Codebase Status:** 50 TODO/FIXME markers verified - 39 completed, 11 still requiring work (sync protocols, rendezvous/transport, effect integration)

## Priority Levels

- **🔴 CRITICAL:** Blocking functionality, safety issues, or architectural problems that prevent core features from working
- **🟠 HIGH:** Important features and significant TODOs that impact major functionality
- **🟡 MEDIUM:** Improvements, refactorings, and technical debt that should be addressed
- **🟢 LOW:** Nice-to-haves, optimizations, and minor improvements

---

**Journal**
- [x] Stabilize consensus choreography surface: Simplify run_consensus_choreography until real FROST wiring lands. Keep signatures consistent but fence off experimental code behind a feature flag (e.g., consensus_frost_full). That avoids breaking downstream while you iterate on signing shares/nonce handling. **COMPLETED**
- [x] Normalize AEAD/key handling: Derive nonces from header (already started) and define a single KDF path for AMP message keys. Move the XOR/AES-GCM placeholder into a crypto::amp helper so routes can be swapped without touching transport code. **COMPLETED**
- [x] Guard chain ergonomics: Provide a helper to build the AMP send guard (cap, flow cost, leakage) to avoid repeating guard construction in every call. This reduces lifetime/capture issues and centralizes flow charging. **COMPLETED**
- [x] Reduce doc/format noise: Run cargo fmt and add #![allow(missing_docs)] only where intentional, or add short field docs to AMP facts/headers to stop warning flood. This will make real errors surface sooner. **COMPLETED**
- [ ] Agent/simulator wiring: Move AMP agent helpers into aura-testkit to keep core agent code stable. Simulator scenarios should import via a small facade to keep the surface area contained.
- [x] Maintenance/GC policy: Define a clear GC policy for AMP checkpoints/bumps and document it near the reducer; add a helper to compute safe pruning boundaries to avoid accidental state loss. **COMPLETED**

### Placeholder/Simplified Implementations (From Codebase Scan)

These items denote functionality that is either a placeholder, a simplified version, or explicitly states a need for a more robust "real implementation."

**1. High Priority Items - Trait Interface Implementation**
- [ ] ParityComputationEffects: Parity computation interface (TODO: `crates/aura-store/src/chunk.rs` - placeholder parity generation)
- [ ] SystemServiceEffects: System service interface (crates/aura-cli no longer has handlers/node.rs - not applicable)

## 🟡 MEDIUM PRIORITY

> **Improvements, refactorings, and technical debt**

### TimeEffects Refactoring (Systematic)

- [ ] aura-effects: Refactor SimulatedTimeHandler to avoid direct Instant::now() calls (time.rs:L9 TODO comment present)
- [ ] aura-effects: Use TimeEffects and RandomEffects in monitoring.rs:L9 and metrics.rs:L9
- [ ] aura-agent: Migrate caching timing to TimeEffects (caching.rs has optimization tests disabled at L552)

### Incomplete Tasks

- [ ] aura-simulator: State snapshot/restoration incomplete (scenario.rs:L283 capture, L306 restore - TODO comments present)

### Configuration & Infrastructure Tasks

- [ ] aura-sync: Environment variable loading not implemented in config.rs (all config is hardcoded)
- [ ] aura-journal: Semilattice type aliases and synchronization functions pending handler implementation
- [ ] aura-testkit: Mock handler consolidation and real handler implementations pending
- [ ] aura-rendezvous: Relay connection support removed/not implemented (discovery.rs exists but no connection_manager.rs)
- [ ] aura-cli: CLI handlers refactored - handlers/node.rs no longer exists in current structure
- [ ] aura-protocol: Time and ledger effects test implementation (effect_handlers_test.rs likely removed or refactored)

### Architectural Refactoring - aura-protocol Layer Boundary Violations

**Background**: aura-protocol currently contains code that violates the 8-layer architecture boundaries, making it a "kitchen sink" rather than a focused orchestration layer.

**Reference**: See [System Architecture](docs/001_system_architecture.md) Section 1.3 "Layered Effect Architecture" and [Project Structure](docs/999_project_structure.md) Section "Architecture Principles" for layer boundary rules and code location guidance.

**Problem**: Per the architecture documentation, aura-protocol should be Layer 4 (Orchestration) containing only:
- Multi-handler coordination and multi-party operations 
- Guard chains (CapGuard, FlowGuard, JournalCoupler) and coordination primitives
- CRDT coordinators and distributed coordination patterns
- Aura Consensus integration

**Current violations**:
- Single-party handlers (Layer 3 code) mixed with orchestration
- Domain message types (Layer 2 code) scattered throughout  
- Test utilities that belong in Layer 8
- Upward dependencies to Layer 6 (runtime composition)

**Phase 1: Safe Moves (No Circular Dependencies)**

- [ ] **Move Biscuit authorization domain logic to proper Layer 2 location**
  - **Task**: Move `authorization.rs` to `aura-wot/src/biscuit/authorization.rs` 
  - **Rationale**: Contains `BiscuitAuthorizationBridge` domain logic for token evaluation per Architecture Section 5.3 "Effect System Integration" - authorization effects should be implemented as domain logic, not orchestration
  - **Architecture compliance**: Layer 2 (aura-wot) is designated for "Trust and authorization" with "Capability refinement, Biscuit token helpers" per Project Structure
  - **Safety**: aura-wot (Layer 2) doesn't depend on aura-protocol (Layer 4), so no circular dependencies
  - **File scope**: `aura-protocol/src/authorization.rs` → `aura-wot/src/biscuit/authorization.rs`

- [ ] **Move protocol message specifications to domain crates**
  - **Task**: Redistribute message type definitions from `aura-protocol/src/messages/` to appropriate domain crates
  - **Rationale**: Per Architecture Section 1.3, Layer 2 (domain crates) should contain specifications while Layer 4 should only contain coordination logic. Message types are protocol specifications, not coordination.
  - **Specific moves**:
    - `social_types.rs`, `social_rendezvous.rs` → `aura-transport/src/messages/` (transport domain per Project Structure)
    - `crypto/` → `aura-verify/src/messages/` (identity verification domain per Project Structure)
    - `common_envelope.rs`, `common_error.rs` → `aura-core/src/messages/` (foundation types per Architecture Section 1.3)
  - **Safety**: Pure data structures with serialization traits, no dependencies on aura-protocol coordination logic
  - **Update scope**: Update all imports across codebase to use new locations

- [ ] **Move test infrastructure to Layer 8 testing utilities**  
  - **Task**: Move test-specific handlers to `aura-testkit` 
  - **Rationale**: Per Project Structure, Layer 8 (aura-testkit) is designated for "Shared test fixtures, scenario builders, and property test helpers" - memory and mock handlers are testing infrastructure, not production orchestration
  - **Architecture compliance**: Violates Architecture Section 8.1 - effect implementations belong in aura-effects (Layer 3) or aura-testkit (Layer 8), not aura-protocol (Layer 4)
  - **Specific moves**:
    - `handlers/memory/` → `aura-testkit/src/handlers/memory/` (MemoryChoreographicHandler, MemoryLedgerHandler)
    - `handlers/mock.rs` → `aura-testkit/src/handlers/mock.rs` (MockHandler)
  - **Safety**: aura-testkit already depends on aura-protocol per dependency graph, so no circular dependencies
  - **Integration**: Enhance existing aura-testkit patterns (`CompositeTestHandler`, `TestEffectHandler`) with moved handlers

**Phase 2: Layer 6 Reference Cleanup**

- [ ] **Remove upward dependencies violating layer hierarchy**
  - **Task**: Remove deprecated references to `aura-agent` runtime types from `aura-protocol`
  - **Rationale**: Per Architecture Section 7.2 "Crate Boundary Rules": "No crate implements traits from a higher layer" - Layer 4 cannot depend on Layer 6
  - **Architecture compliance**: Enforces "No circular dependencies" principle and maintains clean layer separation
  - **Scope**: Clean up `effects/mod.rs` and `lib.rs` re-exports pointing to runtime composition types
  - **File locations**: Remove any `use aura_agent::` imports and deprecated re-exports

**Expected Outcome**: 

aura-protocol becomes ~40% smaller and properly focused on Layer 4 orchestration per Architecture Section 1.3:
- ✅ **Retained (proper Layer 4)**: Guard chain infrastructure (CapGuard → FlowGuard → JournalCoupler)
- ✅ **Retained (proper Layer 4)**: CRDT coordination and handler orchestration (`CrdtCoordinator`, `CompositeHandler`)
- ✅ **Retained (proper Layer 4)**: Multi-party consensus coordination and Aura Consensus integration  
- ✅ **Retained (proper Layer 4)**: Anti-entropy and distributed coordination patterns

**Architecture Benefits**:
- Proper layer separation per "Code Location Guidance" in Project Structure
- Elimination of "Anti-Patterns" listed in Architecture Section 7.3
- Compliance with "No circular dependencies" principle 
- Clear responsibility boundaries enabling independent testing and reusability

---

## 🟢 LOW PRIORITY

> **Nice-to-haves, optimizations, and minor improvements**

- [ ] E2E Tests: Fix CLI and structure validation in tests/e2e_cli_dkd_test.rs:L182-183, L483, L720
- [ ] Privacy Tests: Enhance correlation calculation in tests/privacy_contract_properties.rs:L487
- [ ] Logging: Replace direct timestamp access with TimeEffects in aura-effects logging.rs:L255
- [ ] Dependencies: Re-enable cryptography and secret_sharing when dependencies available (aura-agent, aura-journal Cargo.toml)
- [ ] Cache Tests: Re-enable cache optimization tests in aura-agent caching.rs:L552 after architecture stabilizes
- [ ] Simplified Implementations: Enhance aura-agent config/errors/operations, aura-journal errors/reduction, aura-rendezvous crypto
- [ ] Documentation: Uncomment privacy contract tests and helper functions pending type implementation

---

## 🟠 HIGH PRIORITY - Effect Injection Missing

> **Effect injection required to replace direct system calls with effect system**

### Core Protocol Effect Injection (Layer 4-5) - CRITICAL

**UUID Generation (RandomEffects needed):**
- [x] `aura-protocol/src/handlers/timeout_coordinator.rs:97` - Replace `Uuid::new_v4()` with RandomEffects for timeout handles **COMPLETED**
- [ ] `aura-protocol/src/handlers/context/context.rs:405` - Replace `uuid::Uuid::nil()` with RandomEffects for operation IDs (not actively used, low priority)
- [x] `aura-protocol/src/handlers/sync_anti_entropy.rs` - Replace direct UUID calls with deterministic test UUIDs **COMPLETED**
- [x] `aura-protocol/src/handlers/sync_broadcaster.rs` - Replace direct UUID calls with deterministic test UUIDs **COMPLETED**
- [x] `aura-protocol/src/handlers/bridges/unified_bridge.rs` - Remove stale lint suppression (no UUID calls present) **COMPLETED**
- [x] `aura-protocol/src/handlers/bridges/typed_bridge.rs` - Replace test UUID calls with deterministic UUIDs **COMPLETED**

**Time Operations (TimeEffects needed):**
- [ ] `aura-protocol/src/handlers/context/context.rs:272` - Replace `SystemTime::now()` with TimeEffects for session timestamps
- [ ] `aura-protocol/src/handlers/context/context.rs:396` - Replace direct timestamp with TimeEffects for production context
- [ ] `aura-protocol/src/handlers/context/context.rs:523` - Replace direct timestamp millis with TimeEffects
- [ ] `aura-protocol/src/handlers/agent/session.rs:70` - Add TimeEffects for session management
- [ ] `aura-protocol/src/guards/privacy.rs:7` - Replace direct time calls with TimeEffects (file-level TODO)

### Authentication & Authorization Effect Injection

**Network & Crypto Effects in Authentication:**
- [ ] `aura-authenticate/src/guardian_auth.rs:973` - Replace placeholder with NetworkEffects communication
- [ ] `aura-authenticate/src/guardian_auth.rs:1021` - Send challenge via NetworkEffects instead of placeholder
- [ ] `aura-authenticate/src/guardian_auth.rs:1041` - Send approval decision via NetworkEffects instead of placeholder  
- [ ] `aura-authenticate/src/guardian_auth.rs:1336` - Use actual NetworkEffects instead of placeholder
- [ ] `aura-authenticate/src/guardian_auth.rs:1406` - Use JournalEffects to persist authentication state

**Device ID & Identity Generation:**
- [ ] `aura-authenticate/src/guardian_auth.rs:24` - Replace test device ID generation with proper effect system generation

### Invitation & Recovery Protocol Effect Injection

**Invitation Flow Effects:**
- [ ] `aura-invitation/src/guardian_invitation.rs:135` - Use NetworkEffects to send invitation messages
- [ ] `aura-invitation/src/guardian_invitation.rs:147` - Use NetworkEffects to receive invitation responses
- [ ] `aura-invitation/src/guardian_invitation.rs:174` - Use CryptoEffects for signing and attestation exchange
- [ ] `aura-invitation/src/guardian_invitation.rs:201` - Use JournalEffects to record guardian relationships
- [ ] `aura-invitation/src/guardian_invitation.rs:222` - Use JournalEffects to record invitation rejections

**Recovery Protocol Effects:**
- [ ] `aura-recovery/src/recovery_protocol.rs:292` - Use NetworkEffects to send recovery messages
- [ ] `aura-recovery/src/recovery_protocol.rs:324` - Use NetworkEffects to send recovery results
- [ ] `aura-recovery/src/recovery_protocol.rs:349` - Use JournalEffects to record recovery state changes

### Sync & Storage Effect Injection

**Sync Protocol Time Effects:**
- [ ] `aura-sync/src/services/sync.rs:30` - Replace `Instant::now()` with TimeEffects for sync timing
- [ ] `aura-sync/src/services/sync.rs:270` - Obtain Instant via TimeEffects instead of direct call
- [ ] `aura-sync/src/services/sync.rs:274` - Obtain timestamp via TimeEffects instead of direct call
- [ ] `aura-sync/src/protocols/snapshots.rs:243` - Use RandomEffects for UUID generation instead of direct calls

**Journal & Storage Effects:**
- [ ] `aura-sync/src/protocols/anti_entropy.rs:597` - Convert operations to journal deltas via JournalEffects
- [ ] `aura-sync/src/protocols/authority_journal_sync.rs:175` - Load from storage via StorageEffects
- [ ] `aura-sync/src/protocols/authority_journal_sync.rs:195` - Get timestamp from TimeEffects

### Security & Biometric Effect Injection

**BiometricEffects Integration:**
- [ ] `aura-protocol/src/handlers/agent/auth.rs:10` - Use BiometricEffects for platform biometric API integration
- [ ] `aura-protocol/src/handlers/agent/auth.rs:244` - Replace simulation with real BiometricEffects platform integration

**SecureStorage Effects:**
- [ ] `aura-core/src/crypto/tree_signing.rs:104` - Use SecureStorageEffects to store nonces securely
- [ ] `aura-core/src/crypto/tree_signing.rs:352` - Use SecureStorageEffects to retrieve signing shares from secure storage
- [ ] `aura-core/src/crypto/tree_signing.rs:381` - Use SecureStorageEffects for secure nonce storage

### Guard & Effect System Integration

**Effect System Bridge Completion:**
- [ ] `aura-protocol/src/guards/effect_system_trait.rs:75` - Delegate device ID retrieval to actual effect system
- [ ] `aura-protocol/src/guards/effect_system_trait.rs:81` - Delegate context ID retrieval to actual effect system
- [ ] `aura-protocol/src/guards/effect_system_trait.rs:87` - Delegate context metadata to actual effect system
- [ ] `aura-protocol/src/guards/effect_system_trait.rs:93` - Check actual effect system capabilities
- [ ] `aura-protocol/src/guards/send_guard.rs:322` - Replace placeholder with actual token retrieval from effect system

**Testing Infrastructure Effect Integration:**
- [ ] `aura-testkit/src/verification/capabilities.rs:7` - Replace direct time calls with TimeEffects in testkit
- [ ] `aura-testkit/src/verification/capability_soundness.rs:7` - Replace direct time calls with TimeEffects in capability verification

---

## 📋 Codebase TODO Scan (50 items across 20 files - CURRENT STATUS)

**Completed Integration & Infrastructure (34 items):**
- ✅ Effect System (15): TimeEffects, RandomEffects, secure storage, Bloom filters, transport coordination integrated
- ✅ Authorization & Guards (10): Biscuit implementation, flow guards, journal coupling, E2E test blockers resolved
- ✅ Testkit & Verification (5): Mock handlers, test fixtures, TimeEffects for test timing
- ✅ Core & Verification (4): SecureStorageEffects, tree signing, identity verification, storage authorization

**Remaining Implementation Work (16 items):**
- [ ] Synchronization (10): Authority journal sync, maintenance URI support, signature verification, journal update via effects
  - `aura-sync/src/protocols/anti_entropy.rs:L232` - Get actual authority ID from peer registration
  - `aura-sync/src/protocols/anti_entropy.rs:L597, L602` - Convert operations to journal deltas via effects
  - `aura-sync/src/services/maintenance.rs` - URI support, epoch/fence mapping, signature verification
  - `aura-sync/src/protocols/authority_journal_sync.rs` - Storage loading, merkle root, network operations
- [ ] Rendezvous & Transport (6): Connection establishment, sender ID tracking, message processing
  - `aura-rendezvous/src/messaging/transport.rs` - Offer/answer processing, connection establishment
  - `aura-rendezvous/src/integration/connection.rs` - Connection management implementation
- [ ] Protocol & Authorization (4): Biscuit-based authorization integration, handler exports, effect system bridge

---

## 📋 UNTRACKED TODO Items (Found in Codebase but NOT in TODO.md)

**Scan Date:** 2025-11-22 - Found 70+ additional substantive work items in consensus, tree operations, sync, and rendezvous protocols

These are legitimate TODO items discovered in the codebase that should be tracked:

### 🔴 CRITICAL - New Untracked Items

**AMP Protocol & Consensus:**
- [ ] `aura-protocol/src/consensus/amp.rs:L84` - Evidence plumbing: integrate evidence deltas tracking per message provenance
- [ ] `aura-transport/src/amp.rs` - Empty transport layer file: implement AMP message protocol integration
- [ ] `aura-protocol/src/consensus/commit_fact.rs:L118` - Verify threshold signature once FROST integration complete
- [ ] `aura-journal/src/authority_state.rs:L30` - Implement actual FROST threshold signing coordination

### 🟠 HIGH - New Untracked Items (70+ items across consensus, trees, sync, rendezvous)

**Consensus & FROST Integration (12 items):**
- [ ] `aura-protocol/src/consensus/choreography.rs:L279` - Verify prestate matches witness view in choreography
- [ ] `aura-protocol/src/consensus/choreography.rs:L349` - Verify choreography result
- [ ] `aura-protocol/src/consensus/choreography.rs:L426` - Replace placeholder FROST nonce generation with real key shares
- [ ] `aura-protocol/src/consensus/coordinator.rs:L119` - Actually send consensus messages via transport
- [ ] `aura-protocol/src/consensus/coordinator.rs:L125` - Collect real nonce commitments instead of placeholder
- [ ] `aura-protocol/src/consensus/coordinator.rs:L133` - Send signature request with aggregated nonces
- [ ] `aura-protocol/src/consensus/coordinator.rs:L137` - Collect real signatures from participants
- [ ] `aura-protocol/src/consensus/coordinator.rs:L145` - Aggregate signatures using actual FROST
- [ ] `aura-protocol/src/consensus/coordinator.rs:L276-289` - Production gossip and network effects integration
- [ ] `aura-protocol/src/consensus/coordinator.rs:L345-367` - Real convergence checks and FROST aggregation
- [ ] `aura-protocol/src/consensus/witness.rs:L199-218` - FROST nonce commitment generation and signature collection
- [ ] `aura-protocol/src/consensus/witness.rs:L206` - Generate nonce commitment with actual FROST

**Tree Operations & Commitments (8 items):**
- [ ] `aura-journal/src/commitment_tree/authority_state.rs:L80` - Update tree structure and recompute commitments
- [ ] `aura-journal/src/commitment_tree/authority_state.rs:L137` - Implement tree rebalancing
- [ ] `aura-journal/src/commitment_tree/authority_state.rs:L169` - Invalidate cached key shares on tree changes
- [ ] `aura-journal/src/commitment_tree/authority_state.rs:L182` - Implement proper tree commitment computation
- [ ] `aura-journal/src/commitment_tree/authority_state.rs:L213` - Derive keys from tree structure
- [ ] `aura-journal/src/commitment_tree/application.rs:L448-449` - Track parent nodes for affected node calculation
- [ ] `aura-journal/src/commitment_tree/application.rs:L520` - Replace simplified recomputation with efficient tree updates
- [ ] `aura-journal/src/journal_api.rs:L100` - Derive device count from authority facts in TreeState

**Sync & Maintenance (10 items):**
- [ ] `aura-sync/src/services/maintenance.rs:L407` - Add URI support for artifacts
- [ ] `aura-sync/src/services/maintenance.rs:L408` - Map activation_epoch to IdentityEpochFence
- [ ] `aura-sync/src/services/maintenance.rs:L418` - Verify threshold signature during maintenance
- [ ] `aura-sync/src/services/sync.rs:L337` - Implement full journal_sync protocol integration
- [ ] `aura-sync/src/services/sync.rs:L353` - Implement peer synchronization using journal_sync
- [ ] `aura-sync/src/services/sync.rs:L526` - Implement actual peer synchronization via journal_sync
- [ ] `aura-sync/src/services/sync.rs:L387` - Track last_sync from metrics
- [ ] `aura-sync/src/services/sync.rs:L402-404` - Populate sync metrics (requests_processed, errors, avg_latency)
- [ ] `aura-sync/src/services/maintenance.rs:L467, L486, L500-501` - Implement background task management and operation cleanup

**Rendezvous & Transport (13 items):**
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L269` - Implement actual encryption using context keys
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L286` - Implement guard chain serialization
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L292` - Implement guard chain evaluation
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L320` - Implement message forwarding via effects
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L334-341` - Implement receipt signing and validation
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L365` - Add facts to appropriate journal namespace
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L372` - Implement cache cleanup based on timestamps
- [ ] `aura-rendezvous/src/context/rendezvous.rs:L420` - Implement actual transport connection establishment
- [ ] `aura-sync/src/infrastructure/peers.rs:L291` - Integrate with aura-rendezvous DiscoveryService
- [ ] `aura-sync/src/infrastructure/connections.rs:L317` - Integrate with aura-transport to establish connection
- [ ] `aura-sync/src/infrastructure/connections.rs:L374, L394` - Actually close connection via aura-transport
- [ ] `aura-sync/src/protocols/namespaced_sync.rs:L134, L146` - Implement authorization checks
- [ ] `aura-sync/src/protocols/namespaced_sync.rs:L174` - Implement pagination in sync protocol

**Peer & Connection Management (6 items):**
- [ ] `aura-sync/src/infrastructure/peers.rs:L348` - Proper token validation with root public key
- [ ] `aura-sync/src/protocols/namespaced_sync.rs:L250` - Get page_size from effects or configuration
- [ ] `aura-sync/src/protocols/namespaced_sync.rs:L261` - Implement actual network exchange
- [ ] `aura-rendezvous/src/sbb/flooding.rs:L313` - Integrate with transport layer
- [ ] `aura-journal/src/commitment_tree/attested_ops.rs:L40-42` - Refactor ID generation to avoid random generation in From trait

**Authorization & Operations (aura-agent):**
- `crates/aura-agent/src/operations.rs:L229-368` - Multiple TODOs for handler integration (storage, journal, session, auth)
- `crates/aura-agent/src/operations.rs:L250, L258` - Implement full Biscuit token authorization
- `crates/aura-agent/src/agent.rs:L39, L90, L96` - Multi-device authority lookup and handling
- [deferred] `crates/aura-agent/src/agent.rs:L219-233` - Placeholder device status (hardware_security, attestation, storage_usage, sync timing, device_name, storage_limit)

**Protocol Guards & Authorization (aura-protocol):**
- `crates/aura-protocol/src/guards/capability_guard.rs:L189` - Extract delegation_depth from Biscuit evaluation
- `crates/aura-protocol/src/guards/mod.rs:L88` - Use proper Biscuit token type instead of Vec<String>
- `crates/aura-protocol/src/guards/execution.rs:L185` - Replace with actual Biscuit token creation
- `crates/aura-protocol/src/guards/effect_system_trait.rs:L75, L81, L87, L93` - Delegate to actual effect system
- `crates/aura-protocol/src/guards/effect_system_bridge.rs:L15` - Implement Biscuit-based authorization integration
- `crates/aura-protocol/src/guards/send_guard.rs:L325` - Replace with actual token retrieval from effect system

**Handler/Coordinator TODOs (aura-protocol):**
- `crates/aura-protocol/src/handlers/timeout_coordinator.rs:L96` - Refactor to use RandomEffects for UUID generation
- `crates/aura-protocol/src/handlers/sync_anti_entropy.rs:L205` - Log vs. execute anti-entropy operations
- `crates/aura-protocol/src/handlers/agent/system.rs:L106, L268` - Sync with distributed journal, decrypt/restore credentials
- `crates/aura-protocol/src/handlers/agent/auth.rs:L301, L307, L338` - Parse/verify capability tokens, proper device attestation
- `crates/aura-protocol/src/guards/deltas.rs:L782` - Support for additional operation types in delta application
- `crates/aura-protocol/src/guards/privacy.rs:L254` - Data classification refinement beyond simplified version

**Rendezvous & Transport (aura-rendezvous):**
- `crates/aura-rendezvous/src/messaging/transport.rs:L298, L316, L326, L409, L454, L499` - Process offers/answers, establish connections, sender ID tracking, accessors
- `crates/aura-rendezvous/src/integration/connection.rs:L245` - Binary STUN encoding per RFC 5389
- `crates/aura-rendezvous/src/discovery.rs:L547` - Filtering rendezvous points

**Journal & Trees (aura-journal):**
- `crates/aura-journal/src/authority_state.rs:L51` - Implement actual FROST threshold signing coordination
- `crates/aura-journal/src/semilattice/mod.rs:L33, L57, L70, L79, L92, L105, L123` - Type aliases, CRDT modules, synchronization functions, choreographic runtime
- `crates/aura-journal/src/semilattice/journal_map.rs:L286, L294` - Filter to only devices, count guardians separately
- `crates/aura-journal/src/semilattice/types.rs:L102` - Full OR-Set implementation
- `crates/aura-journal/src/commitment_tree/authority_state.rs:L80, L137, L169, L182, L213` - Tree updates, rebalancing, cache invalidation, commitment computation, key derivation
- `crates/aura-core/src/authority.rs:L65, L70, L94, L111` - Replace facts field with Fact type, implement tree derivation and updates

**Core/Crypto (aura-core):**
- `crates/aura-core/src/crypto/tree_signing.rs:L27, L104, L352, L381` - SecureStorageEffects integration for nonces and signing shares
- `crates/aura-core/src/conversions.rs:L129` - Tests using old Cap API (commented out)
- `crates/aura-core/src/journal.rs:L826` - Tests using old Journal API (commented out)

**Time Management (aura-agent):**
- `crates/aura-agent/src/runtime/context.rs:L7` - Refactor to use TimeEffects for context timing
- `crates/aura-agent/src/optimizations/caching.rs:L6, L165, L229, L249, L270, L290, L313, L335, L357` - Use TimeEffects for cache timing (multiple locations)

**Test Infrastructure (aura-protocol, aura-authenticate):**
- `crates/aura-protocol/tests/effect_handlers_test.rs:L204, L236` - Re-enable time and ledger effects tests
- `crates/aura-protocol/tests/common/helpers.rs:L74` - Create actual test keypair instead of dummy
- `crates/aura-protocol/tests/tree_scalability.rs:L71` - Replace placeholder verification
- `crates/aura-protocol/tests/tree_operations.rs:L118, L177` - Proper function verification
- `crates/aura-protocol/tests/tree_chaos.rs:L81` - Proper structure rejection verification
- `crates/aura-authenticate/src/device_auth.rs:L513` - Re-enable test disabled due to effect system builder issues
- `tests/integration/guard_chain.rs:L59` - Update to work with new choreography architecture
- `tests/e2e/01_authority_lifecycle.rs:L51` - Fix reduction pipeline leaf visibility issue

**Module Re-exports (aura-agent, aura-protocol):**
- `crates/aura-agent/src/handlers/mod.rs:L14, L21` - Re-export agent handlers from aura-protocol

**Handler Modules:**
- `crates/aura-effects/src/system/monitoring.rs:L1165` - Component restart trigger logic

### 🟡 MEDIUM - New Untracked Items

**CLI & Stubs (3 items):**
- [ ] `aura-cli/src/commands/amp.rs:L1` - Implement AMP CLI commands (currently rough stub with only action definitions)
- [ ] `aura-agent/tests/integration_tests.rs:L16` - Refactor tests to use current API structure
- [ ] `aura-agent/tests/quick_keychain_test.rs:L26` - Re-implement using current API structure

**Rendezvous & Integration (4 items):**
- [ ] `aura-rendezvous/tests/integration_tests.rs:L171, L219` - Update tests to use add_peer_for_context with proper ContextId
- [ ] `aura-rendezvous/src/integration/sbb_system.rs:L207` - Complete full encrypted SBB flooding implementation
- [ ] `aura-journal/src/commitment_tree/compaction.rs:L71` - Replace RotateEpoch placeholder with proper SnapshotFact

### 🟢 LOW - New Untracked Items

**Documentation & Configuration (3 items):**
- [ ] `aura-sync/src/lib.rs:L63` - Complete documentation before re-enabling this feature
- [ ] `aura-sync/src/lib.rs:L119` - Create INTEGRATION.md documentation file
- [ ] `aura-quint/src/evaluator.rs:L58` - Fix evaluator path to use nix environment provided version

**Placeholder Conversions (2 items):**
- [ ] `aura-journal/src/reduction.rs:L253, L318` - Replace placeholder string conversions during reduction
- [ ] `aura-journal/src/journal_api.rs` - Refactor state snapshot/restoration (scenario.rs:L283 capture, L306 restore)

### Low Priority Placeholders NOT Tracked (OLDER ITEMS)

**Storage & Chunks (aura-store):**
- `crates/aura-store/src/chunk.rs:L255` - Compute actual parity data

**Disabled/Incomplete Test Features:**
- `crates/aura-journal/src/commitment_integration.rs:L12` - Conversion implementations disabled (incompatible type systems)
- `crates/aura-authenticate/src/device_auth.rs:L513` - Device auth test disabled
- `crates/aura-protocol/src/handlers/sync_broadcaster.rs:L301, L384` - test_eager_push_disabled, test_lazy_pull_disabled

---

## Summary Statistics

| Priority | Count | Status |
|----------|-------|--------|
| 🔴 Critical (Architecture) | 9 | ✅ COMPLETE |
| 🔴 Critical (E2E Test Blockers) | 7 | ✅ COMPLETE |
| 🔴 Critical (NEW - Consensus/AMP) | 4 | 📝 PENDING |
| 🟠 High (Deprecated Code Removal) | 93 | ✅ COMPLETE |
| 🟠 High (Effect Injection Missing) | 37 | 📝 PENDING |
| 🟠 High (NEW - Consensus/Trees/Sync/Rendezvous) | 59 | 📝 PENDING |
| 🟡 Medium (Features & Refactoring) | 35 | ⏳ IN PROGRESS (16 codebase TODOs remaining) |
| 🟡 Medium (NEW - CLI/Tests/Integration) | 7 | 📝 PENDING |
| 🟢 Low (Nice-to-Haves) | ~15 | 📝 PENDING |
| 🟢 Low (NEW - Documentation/Config) | 5 | 📝 PENDING |
| **Total Completed** | **120+** | **Core infrastructure, architecture, deprecation cleanup, E2E blockers, 34 codebase items** |
| **Total Remaining (Previous)** | **68** | **37 effect injection + 16 sync/protocol + 15 low priority** |
| **Total Remaining (With NEW)** | **+75** | **4 critical + 59 high + 7 medium + 5 low** |
| **GRAND TOTAL REMAINING** | **~143** | **Comprehensive work backlog including consensus implementation, tree operations, full sync integration, rendezvous protocols** |

**Key Findings from Code Review:**
- DeviceAttestationEffects: ✅ Implemented (was marked incomplete)
- File structure changes: Many referenced paths no longer exist (handlers/node.rs, connection_manager.rs, etc.)
- Core work remaining: Synchronization protocol journal integration (10), rendezvous transport implementation (6), TimeEffects refactoring (5)
- Most infrastructure complete: Effect system, authorization, guard chain, testkit all working
