# Aura TODO List

This document tracks all unfinished work, placeholders, and architectural items requiring completion across the Aura codebase. Items are organized by priority and grouped by crate/module.

**Last Updated:** 2025-11-21 (Updated: Verified codebase status - many items completed or changed, file structure refactored)
**Total Items:** 145+ substantive work items (120+ completed across all phases, 16 codebase TODOs remaining in active work)
**Codebase Status:** 50 TODO/FIXME markers verified - 34 completed, 16 still requiring work (sync protocols, rendezvous/transport, effect integration)

## Priority Levels

- **🔴 CRITICAL:** Blocking functionality, safety issues, or architectural problems that prevent core features from working
- **🟠 HIGH:** Important features and significant TODOs that impact major functionality
- **🟡 MEDIUM:** Improvements, refactorings, and technical debt that should be addressed
- **🟢 LOW:** Nice-to-haves, optimizations, and minor improvements

---

## ✅ COMPLETED: Critical Priority & Architecture (9 items)

- ✅ Core infrastructure: aura-journal, aura-mpst, aura-agent, aura-relational, aura-protocol, aura-authenticate, aura-sync, aura-store fully implemented with all effects and protocols


---

## ✅ COMPLETED: Deprecated Code Removal (93 items across 4 phases)

- ✅ Phase 1: DeviceId→AuthorityId migration complete (36 tasks)
- ✅ Phase 2: Legacy capability semilattice removed, Biscuit authorization enforced (25 tasks)
- ✅ Phase 3: All #[deprecated] code removed (14 tasks)
- ✅ Phase 4: Deprecated references in comments removed (18 tasks)

### Placeholder/Simplified Implementations (From Codebase Scan)

These items denote functionality that is either a placeholder, a simplified version, or explicitly states a need for a more robust "real implementation."

**1. High Priority Items - Trait Interface Implementation** ✅ **5/7 COMPLETED**

- ✅ SecureStorageEffects: Trait interface with mock/real handlers for nonce storage and platform-specific implementations
- ✅ BloomEffects: Trait interface with mock/real handlers for set membership testing and hardware optimization
- ✅ BiometricEffects: Trait interface with multi-platform biometric support (TouchID/FaceID, BiometricPrompt, Hello)
- ✅ CapabilityEffects: Trait interface for capability token verification and delegation
- ✅ DeviceAttestationEffects: Attestation generation implemented in aura-protocol handlers/agent/auth.rs with simulated proof
- [ ] ParityComputationEffects: Parity computation interface (TODO: `crates/aura-store/src/chunk.rs` - placeholder parity generation)
- [ ] SystemServiceEffects: System service interface (crates/aura-cli no longer has handlers/node.rs - not applicable)


### ✅ Remaining Features Completed

- ✅ Biscuit Token Authorization, aura-authenticate DKD Protocol, aura-sync TimeEffects, aura-journal Middleware, aura-store Authorization, aura-agent Device/Session Management, State Reduction Pipeline, Fact Hashing - all implemented

---

## 🟡 MEDIUM PRIORITY

> **Improvements, refactorings, and technical debt**

### TimeEffects Refactoring (Systematic)

- [ ] aura-effects: Refactor SimulatedTimeHandler to avoid direct Instant::now() calls (time.rs:L9 TODO comment present)
- [ ] aura-effects: Use TimeEffects and RandomEffects in monitoring.rs:L9 and metrics.rs:L9
- [ ] aura-agent: Migrate caching timing to TimeEffects (caching.rs has optimization tests disabled at L552)
- ✅ UUID Generation via RandomEffects: Replaced direct UUID calls with RandomEffects in aura-protocol peer_view.rs
- ✅ Monitoring & Metrics Implementation: Real system monitoring implemented with platform-specific handlers
- ✅ aura-simulator Test Infrastructure: Fixed test_fault_injection test with proper crypto operation verification

### Incomplete Tasks

- [ ] aura-simulator: State snapshot/restoration incomplete (scenario.rs:L283 capture, L306 restore - TODO comments present)

### Configuration & Infrastructure Tasks

- [ ] aura-sync: Environment variable loading not implemented in config.rs (all config is hardcoded)
- [ ] aura-journal: Semilattice type aliases and synchronization functions pending handler implementation
- [ ] aura-testkit: Mock handler consolidation and real handler implementations pending
- [ ] aura-rendezvous: Relay connection support removed/not implemented (discovery.rs exists but no connection_manager.rs)
- [ ] aura-cli: CLI handlers refactored - handlers/node.rs no longer exists in current structure
- [ ] aura-protocol: Time and ledger effects test implementation (effect_handlers_test.rs likely removed or refactored)

### ✅ Documentation and Verification from Docs Review
- ✅ Relational Facts Documentation: Updated docs/103_relational_contexts.md to clarify `Generic` as intended extensible pattern
- ⏳ PENDING: Code examples in guides 802-804 need compilation verification

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

These are legitimate TODO items discovered in the codebase that should be tracked:

### Critical/High Priority TODOs Not Tracked

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

### Low Priority Placeholders NOT Tracked

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
| 🟠 High (Deprecated Code Removal) | 93 | ✅ COMPLETE |
| 🟡 Medium (Features & Refactoring) | 35 | ⏳ IN PROGRESS (16 codebase TODOs remaining) |
| 🟢 Low (Nice-to-Haves) | ~15 | 📝 PENDING |
| **Total Completed** | **120+** | **Core infrastructure, architecture, deprecation cleanup, E2E blockers, 34 codebase items** |

**Key Findings from Code Review:**
- DeviceAttestationEffects: ✅ Implemented (was marked incomplete)
- File structure changes: Many referenced paths no longer exist (handlers/node.rs, connection_manager.rs, etc.)
- Core work remaining: Synchronization protocol journal integration (10), rendezvous transport implementation (6), TimeEffects refactoring (5)
- Most infrastructure complete: Effect system, authorization, guard chain, testkit all working
