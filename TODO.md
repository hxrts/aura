# Aura TODO List

This document tracks all unfinished work, placeholders, and architectural items requiring completion across the Aura codebase. Items are organized by priority and grouped by crate/module.

**Last Updated:** 2025-11-19
**Total Items:** 126 substantive work items
**Codebase Scan:** 429 TODO/FIXME markers found

## Priority Levels

- **🔴 CRITICAL:** Blocking functionality, safety issues, or architectural problems that prevent core features from working
- **🟠 HIGH:** Important features and significant TODOs that impact major functionality
- **🟡 MEDIUM:** Improvements, refactorings, and technical debt that should be addressed
- **🟢 LOW:** Nice-to-haves, optimizations, and minor improvements

---

## 🔴 CRITICAL PRIORITY

> **Blocking functionality, safety issues, or architectural problems**

### aura-journal (Core Journal API)

**File:** `crates/aura-journal/src/journal_api.rs`

- **Line 68:** `todo!("Add fact implementation")`
  - **Context:** `pub fn add_fact(&mut self, _fact: JournalFact) -> Result<(), AuraError>`
  - **Impact:** Core journal fact addition is not implemented. This blocks the entire fact-based journal system.
  - **Blocker for:** All fact-based operations, CRDT synchronization, consensus

- **Line 74:** `todo!("Get capabilities implementation")`
  - **Context:** `pub fn get_capabilities(&self, _context: &ContextId) -> CapabilitySet`
  - **Impact:** Capability retrieval from journal is not implemented. Critical for authorization system.
  - **Blocker for:** Authorization, guard chain, capability evaluation

### aura-mpst (Session Type Runtime - Guard Chain)

**File:** `crates/aura-mpst/src/runtime.rs`

The guard chain extensions are currently logging instead of executing:

- **Line 603:** Flow cost charging logic not implemented
  - **Impact:** Violates charge-before-send invariant; no flow budget enforcement
  - **Security Risk:** Spam prevention not working

- **Line 631:** Journal fact recording logic not implemented
  - **Impact:** Facts from choreographies not persisted
  - **Blocker for:** State persistence, consensus, recovery

- **Line 659:** Journal merge logic not implemented
  - **Impact:** CRDT merging not working in session types
  - **Blocker for:** Synchronization, anti-entropy

- **Line 686:** Guard chain execution logic not implemented
  - **Impact:** CapGuard → FlowGuard → JournalCoupler chain not executing
  - **Blocker for:** Authorization enforcement, privacy budgets, journal coupling

**Recommended Action:** Implement actual guard chain execution in extension handlers, integrating with aura-protocol guard infrastructure.

### aura-agent (Coordinator Stub)

**File:** `crates/aura-agent/src/runtime/coordinator_stub.rs`

This is a minimal stub with many critical effect traits unimplemented:

#### JournalEffects (Lines 398-432)
All methods return `"not implemented in stub"` errors:
- `merge_facts`, `refine_caps`, `get_journal`, `persist_journal`
- `get_flow_budget`, `update_flow_budget`, `charge_flow_budget`

**Impact:** Core journal operations completely stubbed out. No fact persistence, no capability refinement, no flow budget tracking.

#### TreeEffects (Lines 594-654)
All methods return `"not implemented in stub"` errors:
- `get_current_state`, `get_current_commitment`, `apply_attested_op`
- `add_leaf`, `remove_leaf`, `change_policy`, `rotate_epoch`
- Snapshot operations

**Impact:** Ratchet tree operations completely stubbed out. No device management, no threshold updates, no epoch rotation.

#### ChoreographicEffects (Lines 658-717)
All methods return `"not implemented in stub"` errors:
- `send_to_role_bytes`, `receive_from_role_bytes`, `broadcast_bytes`, `start_session`

**Impact:** Multi-party choreography communication completely stubbed. Distributed protocols cannot execute.

**Recommended Action:** Replace coordinator_stub with full coordinator implementation or implement missing effect methods.

### aura-relational (Consensus)

**File:** `crates/aura-relational/src/consensus.rs`

- **Line 30:** `TODO: Replace with actual FROST threshold signature components`
  - **Context:** ThresholdSignature struct is a placeholder
  - **Impact:** Consensus uses placeholder signatures instead of real FROST cryptography
  - **Security Risk:** Cannot verify consensus decisions cryptographically

- **Lines 42-53:** Stub consensus implementation
  - **Context:** `initiate_consensus` returns false without actual protocol execution
  - **Impact:** No consensus mechanism, just placeholder
  - **Blocker for:** Strong agreement, safety guarantees

### aura-protocol (Consensus Coordinator)

**File:** `crates/aura-protocol/src/consensus/coordinator.rs`

- **Line 182:** `"Epidemic gossip not yet implemented"`
  - **Context:** Fast path disabled, fallback gossip not implemented
  - **Impact:** No gossip protocol means consensus cannot fall back from fast path
  - **Availability Risk:** Network partitions will break consensus

**File:** `crates/aura-protocol/src/consensus/choreography.rs`

- **Line 316:** `"Choreography execution not yet implemented"`
  - **Impact:** Consensus choreography execution is stubbed
  - **Blocker for:** Distributed consensus protocol

### aura-authenticate (Core Authentication Flows)

**File:** `crates/aura-authenticate/src/guardian_auth.rs`

Multiple critical TODOs for guardian authentication:

- **Lines 542-543:** Guardian auth choreography not integrated
  - **Impact:** Cannot execute choreographic protocol for guardian auth

- **Lines 718, 751, 777:** Network communication not implemented
  - **Impact:** Cannot send/receive guardian approval requests

- **Line 848:** Journal state tracking not implemented
  - **Impact:** Cannot track authentication state

- **Lines 892, 902, 940, 960:** Guardian device communication completely missing
  - **Impact:** Guardian devices cannot communicate for approvals

**File:** `crates/aura-authenticate/src/authority_auth.rs`

- **Line 191:** Authority authentication not integrated with choreography runtime
- **Line 219:** Verification logic incomplete

**File:** `crates/aura-authenticate/src/session_creation.rs`

- **Line 257:** Session creation choreography not integrated

**File:** `crates/aura-authenticate/src/device_auth.rs`

- **Line 238:** Device auth choreography not integrated

**Impact:** All authentication flows incomplete - guardian auth, authority auth, session creation, device auth all missing choreography integration and network layers.

### aura-agent (Recovery & Invitations)

**File:** `crates/aura-agent/src/handlers/recovery.rs`

- **Line 55:** `"Guardian key recovery not yet implemented - requires Arc-based effect system"`
  - **Impact:** Core recovery functionality missing; cannot recover from lost devices

**File:** `crates/aura-agent/src/handlers/invitations.rs`

- **Line 34:** Device invitation not implemented
- **Line 46:** Invitation acceptance not implemented
  - **Impact:** Cannot onboard new devices or accept invitations

### aura-journal (Authority State)

**File:** `crates/aura-journal/src/authority_state.rs`

- **Lines 30-33:** Threshold signing returns error instead of signing
  - **Impact:** Cannot sign with threshold keys

- **Lines 129, 133, 137:** Critical tree operations incomplete
  - Leaf removal, threshold update, epoch rotation
  - **Impact:** Cannot manage authority membership or policies

---

## 🟠 HIGH PRIORITY

> **Important features and significant TODOs**

### aura-authenticate (DKD Protocol)

**File:** `tests/e2e_cli_dkd_test.rs`

- **Line 22:** `TODO: DKD protocol should be implemented in aura-authenticate feature crate`
  - **Impact:** Distributed Key Derivation needs proper implementation in feature crate
  - **Current State:** Test exists but protocol not in proper architectural layer

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

### aura-store (Biscuit Authorization)

**File:** `crates/aura-store/src/biscuit_authorization.rs`

- **Line 160:** `TODO: Verify token authority_id matches _authority_id`
  - **Security Risk:** Token authority verification missing
  - **Impact:** Could allow tokens from wrong authority

- **Line 345:** `TODO: These tests need to be updated for the new authority-centric API`
  - **Impact:** Test suite outdated for new architecture

### aura-authenticate (Guardian Auth Relational)

**File:** `crates/aura-authenticate/src/guardian_auth_relational.rs`

- **Line 145:** Signature verification using guardian's public key not implemented
- **Line 159:** Consensus proof verification not implemented
- **Line 228:** Time-based checks not implemented
- **Line 237:** Specific permissions checking not implemented

**Impact:** Relational guardian auth verification incomplete; security checks missing.

### aura-agent (Device Management)

**File:** `crates/aura-agent/src/runtime/authority_manager.rs`

- **Line 111:** `"Device management not yet implemented"`
  - **Impact:** Cannot manage device lifecycle (add, remove, update)

### aura-agent (Session Management)

**File:** `crates/aura-agent/src/handlers/sessions.rs`

All session management operations incomplete:

- **Line 413:** Create session through effects
- **Line 472:** Get session status
- **Line 557:** End session
- **Line 581:** List sessions
- **Line 594:** Get session statistics
- **Line 613:** Clean up sessions

**Impact:** Session management layer incomplete; cannot manage multi-party sessions properly.

### aura-verify (Identity Verification)

**File:** `crates/aura-verify/src/lib.rs`

- **Line 404:** `TODO fix - For now, we'll need an account_id to look up the group key`
  - **Impact:** Verification needs account_id lookup integration

### aura-journal (State Reduction)

**File:** `crates/aura-journal/src/reduction.rs`

- **Line 39:** Actual tree state transitions not implemented
- **Lines 182, 187:** Proper state hashing not implemented

**Impact:** State reduction pipeline incomplete; cannot deterministically reduce from facts to state.

### aura-relational (Fact Hashing)

**File:** `crates/aura-relational/src/lib.rs`

- **Line 165:** `TODO: Implement proper fact hashing`
  - **Impact:** Using placeholder for fact content hashing

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
| 🔴 Critical | 47 | aura-journal, aura-mpst, aura-agent, aura-authenticate, aura-relational |
| 🟠 High | 38 | aura-sync, aura-authenticate, aura-journal, aura-store, aura-agent |
| 🟡 Medium | 26 | aura-effects, aura-agent, aura-sync, aura-simulator, aura-testkit |
| 🟢 Low | 15 | tests/, aura-agent, aura-journal, aura-rendezvous |
| **Total** | **126** | **22 crates** |

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
