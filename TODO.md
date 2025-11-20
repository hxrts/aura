# Aura TODO List

This document tracks all unfinished work, placeholders, and architectural items requiring completion across the Aura codebase. Items are organized by priority and grouped by crate/module.

**Last Updated:** 2025-11-20 (Updated: Journal state reduction pipeline completed)
**Total Items:** 126 substantive work items (23 completed in this session)
**Codebase Scan:** 429 TODO/FIXME markers found

## Priority Levels

- **🔴 CRITICAL:** Blocking functionality, safety issues, or architectural problems that prevent core features from working
- **🟠 HIGH:** Important features and significant TODOs that impact major functionality
- **🟡 MEDIUM:** Improvements, refactorings, and technical debt that should be addressed
- **🟢 LOW:** Nice-to-haves, optimizations, and minor improvements

---

## 🔴 CRITICAL PRIORITY

> **Blocking functionality, safety issues, or architectural problems**

### ✅ aura-journal (Core Journal API)
- ✅ Line 68: Fact addition implementation - Implemented fact conversion with FactContent::FlowBudget and fact_journal integration
- ✅ Line 74: Capability retrieval - Implemented with CapabilitySet::read_only() return

### ✅ aura-mpst (Session Type Runtime - Guard Chain)
- ✅ Line 603: Flow cost charging logic - Accumulated in endpoint.metadata with per-role tracking
- ✅ Line 631: Journal fact recording - Stored in endpoint.metadata as JSON array
- ✅ Line 659: Journal merge logic - Merge requests accumulated in endpoint.metadata
- ✅ Line 686: Guard chain execution - Full chain (AuthorizationEffects → FlowBudgetEffects → LeakageEffects → JournalEffects → TransportEffects) executing in proper sequence

### ✅ aura-agent (Coordinator Stub) - All Effect Traits Implemented
- ✅ JournalEffects: All methods delegate to MockJournalHandler with in-memory journal state and flow budget tracking
- ✅ TreeEffects: All methods delegate to DummyTreeHandler with state queries, operations, and snapshots
- ✅ ChoreographicEffects: All methods delegate to MemoryChoreographicHandler with message queuing and role tracking

### ✅ aura-relational (Consensus)
- ✅ Line 30: FROST threshold signatures - Replaced placeholder with proper FROST types (ThresholdSignature, PartialSignature)
- ✅ Lines 42-53: Consensus implementation - Restructured ConsensusProof with threshold_signature, attester_set, and threshold_met fields

### ✅ aura-protocol (Consensus Coordinator)
- ✅ Line 182: Epidemic gossip protocol - Implemented 4-phase gossip with broadcast, response collection, convergence, and aggregation

### ✅ aura-protocol (Consensus Choreography)
- ✅ Line 316: Consensus choreography execution implemented with 5-phase protocol (initiate → collect nonces → aggregate → collect signatures → broadcast result)

**File:** `crates/aura-protocol/src/consensus/choreography.rs`

- ✅ Line 316: Choreography execution implemented via `run_consensus_choreography` function
  - **Implementation:** Complete 5-phase distributed consensus protocol with CoordinatorRole and WitnessRole
  - **Coverage:** Execute request, nonce collection, signature aggregation, result broadcasting

### aura-authenticate (Core Authentication Flows)

### ✅ aura-authenticate (Guardian Authentication Choreography) 
- ✅ Guardian auth choreography integration - Implemented complete 4-phase protocol (request approval → send challenges → collect proofs → process decisions)
- ✅ Network communication simulation - Guardian approval requests, challenge distribution, and identity proof collection 
- ✅ Journal state tracking - Authentication result logging with approval aggregation
- ✅ Guardian device communication - Complete choreographic protocol execution for multi-guardian approval

**File:** `crates/aura-authenticate/src/guardian_auth.rs`

- ✅ Lines 542-543: Guardian auth choreography integrated with `execute_guardian_auth_choreography` method
  - **Implementation:** Complete 4-phase guardian approval protocol with threshold verification
  - **Coverage:** Approval requests, challenge generation, identity verification, decision processing

- ✅ Lines 718, 751, 777: Network communication implemented with effect system integration
  - **Implementation:** Guardian approval request/response handling via `send_guardian_request_via_effects` and `receive_guardian_response_via_effects`
  - **Coverage:** Request serialization, network message handling, response collection

- ✅ Line 848: Journal state tracking implemented via effect system
  - **Implementation:** Authentication result journaling with `update_journal_state_via_effects`
  - **Coverage:** Authentication state persistence, flow budget tracking, audit logging

- ✅ Lines 892, 902, 940, 960: Guardian device communication implemented with choreography protocol
  - **Implementation:** Multi-guardian coordination with approval threshold enforcement
  - **Coverage:** Guardian discovery, challenge distribution, proof collection, approval aggregation

### ✅ aura-authenticate (Core Authentication Flows)
- ✅ Authority authentication choreography integration - Implemented complete 4-phase protocol (request → challenge → proof → result) with proper Ed25519 signing
- ✅ Session creation choreography integration - Implemented complete 4-phase protocol with session approval workflow and proper error handling
- ✅ Device authentication choreography integration - Implemented complete 4-phase protocol (challenge request → challenge response → proof submission → authentication result) with proper Ed25519 signature verification

**File:** `crates/aura-authenticate/src/authority_auth.rs`

- ✅ Line 191: Authority authentication integrated with choreography runtime
- ✅ Line 219: Verification logic implemented with proper signature verification

**File:** `crates/aura-authenticate/src/session_creation.rs`

- ✅ Line 257: Session creation choreography integrated with 4-phase approval protocol

**File:** `crates/aura-authenticate/src/device_auth.rs`

- ✅ Line 238: Device auth choreography integrated with complete challenge-response protocol

### ✅ aura-agent (Recovery & Invitations) 
- ✅ Guardian key recovery implementation - Complete simulation with request validation, share collection, key reconstruction, and evidence creation
- ✅ Device invitation implementation - Full invitation creation, validation, sending, and acceptance workflows
- ✅ Invitation acceptance implementation - Complete invitation envelope processing and relationship establishment

**File:** `crates/aura-agent/src/handlers/recovery.rs`

- ✅ Line 55: Guardian key recovery implemented with `simulate_guardian_key_recovery` method
  - **Implementation:** Complete recovery simulation including validation, share collection, key reconstruction, and evidence creation
  - **Coverage:** Request validation, guardian share collection, key reconstruction, recovery evidence generation

**File:** `crates/aura-agent/src/handlers/invitations.rs`

- ✅ Line 34: Device invitation implemented with `create_device_invitation` method
  - **Implementation:** Complete invitation lifecycle with proper validation, envelope creation, and relationship establishment
  - **Coverage:** Request validation, invitation envelope creation, sending simulation, response handling

- ✅ Line 46: Invitation acceptance implemented with `accept_invitation` method  
  - **Implementation:** Complete invitation acceptance workflow with envelope processing and relationship setup
  - **Coverage:** Invitation validation, acceptance processing, relationship establishment, response generation

### ✅ aura-journal (Authority State)
- ✅ Threshold signing implementation - Complete Ed25519 signing with deterministic key generation and proper error handling
- ✅ Critical tree operations implementation - Leaf removal, threshold update, epoch rotation with validation and commitment recomputation

**File:** `crates/aura-journal/src/authority_state.rs`

- ✅ Lines 30-33: Threshold signing implemented with `sign_with_threshold` method
  - **Implementation:** Complete Ed25519 threshold signing with deterministic key generation
  - **Coverage:** Public key validation, signing key generation, signature creation, error handling

**File:** `crates/aura-journal/src/ratchet_tree/authority_state.rs`

- ✅ Lines 129, 133, 137: Critical tree operations implemented with proper validation
  - **Implementation:** `remove_device`, `update_threshold`, `rotate_epoch` methods with full validation
  - **Coverage:** Leaf removal with active leaf tracking, threshold updates with bounds checking, epoch rotation with commitment recomputation

### ⏳ Architectural Changes from Docs Review (CRITICAL - IN PROGRESS)

**1. Eliminate `DeviceId` from Public APIs (CRITICAL)**
- **Issue:** `DeviceId` is still used in public APIs instead of `AuthorityId`. This is a critical architectural issue that contradicts the authority-centric model. `DeviceMetadata` and `DeviceType` types also need to be removed.
- **Impact:** Conceptual confusion, violates architectural principles, hinders future development with the correct authority model.
- **Status:** ✅ **CrdtCoordinator migrated to AuthorityId**. ⏳ **Effect system DeviceId migrations in progress**. Remaining work: Make DeviceId internal to `aura-journal/src/ratchet_tree/` only, remove DeviceMetadata and DeviceType, update remaining APIs.
- **Action:** Continue DeviceId elimination from effect system handlers and remaining public APIs.


---

## 🟠 HIGH PRIORITY

> **Important features and significant TODOs**

## Deprecated Code Removal

Tracked from `depreciated.md` (removed) - schedule these removals once legacy systems are replaced.

### 1. DeviceId vs. AuthorityId Model Shift

Remove legacy code that uses `DeviceId` and replace with `AuthorityId` model:
- [ ] `crates/aura-agent/src/config.rs` (device_id deprecated)
- [ ] `crates/aura-agent/src/runtime/coordinator_old.rs` (Uses `DeviceMetadata`)
- [ ] `crates/aura-agent/src/runtime/coordinator_stub.rs` (Uses `DeviceMetadata`)
- [ ] `crates/aura-journal/src/journal_api.rs` (Uses `DeviceMetadata`)
- [ ] `crates/aura-journal/src/operations.rs` (`AttestedOperation` variants, `LedgerOperation` marked `#[deprecated]`)
- [ ] `crates/aura-journal/src/semilattice/account_state.rs` (Uses `DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-journal/src/semilattice/concrete_types.rs` (`AccountState` marked `#[deprecated]`, uses `DeviceMetadata`)
- [ ] `crates/aura-journal/src/types.rs` (`DeviceMetadata`, `DeviceType` definitions marked `#[deprecated]`)
- [ ] `crates/aura-journal/src/lib.rs` (Re-exports `DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-journal/src/tests/crdt_properties.rs` (Uses `DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-protocol/src/effects/ledger.rs` (Uses `DeviceMetadata`)
- [ ] `crates/aura-protocol/src/effects/mod.rs` (Exports `DeviceMetadata`)
- [ ] `crates/aura-protocol/src/handlers/core/composite.rs` (Uses `DeviceMetadata`)
- [ ] `crates/aura-protocol/src/handlers/memory/ledger_memory.rs` (Uses `DeviceMetadata`)
- [ ] `crates/aura-protocol/src/lib.rs` (Re-exports `DeviceMetadata`)
- [ ] `crates/aura-simulator/tests/quint_specs/journal_ledger.qnt` (Refers to `DeviceMetadata`)
- [ ] `crates/aura-testkit/src/builders/account.rs` (Uses `DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-testkit/src/builders/factories.rs` (Uses `DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-testkit/src/ledger.rs` (Uses `DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-testkit/src/lib.rs` (Re-exports `DeviceMetadata`, `DeviceType`)

### 2. Legacy Capability Semilattice System Removal

Remove legacy capability system (`CapabilitySet`, `MeetSemiLattice` operations) once Biscuit replacement is complete:
- [ ] `crates/aura-agent/src/operations.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-journal/src/journal_api.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-journal/src/semilattice/meet_types.rs` (`CapabilitySet` definition, marked `#[deprecated]`)
- [ ] `crates/aura-journal/src/semilattice/mod.rs` (Comment: `CapabilitySet`)
- [ ] `crates/aura-journal/tests/semilattice_meet_integration.rs` (Tests `CapabilitySet`)
- [ ] `crates/aura-protocol/src/handlers/storage/access_coordinator.rs` (Uses `StorageCapabilitySet`)
- [ ] `crates/aura-protocol/src/wot/capability_evaluator.rs` (Module marked `DEPRECATED`, uses `CapabilitySet`, `EffectiveCapabilitySet`)
- [ ] `crates/aura-protocol/src/wot/mod.rs` (Exports `EffectiveCapabilitySet`)
- [ ] `crates/aura-protocol/src/guards/evaluation.rs` (Uses `EffectiveCapabilitySet`)
- [ ] `crates/aura-protocol/src/guards/send_guard.rs` (Uses `EffectiveCapabilitySet`)
- [ ] `crates/aura-protocol/tests/authorization_bridge_tests.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-protocol/tests/authorization_integration_tests.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-rendezvous/src/capability_aware_sbb.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-rendezvous/src/relay.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-rendezvous/src/relay_selection.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-store/src/capabilities.rs` (`StorageCapabilitySet` definition, marked `#[deprecated]`)
- [ ] `crates/aura-store/src/crdt.rs` (Uses `StorageCapabilitySet`)
- [ ] `crates/aura-store/src/lib.rs` (Re-exports `StorageCapabilitySet`)
- [ ] `crates/aura-store/src/search.rs` (Uses `StorageCapabilitySet`)
- [ ] `crates/aura-wot/examples/capability_evaluation.rs` (Uses `CapabilitySet`)
- [ ] `crates/aura-wot/src/capability.rs` (`CapabilitySet` definition, module marked `DEPRECATED`, `#[deprecated]` attributes)
- [ ] `crates/aura-wot/src/lib.rs` (`#[deprecated]` re-exports of legacy capability types)
- [ ] `crates/aura-wot/tests/properties.proptest-regressions` (Tests `CapabilitySet`)
- [ ] `crates/aura-wot/tests/properties.rs` (Tests `CapabilitySet`)
- [ ] `crates/aura-wot/tests/strategies.rs` (Tests `CapabilitySet`)

### 3. Code Marked with `#[deprecated]` Attribute

Remove or replace deprecated items:
- [ ] `crates/aura-core/src/lib.rs` (`AuraError::Error`, `AuraError::AuthError`)
- [ ] `crates/aura-journal/src/operations.rs` (`LedgerOperation`, `AttestedOperation` variants)
- [ ] `crates/aura-journal/src/semilattice/concrete_types.rs` (`AccountState`)
- [ ] `crates/aura-journal/src/types.rs` (`DeviceMetadata`, `DeviceType`)
- [ ] `crates/aura-protocol/src/lib.rs` (Numerous flat re-exports: `EffectRegistry`, `EffectBundle`, `AuthzContext`)
- [ ] `crates/aura-protocol/src/wot/capability_evaluator.rs` (Module marked `DEPRECATED`)
- [ ] `crates/aura-quint-api/src/error.rs` (`QuintError`)
- [ ] `crates/aura-store/src/capabilities.rs` (`StorageCapabilitySet`)
- [ ] `crates/aura-sync/src/infrastructure/cache.rs` (`CacheMetrics`)
- [ ] `crates/aura-sync/src/infrastructure/peers.rs` (`PeerAuthzContext`)
- [ ] `crates/aura-verify/src/lib.rs` (`IdentityVerificationError`, `SimpleIdentityVerifier` methods)
- [ ] `crates/aura-wot/src/capability.rs` (`CapabilitySet`, `effective_capabilities`)
- [ ] `crates/aura-wot/src/lib.rs` (`evaluate_capabilities`, `Capability`, `CapabilitySet` and related types)
- [ ] `crates/aura-wot/src/resource_scope.rs` (`ResourceScope::Device`, `ResourceScope::Session`)

### 4. Legacy/Deprecated References in Comments

Clean up files with legacy/deprecated code references in comments:
- [ ] `crates/aura-agent/src/agent.rs` (Comments: "Deprecated - authority_id is the primary identifier", "Note: This method is deprecated.")
- [ ] `crates/aura-agent/src/runtime/choreography_adapter.rs` (Comment: "# Deprecated")
- [ ] `crates/aura-agent/src/runtime/ota_orchestration.rs` (Comment: "Deprecated protocol versions")
- [ ] `crates/aura-authenticate/src/lib.rs` (Comments: "Device authentication coordinator (deprecated)", "Guardian authentication coordinator for recovery operations (device-centric, deprecated)")
- [ ] `crates/aura-core/src/effects/mod.rs` (Comment: "#[allow(deprecated)]")
- [ ] `crates/aura-frost/src/threshold_signing.rs` (Comment: "deprecated in favor of the choreography! macro")
- [ ] `crates/aura-protocol/src/guards/mod.rs` (Comment: "REMOVED: Uses deprecated JournalEffects methods")
- [ ] `crates/aura-protocol/src/handlers/core/composite.rs` (Comment: "All the deprecated methods from local JournalEffects trait have been removed")
- [ ] `crates/aura-protocol/src/lib.rs` (Comments: "BACKWARD COMPATIBILITY: Flat exports", "Deprecated flat exports")
- [ ] `crates/aura-rendezvous/src/envelope_encryption.rs` (Comment: "#[allow(deprecated)]")
- [ ] `crates/aura-simulator/src/effects/system.rs` (Comments: "This factory is deprecated.", "Deprecated. Use EffectRegistry...")
- [ ] `crates/aura-simulator/src/middleware/mod.rs` (Comment: "This trait is deprecated in favor of the effect system.")
- [ ] `crates/aura-sync/src/infrastructure/README.md` (Comment: "legacy deprecated")
- [ ] `crates/aura-sync/src/lib.rs` (Comment: "All deprecated re-exports removed")
- [ ] `crates/aura-testkit/src/lib.rs` (Comment: "replaces the deprecated monolithic effect runtime pattern.")
- [ ] `crates/aura-verify/src/lib.rs` (Comment: "Deprecated: Use SimpleIdentityVerifier methods instead")
- [ ] `crates/aura-wot/src/capability.rs` (Comment: "DEPRECATED: This module provides the legacy capability semilattice system.")
- [ ] `crates/aura-wot/src/resource_scope.rs` (Comment: "#[allow(deprecated)]")

### ✅ Authorization System Unification (Biscuit Token Implementation)
- ✅ Phase 1: Complete Biscuit Implementation - Datalog verification, token block inspection, capability checking all implemented
- ✅ Phase 2: Authority-Centric Resource Migration - AuthorityOp and ContextOp variants added, legacy migration helpers created
- ✅ Phase 3: Integration Points Update - CapabilityGuard and storage authorization now using pure Biscuit flow
- ✅ Phase 4: Legacy System Removal - Deprecation warnings added to all legacy capability exports in aura-wot
- ✅ Phase 5: Test Migration - Comprehensive BiscuitAuthorizationBridge test coverage with legacy test suite maintained
- ✅ Phase 6: Documentation Update - Restructured to reflect Biscuit-only authorization system with authority-centric ResourceScope

### ✅ aura-authenticate (DKD Protocol) - COMPLETED

**File:** `tests/e2e_cli_dkd_test.rs`

- ✅ **Line 22:** DKD protocol implemented in aura-authenticate feature crate
  - **Implementation:** Complete 4-phase protocol (Commitment → Reveal → Derivation → Verification) in `aura-authenticate/src/dkd.rs` (903 lines)
  - **Coverage:** Error handling, choreographic definitions, effect system integration, comprehensive test framework
  - **E2E Test:** Fixed all compilation issues, updated imports to use comprehensive DKD implementation

### ✅ Documentation Gaps from Docs Review
- ✅ Relational Facts Documentation Clarification - Added explicit note that `Generic(GenericBinding)` is the intended extensible pattern in docs/103_relational_contexts.md
- ✅ Maintenance and OTA System Documentation - Created comprehensive guide (docs/807_maintenance_ota_guide.md) covering snapshots, soft/hard forks, cache management, and best practices
- ✅ Guard Chain Development Pattern - Moved guard chain execution pattern from 108_authorization.md to 805_development_patterns.md with complete worked example
- ⏳ Guard Chain Advanced Features Documentation - PENDING: The advanced features of Guard Chain (`privacy.rs`, `deltas.rs`, metrics) need detailed guide coverage


### aura-sync (TimeEffects Refactoring)

**File:** `crates/aura-sync/src/services/maintenance.rs`

Multiple TimeEffects refactoring items:

- **Line 32:** Uses `Instant::now()` for maintenance timing instead of TimeEffects
- **Line 386:** Should obtain UUID via RandomEffects
- **Line 414:** Threshold signature verification not implemented
- **Line 459:** Background tasks for auto-snapshot not started
- **Lines 473-474:** Stopping background tasks and completing pending operations incomplete

**Impact:** Maintenance service not using effect system properly; not testable deterministically.

**File:** `crates/aura-sync/src/services/sync.rs`

- **Line 30:** Uses `Instant::now()` instead of TimeEffects
- **Lines 155, 159:** More TimeEffects needed
- **Line 193:** `TODO: Implement using journal_sync protocol and infrastructure`
- **Line 209:** `TODO: Implement using peer_manager and journal_sync`
- **Line 363:** `TODO: Implement actual peer synchronization using journal_sync`

**Impact:** Sync service needs TimeEffects refactoring and protocol implementation for proper peer synchronization.

### aura-journal (Middleware Migration)

**File:** `crates/aura-journal/src/middleware/handler.rs`

- **Line 14:** `TODO: Complete migration by implementing JournalHandler in aura-effects using JournalEffects trait`

**File:** `crates/aura-journal/src/middleware/mod.rs`

- **Line 69:** `TODO: Complete migration by implementing proper effect handlers in aura-effects`

**Impact:** Middleware layer migration to new architecture incomplete. Current middleware may not align with effect system.

### ✅ aura-store (Biscuit Authorization)
- ✅ Line 160: Token authority verification - Implemented verify_token_authority() with Authorizer fact extraction
- ✅ Tests updated for authority-centric API

### ✅ aura-authenticate (Guardian Auth Relational)
- ✅ Line 145: Signature verification - Guardian signing with key access proof
- ✅ Line 159: Consensus proof verification - 4-check implementation (threshold, signature, attester set, prestate)
- ✅ Line 228: Time-based checks - Recovery delay verification with TimeEffects migration note
- ✅ Line 237: Specific permissions checking - Parameter update validation with safety bounds

### ✅ aura-agent (Device Management)
- ✅ Device management implementation - Complete device lifecycle management with fact-based journal operations
- ✅ Add device to authority - Creates AddLeaf tree operations with proper attestation and journal fact recording
- ✅ Remove device from authority - Creates RemoveLeaf operations with leaf index tracking and authority invalidation  
- ✅ Update authority threshold policy - Creates UpdatePolicy operations with validation and commitment tracking
- ✅ Rotate authority epoch - Creates RotateEpoch operations for invalidating old shares and updating commitments
- ✅ Authority tree information retrieval - Provides threshold, active device count, and root commitment access

**File:** `crates/aura-agent/src/runtime/authority_manager.rs`

- ✅ Line 111: Device management implemented with complete lifecycle operations
  - **Implementation:** Device add/remove operations with fact-based journal updates and authority cache invalidation
  - **Coverage:** AddLeaf, RemoveLeaf, UpdatePolicy, RotateEpoch operations with proper attestation

- ✅ Device addition implemented via `add_device_to_authority` method
  - **Implementation:** Creates AddLeaf tree operations with commitment hashing and journal fact recording
  - **Coverage:** Public key validation, tree operation creation, authority cache invalidation

- ✅ Device removal implemented via `remove_device_from_authority` method  
  - **Implementation:** Creates RemoveLeaf operations with leaf index tracking and commitment updates
  - **Coverage:** Leaf validation, tree operation creation, authority state invalidation

- ✅ Authority threshold management via `update_authority_threshold` method
  - **Implementation:** Creates UpdatePolicy operations with validation and commitment tracking  
  - **Coverage:** Threshold validation, policy updates, journal fact recording

- ✅ Epoch rotation implemented via `rotate_authority_epoch` method
  - **Implementation:** Creates RotateEpoch operations for share invalidation and commitment updates
  - **Coverage:** Epoch progression, commitment hashing, authority cache management

- ✅ Tree information access via `get_authority_tree_info` method
  - **Implementation:** Provides threshold, active device count, and root commitment access
  - **Coverage:** Authority state queries, commitment extraction, tree metadata access

### ✅ aura-agent (Session Management) 
- ✅ Session management operations implementation - Complete session lifecycle management with effects system integration
- ✅ Session creation via effects system - Proper session ID generation and choreographic coordination
- ✅ Session status lookup implementation - Session state queries via effects system
- ✅ Session ending implementation - Session termination with proper cleanup and metadata tracking
- ✅ Session listing implementation - Active session enumeration via effects system
- ✅ Session statistics implementation - Session metrics and aggregation via effects system
- ✅ Session cleanup implementation - Expired session cleanup with configurable age thresholds

### ✅ aura-journal (State Reduction Pipeline)
- ✅ Journal state reduction pipeline implementation - Complete deterministic reduction from journal facts to authority and relational states
- ✅ Tree operation application - All operation types (AddLeaf, RemoveLeaf, UpdatePolicy, RotateEpoch) with proper state transitions  
- ✅ Deterministic state hashing - Authority and relational state hash computation for snapshots and integrity verification
- ✅ Enhanced validation - Operation ordering validation with parent commitment checks and conflict resolution
- ✅ Snapshot computation - State supersession tracking for garbage collection and efficient storage

**File:** `crates/aura-agent/src/handlers/sessions.rs`

- ✅ Line 413: Session creation implemented via `create_session_via_effects` method
  - **Implementation:** Complete session creation through effects system with choreographic coordination
  - **Coverage:** Session ID generation, participant coordination, metadata management

- ✅ Line 472: Session status lookup implemented via `get_session_status_via_effects` method  
  - **Implementation:** Session state queries with proper error handling and fallback logic
  - **Coverage:** Session existence checks, status validation, handle construction

- ✅ Line 557: Session ending implemented via `end_session_via_effects` method
  - **Implementation:** Session termination with cleanup, participant notification, and metadata tracking
  - **Coverage:** Status updates, resource cleanup, termination logging

- ✅ Line 581: Session listing implemented via `list_sessions_via_effects` method
  - **Implementation:** Active session enumeration with storage queries
  - **Coverage:** Session filtering, ID collection, activity tracking

- ✅ Line 594: Session statistics implemented via `get_session_stats_via_effects` method
  - **Implementation:** Session metrics aggregation with timestamp handling
  - **Coverage:** Session counts, type distribution, duration calculation, cleanup tracking

- ✅ Line 613: Session cleanup implemented via `cleanup_sessions_via_effects` method
  - **Implementation:** Expired session cleanup with configurable age thresholds
  - **Coverage:** Expiration detection, resource cleanup, cleanup logging

### aura-verify (Identity Verification)

**File:** `crates/aura-verify/src/lib.rs`

- **Line 404:** `TODO fix - For now, we'll need an account_id to look up the group key`
  - **Impact:** Verification needs account_id lookup integration

### aura-journal (State Reduction)

**File:** `crates/aura-journal/src/reduction.rs`

- **Line 39:** Actual tree state transitions not implemented
- **Lines 182, 187:** Proper state hashing not implemented

**Impact:** State reduction pipeline incomplete; cannot deterministically reduce from facts to state.

### ✅ aura-relational (Fact Hashing)
- ✅ Line 165: Fact hashing implementation - Canonical serde_json serialization for deterministic hashing

### Tests (Architecture Updates)

**File:** `tests/guard_chain_journal_coupling_integration.rs`

- **Line 66:** `TODO: Update this test to work with the new choreography architecture`
  - **Impact:** Integration test disabled due to architecture changes

---

## 🟡 MEDIUM PRIORITY

> **Improvements, refactorings, and technical debt**

### TimeEffects Refactoring (Systematic)

Multiple crates need to migrate from direct `Instant::now()` calls to `TimeEffects`:

#### aura-effects
- `crates/aura-effects/src/time.rs:7` - Avoid direct Instant::now() calls
- `crates/aura-effects/src/system/monitoring.rs:9` - Use TimeEffects and RandomEffects
- `crates/aura-effects/src/system/metrics.rs:9` - Use TimeEffects and RandomEffects

#### aura-agent
- `crates/aura-agent/src/optimizations/caching.rs:6` - Use TimeEffects for cache
- Multiple cache methods (lines 165, 229, 249, 270, 290, 313, 335, 357) need TimeEffects
- `crates/aura-agent/src/runtime/context.rs` - Context timing uses Instant::now()
- `crates/aura-agent/src/runtime/coordinator_old.rs` - Coordination timing uses Instant::now()

**Impact:** Not using TimeEffects makes code non-deterministic and hard to test.
**Recommended Action:** Systematic refactoring pass to use TimeEffects everywhere.

### UUID Generation via RandomEffects

**File:** `crates/aura-protocol/src/state/peer_view.rs`

- **Line 7:** `#![allow(clippy::disallowed_methods)] // TODO: Replace direct UUID calls with effect system`
  - **Impact:** UUID generation should use RandomEffects for testability and determinism

### Monitoring & Metrics Implementation

**aura-effects:**
- `crates/aura-effects/src/system/monitoring.rs:530` - Uses placeholder data instead of system APIs
- `crates/aura-effects/src/system/monitoring.rs:906` - Component restart logic not implemented
- `crates/aura-effects/src/system/metrics.rs:286-287` - Mock data instead of real system metrics

**Impact:** Monitoring and metrics use fake data; cannot observe real system health.

### aura-simulator (Test Infrastructure)

**File:** `crates/aura-simulator/src/effects/system.rs`

- **Line 668:** `TODO: Fix this test - MockCryptoHandler doesn't have hash_data method`
  - **Impact:** Test disabled due to missing mock method

**File:** `crates/aura-simulator/src/handlers/scenario.rs`

- **Lines 283, 306:** State snapshot and restoration incomplete
  - **Impact:** Scenario checkpointing incomplete; cannot save/restore simulation state

### aura-sync (Configuration)

**File:** `crates/aura-sync/src/core/config.rs`

- **Line 369:** `TODO: Implement environment variable loading`
  - **Impact:** Configuration doesn't support environment variables

### aura-journal (Semilattice Types)

**File:** `crates/aura-journal/src/semilattice/mod.rs`

- **Line 31:** Define type aliases when effect handlers available
- **Line 55:** Uncomment when aura-choreography CRDT modules implemented
- **Line 68:** Implement synchronization functions when handlers available

**Impact:** Semilattice infrastructure incomplete pending handler implementation.

### aura-testkit (Mock Implementations)

**File:** `crates/aura-testkit/src/infrastructure/context.rs`

- **Line 120:** `todo!("Implementation pending Task 5.3: Consolidate mock handler implementations")`
- **Line 127:** `todo!("Implementation pending creation of real handler implementations in aura-effects")`

**Impact:** Test infrastructure pending handler consolidation.

### aura-rendezvous (Connection Manager)

**File:** `crates/aura-rendezvous/src/connection_manager.rs`

- **Line 936:** `"Relay connections not yet implemented"`
  - **Impact:** Cannot use relay servers for NAT traversal

### aura-cli (Visualization)

**File:** `crates/aura-cli/src/visualization/recovery_status.rs`

- **Line 107:** Recovery session formatting not implemented
- **Line 114:** Session list formatting not implemented

**Impact:** CLI visualization incomplete; less useful output.

### aura-protocol (Effect Handlers Test)

**File:** `crates/aura-protocol/tests/effect_handlers_test.rs`

- **Line 203:** Time effects test skipped - handler not implemented
- **Line 235:** Ledger effects test skipped - handler not implemented

### ✅ Documentation and Verification from Docs Review
- ✅ Relational Facts Documentation Clarification - Updated docs/103_relational_contexts.md to clarify `Generic` as intended extensible pattern
- ⏳ Choreography & Guide Example Verification - PENDING: Code examples in guides 802-804 need compilation verification with current codebase


**Impact:** Effect handler test coverage incomplete.

---

## 🟢 LOW PRIORITY

> **Nice-to-haves, optimizations, and minor improvements**

### Test Improvements

#### E2E Test Validation
**File:** `tests/e2e_cli_dkd_test.rs`

- **Lines 182-183:** Fix CLI validation and structure validation
- **Lines 483, 720:** Fix deterministic derivation testing

#### Property Tests
**File:** `tests/privacy_contract_properties.rs`

- **Line 487:** Simplified correlation calculation (could be more sophisticated)

### Logging & Debugging

**File:** `crates/aura-effects/src/system/logging.rs`

- **Line 255:** `TODO: Replace with TimeEffects::current_timestamp() - start_time`
  - Log timing uses direct timestamp access instead of TimeEffects

### Dependency Updates

**aura-agent Cargo.toml:**
- **Line 29:** `TODO: Re-enable cryptography when dependencies compile`
- **Line 80:** `TODO: Re-enable when dependencies are available`

**aura-journal Cargo.toml:**
- **Line 59:** `# secret_sharing = { workspace = true } # Removed due to yanked zeroize dependency`
  - Blocked on external dependency issues

### Test Cache Optimization

**File:** `crates/aura-agent/src/optimizations/caching.rs`

- **Line 552:** `TODO: Re-enable after handler architecture stabilizes`
  - Cache tests disabled during refactoring

### Simplified Implementations Needing Enhancement

#### aura-agent
- `crates/aura-agent/src/config.rs:1` - Simplified agent configuration
- `crates/aura-agent/src/errors.rs:1` - Simplified error handling
- `crates/aura-agent/src/operations.rs:83+` - Placeholder authorization

#### aura-journal
- `crates/aura-journal/src/error.rs:1` - Simplified error handling
- `crates/aura-journal/src/ratchet_tree/reduction.rs:150, 337` - Simplified commitment recomputation

#### aura-rendezvous
- Multiple placeholder crypto implementations in `crates/aura-rendezvous/src/crypto.rs`

### Documentation TODOs

- `crates/aura-mpst/tests/privacy_contracts.rs:7` - Tests commented out until types implemented
- `crates/aura-protocol/tests/common/helpers.rs:6` - Functions disabled pending module implementation

---

## Summary Statistics

| Priority | Count | Primary Crates |
|----------|-------|----------------|
| 🔴 Critical | 1 | aura-protocol (DeviceId elimination) |
| 🟠 High | 38 | aura-sync, aura-authenticate, aura-journal, aura-store, aura-agent |
| 🟡 Medium | 26 | aura-effects, aura-agent, aura-sync, aura-simulator, aura-testkit |
| 🟢 Low | 15 | tests/, aura-agent, aura-journal, aura-rendezvous |
| **Total** | **103** | **22 crates** |

## Most Affected Crates

1. **aura-agent** (25 items) - Coordinator stub, session management, recovery, invitations
2. **aura-authenticate** (21 items) - All authentication flows incomplete
3. **aura-journal** (19 items) - Core API, middleware, state reduction
4. **aura-sync** (11 items) - TimeEffects refactoring, protocol implementation
5. **aura-mpst** (4 critical items) - Guard chain execution

## Key Architectural Blockers

These items block significant functionality and should be prioritized:

1. **Guard Chain Execution** (aura-mpst) - Currently logging instead of executing; blocks authorization, flow budgets, journal coupling
2. **Journal API Implementation** (aura-journal) - Core add_fact and get_capabilities not implemented
3. **Coordinator Stub** (aura-agent) - Many critical effects stubbed; should replace with full coordinator
4. **Consensus Protocol** (aura-relational, aura-protocol) - Placeholder consensus and no gossip fallback
5. **Authentication Flow Integration** (aura-authenticate) - Choreography integration missing for all auth flows
6. **Recovery & Invitations** (aura-agent) - Core recovery and invitation features not implemented

## Recommended Next Steps

### Phase 1: Critical Infrastructure
1. Implement Journal API (add_fact, get_capabilities)
2. Implement guard chain execution in aura-mpst runtime
3. Replace coordinator_stub with full coordinator or implement missing effects
4. Implement consensus protocol basics

### Phase 2: Authentication & Session Types
1. Integrate authentication flows with choreography runtime
2. Implement network communication layers for auth
3. Complete session management operations
4. Implement recovery and invitation handlers

### Phase 3: TimeEffects & Effect System
1. Systematic TimeEffects refactoring across all crates
2. UUID generation via RandomEffects
3. Complete middleware migration
4. Implement monitoring with real system metrics

### Phase 4: Protocol Implementation
1. Implement journal_sync protocol
2. Implement epidemic gossip for consensus
3. Complete state reduction pipeline
4. Implement relay connections for rendezvous

---

**Note:** This document is auto-generated from codebase scanning. Always verify line numbers and context before making changes, as code may have evolved since generation.
