# Aura Agent (Layer 6) - Architecture and Invariants

## Purpose
Production runtime composition and effect system assembly for authority-based
identity management. Owns effect registry, builder infrastructure, context
management, and choreography adapters.

## Inputs
- All lower layers (Layers 1-5): core types, effect traits, domain crates, protocols.
- Authority identifiers (`AuthorityId`) and context (`ContextId`, `SessionId`).
- Effect implementations from aura-effects.
- Protocol coordination from aura-protocol.

## Outputs
- `AgentBuilder`, `AuraAgent`, `EffectContext`, `EffectRegistry`.
- `AuraEffectSystem`, `EffectSystemBuilder`, `EffectExecutor`.
- Services: `SessionServiceApi`, `AuthServiceApi`, `RecoveryServiceApi`, `SyncManagerState`.
- `RuntimeSystem`, `LifecycleManager`, `ReceiptManager`, `FlowBudgetManager`.
- `ReactiveScheduler` for signal-based notification.

## Naming
- API-facing services use the `*ServiceApi` suffix (e.g., `AuthServiceApi`).
- Runtime/internal services live under `runtime/services` and use `*Manager` or `*Service`.

## Invariants
- Must NOT create new effect implementations (delegate to aura-effects).
- Must NOT implement multi-party coordination (delegate to aura-protocol).
- Must NOT be imported by Layers 1-5 (prevents circular dependencies).
- Authority-first design: all operations scoped to specific authorities.
- Lazy composition: effects assembled on-demand.
- Mode-aware execution: production, testing, and simulation use same API.

## Boundaries
- Stateless handlers live in aura-effects.
- Protocol logic lives in aura-protocol.
- Application core lives in aura-app.

---

# AuraEffectSystem Subsystem Architecture

The `AuraEffectSystem` organizes related fields into subsystems for better
maintainability and clearer ownership boundaries.

## Subsystem Overview

| Subsystem | Module | Purpose |
|-----------|--------|---------|
| `CryptoSubsystem` | `runtime/subsystems/crypto.rs` | Cryptographic operations, RNG, secure key storage |
| `TransportSubsystem` | `runtime/subsystems/transport.rs` | Network transport, inbox management, statistics |
| `JournalSubsystem` | `runtime/subsystems/journal.rs` | Indexed journal, fact registry, reactive publication |

## CryptoSubsystem

Groups cryptographic fields:
- `handler`: `RealCryptoHandler` - signing, verification, key operations
- `rng`: `Mutex<StdRng>` - cryptographically secure random number generation
- `secure_storage`: `Arc<RealSecureStorageHandler>` - platform secure storage (Keychain/TPM)

**Key methods:**
- `handler()` - get crypto handler reference
- `secure_storage()` - get Arc to secure storage
- `random_bytes(len)` - generate random bytes
- `random_u64()` - generate random u64
- `random_32_bytes()` - generate [u8; 32]

## TransportSubsystem

Groups network transport fields:
- `handler`: `RealTransportHandler` - send/receive operations
- `inbox`: `Arc<RwLock<Vec<TransportEnvelope>>>` - incoming message queue
- `shared_transport`: `Option<SharedTransport>` - simulation mode shared network
- `stats`: `Arc<RwLock<TransportStats>>` - transport metrics

**Key methods:**
- `handler()` - get transport handler reference
- `inbox()` - get shared inbox Arc
- `queue_envelope(env)` - push to inbox
- `drain_inbox()` - drain all envelopes
- `update_stats(closure)` - update statistics
- `stats_snapshot()` - get current stats
- `shared_transport()` - get optional shared transport

## JournalSubsystem

Groups journal and fact management fields:
- `indexed_journal`: `Arc<IndexedJournalHandler>` - B-tree indices, Bloom filters, Merkle proofs
- `fact_registry`: `Arc<FactRegistry>` - domain fact reducers and validators
- `fact_publish_tx`: `Mutex<Option<Sender<FactSource>>>` - reactive scheduler publication
- `journal_policy`: `Option<(Biscuit, Bridge)>` - authorization for journal operations
- `journal_verifying_key`: `Option<Vec<u8>>` - signature verification

**Key methods:**
- `indexed_journal()` - get journal handler Arc
- `fact_registry()` - get fact registry Arc
- `attach_fact_sink(tx)` - attach reactive scheduler channel
- `fact_publisher()` - get cloned sender if attached
- `journal_policy()` - get authorization policy reference
- `journal_verifying_key()` - get verification key slice

## AuraEffectSystem Field Organization

After subsystem extraction, `AuraEffectSystem` has 17 fields organized as:

```
AuraEffectSystem
├── Core Configuration
│   ├── config: AgentConfig
│   ├── authority_id: AuthorityId
│   └── execution_mode: ExecutionMode
├── Subsystems
│   ├── crypto: CryptoSubsystem
│   ├── transport: TransportSubsystem
│   └── journal: JournalSubsystem
├── Composition & Handlers
│   └── composite: CompositeHandlerAdapter
├── Storage Infrastructure
│   ├── storage_handler: Arc<EncryptedStorage<...>>
│   ├── tree_handler: PersistentTreeHandler
│   └── sync_handler: PersistentSyncHandler
├── Time Services
│   ├── time_handler: EnhancedTimeHandler
│   ├── logical_clock: LogicalClockService
│   └── order_clock: OrderClockHandler
├── Authorization & Flow Control
│   ├── authorization_handler: WotAuthorizationHandler
│   └── leakage_handler: ProductionLeakageHandler
├── Reactive System
│   └── reactive_handler: ReactiveHandler
└── Choreography State
    └── choreography_state: RwLock<ChoreographyState>
```

---

# Concurrency Model

This document describes the concurrency patterns and lock usage in the aura-agent runtime.

## Lock-Safety Enforcement

- The crate enables `clippy::await_holding_lock` to catch `.await` points while holding
  blocking locks (`std::sync`/`parking_lot`) or async locks in critical sections.
- Debug builds also start a `parking_lot` deadlock detector thread to surface
  lock ordering regressions early during development.

## Lock Type Selection

The runtime uses three different lock types, each chosen for specific characteristics:

### 1. `parking_lot::Mutex` / `parking_lot::RwLock`

**Use cases:**
- RNG state (`crypto.rng` in `CryptoSubsystem`)
- Channel senders (`journal.fact_publish_tx` in `JournalSubsystem`)
- Transport inbox (`transport.inbox` in `TransportSubsystem`)
- Transport statistics (`transport.stats` in `TransportSubsystem`)
- Choreography state (`choreography_state` in `AuraEffectSystem`)
- Runtime task registry (JoinHandle storage)

**Selection criteria:**
- Operations are synchronous only (no `.await` inside critical section)
- Lock hold duration is brief (sub-millisecond)
- No async work required while holding lock
- Better performance than std::sync for uncontended cases
- No lock poisoning (acceptable for these use cases)

**Example pattern:**
```rust
// Brief synchronous access - parking_lot is appropriate
// CryptoSubsystem handles this internally:
let bytes = self.crypto.random_bytes(32);
// Lock acquired and released within the method

// TransportSubsystem update pattern:
self.transport.update_stats(|stats| {
    stats.envelopes_sent += 1;
});
// Lock released after closure completes
```

### 2. `tokio::sync::RwLock`

**Use cases:**
- Service managers (SyncServiceManager, RendezvousManager, etc.)
- Handler state (InvitationHandler, AuthHandler, etc.)
- Signal views (app_signal_views)
- Receipt storage
- Ceremony tracker state

**Selection criteria:**
- Lock may be held across `.await` points
- Async operations needed inside critical section
- Fair scheduling matters for read-heavy workloads
- Cooperative yielding beneficial for async runtime

**Example pattern:**
```rust
// Lock held across await - must use tokio::sync
let mut guard = self.state.write().await;
let result = self.effects.some_async_operation().await?;
guard.update(result);
// Lock released after async work
```

### 3. `std::sync::RwLock`

**Use cases:**
- Effect registry
- Authority manager

**Selection criteria:**
- Lock poisoning detection is required
- Code explicitly handles `PoisonError`
- Critical state where corruption must be detected
- Brief synchronous operations only

**Example pattern:**
```rust
// Poisoning detection required
let guard = self.state.write().map_err(|_| Error::LockPoisoned)?;
// If previous holder panicked, we detect it
```

## Lock Ordering Rules

To prevent deadlocks, locks must be acquired in the following order:

1. **Subsystem locks** (if multiple subsystems needed):
   - Crypto subsystem
   - Transport subsystem
   - Journal subsystem
   - Choreography subsystem

2. **Within subsystems**, acquire in declaration order

3. **Never hold a parking_lot lock while awaiting**

4. **Never hold multiple RwLock write guards simultaneously** (prefer transactions)

## Expected Lock Hold Durations

| Lock | Subsystem | Expected Duration | Maximum Duration |
|------|-----------|-------------------|------------------|
| `crypto.rng` | CryptoSubsystem | <1 microsecond | 10 microseconds |
| `journal.fact_publish_tx` | JournalSubsystem | <1 microsecond | 10 microseconds |
| `transport.inbox` | TransportSubsystem | <10 microseconds | 100 microseconds |
| `transport.stats` | TransportSubsystem | <1 microsecond | 10 microseconds |
| Service state (tokio) | Service managers | 1-100 milliseconds | 1 second |
| Ceremony tracker | CeremonyTracker | 1-10 milliseconds | 100 milliseconds |

## Contention Monitoring

For production deployments, monitor:

1. **Lock wait times**: If p99 exceeds 10x expected duration, investigate
2. **Lock hold times**: If exceeding maximum, review critical section code
3. **Deadlock detection**: Enable `parking_lot` deadlock detection in debug builds

### Debug Configuration

```rust
#[cfg(debug_assertions)]
fn enable_deadlock_detection() {
    parking_lot::deadlock::check_deadlock();
}
```

## Performance Guidelines

1. **Minimize critical sections**: Do preparation work before acquiring locks
2. **Clone-then-release**: Clone data out of locks rather than holding while processing
3. **Batch operations**: Combine multiple small updates into single lock acquisition
4. **Consider sharding**: For high-contention data, use `dashmap` or manual sharding

## Anti-Patterns to Avoid

### DON'T: Hold parking_lot lock across await
```rust
// BAD - will block the async runtime
let guard = self.data.lock();
self.async_operation().await;  // WRONG
drop(guard);
```

### DON'T: Nested locks without ordering
```rust
// BAD - potential deadlock
let a = self.lock_a.lock();
let b = self.lock_b.lock();  // What if another thread holds b and wants a?
```

### DO: Release before await
```rust
// GOOD - release sync lock before async work
let data = {
    let guard = self.data.lock();
    guard.clone()
};
self.async_operation(data).await;
```

### DO: Use consistent ordering
```rust
// GOOD - always acquire in same order
let a = self.lock_a.lock();
let b = self.lock_b.lock();
// Document this order in code comments
```
