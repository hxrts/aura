# Effect System Violations - Tracking Document

This document tracks remaining violations of the effect system architecture principle where production code uses `#[allow(clippy::disallowed_methods)]` to bypass RandomEffects or TimeEffects.

## Status Summary

- **Total violations audited:** 133
- **Legitimate (test code, effect implementations):** 70 (53%)
- **Production code violations remaining:** 7 (5%) - down from 12
- **Production code violations fixed:** 26 (20%)
- **Trait limitations (tracked):** 12 (9%)
- **Bootstrap code (acceptable):** 18 (13%)

## Completed Fixes

### Phase 1 (Completed - Commit 88a948f)
- ✅ LeakageTracker in aura-mpst (security-critical flow budget enforcement)
- ✅ Session ID generation in aura-agent (bug fix - was using wrong ID)
- ✅ Token generation in aura-wot (API refactored to accept random bytes)

### Phase 2 (Completed - Commit 21ecda6)
- ✅ Removed unnecessary allow from aura-mpst ExecutionContext (deterministic UUID)
- ✅ Updated false "infrastructure is acceptable" comments with honest TODO markers

### Phase 3 (Completed - Current)
- ✅ Fixed aura-sync infrastructure timing violations (4 locations):
  - `aura-sync/src/infrastructure/peers.rs:270` - discover_peers now accepts `now: Instant` parameter
  - `aura-sync/src/infrastructure/peers.rs:285` - add_peer now accepts `now: Instant` parameter
  - `aura-sync/src/infrastructure/connections.rs:267` - acquire now accepts `now: Instant` parameter
  - `aura-sync/src/infrastructure/connections.rs:318` - release now accepts `now: Instant` parameter
- ✅ Updated all test code to pass `now` parameter from test fixtures
- ✅ Updated documentation examples to show correct usage

### Phase 4 (Completed - Commit eec77f3)
- ✅ Created EffectSystemRng adapter bridging async RandomEffects to sync RngCore
- ✅ Fixed all 6 FROST cryptographic violations:
  - `aura-frost/src/threshold_signing.rs:218` - generate_partial_signature now accepts RandomEffects
  - `aura-frost/src/threshold_signing.rs:290` - aggregate_signatures now accepts RandomEffects
  - `aura-frost/src/signature_aggregation.rs:172` - perform_frost_aggregation now accepts RandomEffects
  - `aura-core/src/crypto/tree_signing.rs:362` - generate_nonce_with_share now accepts RngCore parameter
  - `aura-core/src/crypto/tree_signing.rs:440` - frost_sign_partial_with_keypackage now accepts RngCore parameter
  - `aura-protocol/src/handlers/memory/ledger_memory.rs:102` - MemoryLedgerHandler now stores RandomEffects dependency

### Phase 5 (Completed - Commit b26b3c2)
- ✅ Fixed remaining aura-sync timing and randomness violations (6 violations):
  - `aura-sync/src/core/session.rs:325` - create_session now accepts `now: Instant` parameter
  - `aura-sync/src/protocols/journal.rs:216` - sync_with_peers now accepts `start: Instant` parameter
  - `aura-sync/src/protocols/ota.rs:192` - propose_upgrade now accepts `proposal_id: Uuid` parameter
  - `aura-sync/src/protocols/snapshots.rs:295` - commit now accepts `completion_id: Uuid` parameter
  - `aura-sync/src/services/maintenance.rs:458` - Service::start trait now accepts `now: Instant` parameter
  - `aura-sync/src/services/sync.rs:260` - SyncService::start now uses `now` parameter
- ✅ Updated all test code to pass time/UUID parameters
- ✅ Updated Service trait signature to require `now` parameter

### Phase 6 (Completed - Commit e48bbda)
- ✅ Fixed aura-protocol transport coordinator timing violations (3 violations):
  - `aura-protocol/src/handlers/transport_coordinator.rs:207` - Connection metadata now uses TimeEffects
  - `aura-protocol/src/handlers/transport_coordinator.rs:232` - Connection tracking now uses TimeEffects
  - `aura-protocol/src/handlers/transport_coordinator.rs:294` - Stale connection cleanup now uses TimeEffects
- ✅ Renamed `_effects` field to `effects` to enable usage
- ✅ All connection lifecycle timing now uses `self.effects.now_instant().await` for testability

### Phase 7 (Completed - Current)
- ✅ Fixed aura-rendezvous timing violations (4 violations):
  - `aura-rendezvous/src/connection_manager.rs:503/516` - Refactored establish_connection_with_punch to accept `start_time` parameter
  - `aura-rendezvous/src/integrated_sbb.rs:282` - Refactored cleanup_expired_data to accept `current_time` parameter
  - `aura-rendezvous/src/capability_aware_sbb.rs` - Removed current_timestamp() helper, refactored SbbFlowBudget methods to accept `now` parameter
  - `aura-rendezvous/src/sbb.rs` - Updated SbbFlooding trait to require `now` parameter in flood_envelope
- ✅ Fixed aura-authenticate guardian verification (1 violation):
  - `aura-authenticate/src/guardian_auth.rs:355` - Refactored verify_guardian_challenge to accept `now` parameter

## Remaining Production Violations (7 total)

### Priority 1: CRITICAL SECURITY - Cryptographic Operations ✅ COMPLETED (Phase 4)

**Status**: All 6 violations fixed

**Solution Implemented**:
- Created `EffectSystemRng` adapter in `aura-effects/src/crypto.rs`
- Bridges async `RandomEffects` to sync `rand::RngCore + rand::CryptoRng`
- Uses `tokio::runtime::Handle::block_on()` for async-to-sync conversion
- All FROST functions now accept RandomEffects or RngCore parameters
- MemoryLedgerHandler refactored to store RandomEffects dependency

**Testing**: Adapter includes comprehensive unit tests for deterministic behavior.

### Priority 2: HIGH - Infrastructure Timing (4 violations remaining)

**Impact**: Infrastructure timing affects protocol decisions, resource management, and must be testable.

**Fixed in Phase 3** (4 violations):
- ✅ `aura-sync/src/infrastructure/peers.rs` - discover_peers and add_peer
- ✅ `aura-sync/src/infrastructure/connections.rs` - acquire and release

**Fixed in Phase 5** (6 violations):
- ✅ `aura-sync/src/core/session.rs` - create_session
- ✅ `aura-sync/src/protocols/journal.rs` - sync_with_peers
- ✅ `aura-sync/src/protocols/ota.rs` - propose_upgrade (UUID)
- ✅ `aura-sync/src/protocols/snapshots.rs` - commit (UUID)
- ✅ `aura-sync/src/services/maintenance.rs` - Service::start
- ✅ `aura-sync/src/services/sync.rs` - Service::start

**Fixed in Phase 6** (3 violations):
- ✅ `aura-protocol/src/handlers/transport_coordinator.rs` - Connection metadata, tracking, and cleanup

**Fixed in Phase 7** (5 violations):
- ✅ `aura-rendezvous/src/connection_manager.rs` - Connection timing for hole punching
- ✅ `aura-rendezvous/src/integrated_sbb.rs` - SBB cleanup timing
- ✅ `aura-rendezvous/src/capability_aware_sbb.rs` - Flow budget and timestamp operations
- ✅ `aura-authenticate/src/guardian_auth.rs` - Guardian challenge verification

#### aura-sync Infrastructure (3 violations remaining - all already fixed, documentation outdated)

**Peers (0 violations - fixed in Phase 3):**
- ✅ Fixed: PeerMetadata::new already accepts `now` parameter

**Connections (0 violations - fixed in Phase 3):**
- ✅ Fixed: ConnectionMetadata::new and ConnectionHandle::new already accept `now` parameter

**Sessions (0 violations - all fixed in Phase 5):**
- ✅ Fixed: Session creation, metrics, and cleanup now use `now` parameter

**Metrics (0 violations - fixed in Phase 5):**
- ✅ Fixed: Sync start recording now uses `now` parameter

**Protocols (0 violations - all fixed in Phase 5):**
- ✅ Fixed: Duration measurement and OTA timing

**Services (0 violations - all fixed in Phase 5):**
- ✅ Fixed: Service lifecycle now accepts `now` parameter

#### aura-protocol Transport (0 violations - all fixed in Phase 6)
- ✅ Fixed: All connection lifecycle timing now uses TimeEffects

#### aura-rendezvous (0 violations - all fixed in Phase 7)
- ✅ Fixed: All connection timing, SBB cleanup, and capability timing operations

### Priority 3: MEDIUM - Other Infrastructure (4 violations remaining)

#### aura-authenticate (4 violations remaining)

These violations are in placeholder/TODO code that requires major refactoring:

- `crates/aura-authenticate/src/guardian_auth.rs:496` - Request timestamp age check
- `crates/aura-authenticate/src/guardian_auth.rs:534` - Guardian challenge generation (uses time as nonce)
- `crates/aura-authenticate/src/guardian_auth.rs:691/798/848` - Approval/challenge timestamps in execute methods

**Note**: These are in MVP placeholder code with extensive TODOs. The guardian authentication system needs comprehensive refactoring to integrate with the effect system, aura-wot capabilities, and proper network effects. Fixing individual timing calls without this broader refactoring would provide limited value.

**Solution**: Defer until guardian auth system is properly implemented with effect system integration.

#### aura-sync Snapshots (0 violations - fixed in Phase 5)
- ✅ Fixed: Snapshot finalization now accepts UUID parameter

## Trait Evolution Needed (12 violations)

These have legitimate architectural constraints that require trait signature changes:

1. **`ReliabilityEffects` trait** - Needs TimeEffects or time parameter
   - `aura-agent/src/runtime/reliability.rs:353`

2. **Memory handlers** - Need RandomEffects/TimeEffects integration
   - `aura-protocol/src/handlers/memory/*.rs` (multiple files)

3. **Bridge implementations** - Trait signatures don't support effects yet
   - Various bridge and factory files

**Solution**: Track with existing TODO comments, update traits in coordinated effort.

## Legitimate Uses (Keep)

### Category A: Core Effect Injection (28)
- ID constructors (`DeviceId::new()`, `SessionId::new()`, etc.) in aura-core
- These are intentional injection points where effects provide the randomness

### Category B: Test Code (42)
- All `#[test]` functions and test modules
- Test fixtures and mocks
- Acceptable for test-only code

### Category C: Bootstrap (18)
- One-time initialization of effect system itself
- Base reference points for synthetic time
- Must have clear explanatory comments

## Guidelines for New Code

### When `#[allow(clippy::disallowed_methods)]` is NEVER Acceptable:
1. ❌ Production protocol code
2. ❌ Infrastructure that affects protocol behavior
3. ❌ Resource management (connections, peers, sessions)
4. ❌ Security features (flow budgets, rate limits, authentication)
5. ❌ Any code that needs to be tested deterministically

### When `#[allow]` MAY Be Acceptable:
1. ✅ Test code (`#[test]` functions)
2. ✅ Effect implementation code (the actual `impl RandomEffects` or `impl TimeEffects`)
3. ✅ Core injection points (ID constructors in aura-core)
4. ✅ One-time bootstrap initialization (with clear comment explaining why)

### If You Think You Need `#[allow]`:
1. **First**: Can you accept time/randomness as a parameter?
2. **Second**: Can you make the function async and use TimeEffects/RandomEffects?
3. **Third**: Is this truly a test-only function?
4. **Only then**: Add `#[allow]` with a detailed TODO comment explaining:
   - WHY the effect system can't be used (trait limitation? bootstrap?)
   - WHAT needs to change to remove the allow
   - WHEN this should be addressed (link to issue if tracked)

## Next Steps

1. ~~**Phase 3**: Fix aura-sync infrastructure timing (4 violations in peers/connections)~~ ✅ COMPLETED
2. ~~**Phase 4**: Create FROST RNG adapter and fix cryptographic violations (6 violations)~~ ✅ COMPLETED
3. ~~**Phase 5**: Fix remaining aura-sync timing violations (6 violations)~~ ✅ COMPLETED
4. ~~**Phase 6**: Fix aura-protocol transport coordinator (3 violations)~~ ✅ COMPLETED
5. ~~**Phase 7**: Fix aura-rendezvous and verification violations (5 violations)~~ ✅ COMPLETED
6. **Phase 8**: Address aura-authenticate placeholder code (4 violations) - Deferred pending major refactoring
7. **Phase 9**: Address trait evolution needs (coordinated effort)

## References

- Initial audit: Effect system violation audit (see git history)
- Phase 1 fixes: Commit 88a948f
- Phase 2 fixes: Commit 21ecda6
- Phase 3 fixes: Commit 6d52ec2
- Phase 4 fixes: Commit eec77f3
- Phase 5 fixes: Commit b26b3c2
- Phase 6 fixes: Commit e48bbda
- Phase 7 fixes: Current commit
- Architecture: docs/002_system_architecture.md (Effect System section)
- FROST RNG Adapter: crates/aura-effects/src/crypto.rs (EffectSystemRng)
