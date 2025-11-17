# Effect System Violations - Tracking Document

This document tracks remaining violations of the effect system architecture principle where production code uses `#[allow(clippy::disallowed_methods)]` to bypass RandomEffects or TimeEffects.

## Status Summary

- **Total violations audited:** 133
- **Legitimate (test code, effect implementations):** 70 (53%)
- **Production code violations:** 33 (25%)
- **Trait limitations (tracked):** 12 (9%)
- **Bootstrap code (acceptable):** 18 (13%)

## Completed Fixes

### Phase 1 (Completed - Commit 88a948f)
- ✅ LeakageTracker in aura-mpst (security-critical flow budget enforcement)
- ✅ Session ID generation in aura-agent (bug fix - was using wrong ID)
- ✅ Token generation in aura-wot (API refactored to accept random bytes)

### Phase 2 (Completed - Current)
- ✅ Removed unnecessary allow from aura-mpst ExecutionContext (deterministic UUID)
- ✅ Updated false "infrastructure is acceptable" comments with honest TODO markers

## Remaining Production Violations (31 total)

### Priority 1: CRITICAL SECURITY - Cryptographic Operations (6 violations)

**Impact**: Untestable cryptographic operations, can't verify test vectors, non-deterministic signing.

1. **`crates/aura-frost/src/threshold_signing.rs:218`**
   - FROST signing using `rand::thread_rng()`
   - Function: `sign_partial()`

2. **`crates/aura-frost/src/threshold_signing.rs:290`**
   - FROST aggregation using `rand::thread_rng()`
   - Function: `aggregate_signatures()`

3. **`crates/aura-frost/src/signature_aggregation.rs:172`**
   - Comment says "In production, this would..." but THIS IS production code
   - Function: `aggregate()`

4. **`crates/aura-core/src/crypto/tree_signing.rs:362`**
   - FROST nonce generation bypassing RandomEffects
   - Function: `generate_nonces()`

5. **`crates/aura-core/src/crypto/tree_signing.rs:440`**
   - Another FROST signing operation
   - Function: `sign_with_keypackage()`

6. **`crates/aura-protocol/src/handlers/memory/ledger_memory.rs:102`**
   - Secret generation using `rand::thread_rng()`
   - Function: `generate_secret()`

**Solution Required**: Create RNG adapter that bridges `RandomEffects` to `frost_ed25519::RngCore`.

#### FROST RNG Adapter Requirements

The `frost_ed25519` library requires an RNG implementing `rand_core::RngCore + rand_core::CryptoRng`. We need:

1. **`EffectSystemRng` struct** that wraps `&dyn RandomEffects`
2. Implements `RngCore` trait by calling `random_bytes()` internally
3. Implements `CryptoRng` marker trait (our RandomEffects is crypto-secure)
4. Handle async-to-sync conversion (FROST expects sync RNG)

**Example implementation:**
```rust
pub struct EffectSystemRng<'a> {
    effects: &'a dyn RandomEffects,
    runtime: tokio::runtime::Handle,
}

impl RngCore for EffectSystemRng<'_> {
    fn next_u32(&mut self) -> u32 {
        // Call effects.random_u64() via runtime.block_on()
        // Return lower 32 bits
    }

    fn next_u64(&mut self) -> u64 {
        self.runtime.block_on(self.effects.random_u64())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let bytes = self.runtime.block_on(
            self.effects.random_bytes(dest.len())
        );
        dest.copy_from_slice(&bytes);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for EffectSystemRng<'_> {}
```

**Complexity**: Medium - requires async-to-sync conversion, but straightforward.

### Priority 2: HIGH - Infrastructure Timing (22 violations)

**Impact**: Infrastructure timing affects protocol decisions, resource management, and must be testable.

#### aura-sync Infrastructure (18 violations)

**All marked with updated TODO comments clarifying they are violations, not exemptions.**

**Peers (3 violations):**
- `crates/aura-sync/src/infrastructure/peers.rs:280` - Peer refresh tracking
- `crates/aura-sync/src/infrastructure/peers.rs:296` - Peer discovery timing
- `crates/aura-sync/src/infrastructure/peers.rs:275` (from `PeerMetadata::new`)

**Connections (4 violations):**
- `crates/aura-sync/src/infrastructure/connections.rs:272` - Connection acquisition
- `crates/aura-sync/src/infrastructure/connections.rs:325` - Connection release
- `crates/aura-sync/src/infrastructure/connections.rs:119` (from `ConnectionMetadata::new`)
- `crates/aura-sync/src/infrastructure/connections.rs:212` (from `ConnectionHandle::new`)

**Sessions (3 violations):**
- `crates/aura-sync/src/core/session.rs:267` - Session creation
- `crates/aura-sync/src/core/session.rs:277` - Session metrics
- `crates/aura-sync/src/core/session.rs:542` - Cleanup timing

**Metrics (1 violation):**
- `crates/aura-sync/src/core/metrics.rs:317` - Sync start recording

**Protocols (3 violations):**
- `crates/aura-sync/src/protocols/journal.rs:210` - Duration measurement
- `crates/aura-sync/src/protocols/ota.rs:189, 293` - OTA timing

**Services (2 violations):**
- `crates/aura-sync/src/services/maintenance.rs:457` - Service lifecycle
- `crates/aura-sync/src/services/sync.rs:259` - Service lifecycle

**Solution**: Refactor methods to accept `now: Instant` parameter from caller's TimeEffects access.

#### aura-protocol Transport (3 violations)

- `crates/aura-protocol/src/handlers/transport_coordinator.rs:207` - Connection metadata
- `crates/aura-protocol/src/handlers/transport_coordinator.rs:232` - Connection tracking
- `crates/aura-protocol/src/handlers/transport_coordinator.rs:294` - Coordination timing

**Solution**: TransportCoordinator already has TimeEffects access - use it!

#### aura-rendezvous (3 violations)

- `crates/aura-rendezvous/src/connection_manager.rs:502, 515` - Connection timing
- `crates/aura-rendezvous/src/integrated_sbb.rs:281` - SBB timing
- `crates/aura-rendezvous/src/capability_aware_sbb.rs:538` - Capability timing

**Solution**: Refactor to accept time parameter.

### Priority 3: MEDIUM - Other Infrastructure (3 violations)

#### aura-authenticate (2 violations)
- `crates/aura-authenticate/src/guardian_auth.rs:353, 582` - Guardian authentication timing

#### aura-sync Snapshots (1 violation)
- `crates/aura-sync/src/protocols/snapshots.rs:289` - Snapshot finalization

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

1. **Phase 3**: Fix aura-sync infrastructure timing (18 violations)
2. **Phase 4**: Create FROST RNG adapter and fix cryptographic violations (6 violations)
3. **Phase 5**: Fix aura-protocol transport coordinator (3 violations)
4. **Phase 6**: Address trait evolution needs (coordinated effort)

## References

- Initial audit: Effect system violation audit (see git history)
- Phase 1 fixes: Commit 88a948f
- Phase 2 fixes: Current commit
- Architecture: docs/002_system_architecture.md (Effect System section)
