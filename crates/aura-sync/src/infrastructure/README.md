# Infrastructure Module - Public API Design

This document describes the public API design for the `aura-sync::infrastructure` module following Layer 5 (Feature/Protocol) guidelines and effect system patterns.

## Design Principles

All infrastructure components follow these architectural principles:

### 1. **Effect-Based Interfaces**
- No direct dependencies on concrete effect handlers
- Parameterized by effect trait bounds where needed
- Composable through trait-based abstractions

### 2. **Stateless Where Possible**
- Individual operations are context-free
- State is explicit and managed through clear APIs
- No ambient or global mutable state

### 3. **Clean API Boundaries**
- Minimal re-exports from other layers
- Users import from appropriate layer crates directly
- Clear separation of concerns

## Module Structure

```
infrastructure/
├── mod.rs           # Public exports and module organization
├── peers.rs         # Peer discovery and management
├── retry.rs         # Retry logic with exponential backoff
├── cache.rs         # Cache management with epoch tracking
├── connections.rs   # Connection pool management
└── rate_limit.rs    # Rate limiting for flow budget enforcement
```

## Public API Surface

### Core Re-exports

```rust
// From infrastructure/mod.rs
pub use peers::{PeerManager, PeerInfo, PeerDiscoveryConfig, PeerStatus, PeerMetadata};
pub use retry::{RetryPolicy, BackoffStrategy, RetryContext, RetryResult};
pub use cache::{CacheManager, CacheEpochTracker, CacheInvalidation};
pub use connections::{ConnectionPool, ConnectionMetadata, PoolConfig};
pub use rate_limit::{RateLimiter, RateLimit, RateLimitConfig, RateLimitResult};
```

### Peer Management API

**Key Types:**
- `PeerManager` - Main interface for peer discovery and selection
- `PeerInfo` - Detailed peer information including capabilities
- `PeerMetadata` - Connection status and performance tracking
- `PeerDiscoveryConfig` - Configuration for discovery behavior

**Usage Pattern:**
```rust
use aura_sync::infrastructure::{PeerManager, PeerDiscoveryConfig};
use aura_core::effects::NetworkEffects;

async fn discover_and_select_peers<E>(effects: &E) -> Result<Vec<DeviceId>, SyncError>
where
    E: NetworkEffects + Send + Sync,
{
    let config = PeerDiscoveryConfig::default();
    let mut manager = PeerManager::new(config);

    // Discover available peers
    manager.discover_peers(effects).await?;

    // Select best peers for sync
    let selected = manager.select_sync_peers(5);
    Ok(selected)
}
```

**Integration Points:**
- **aura-rendezvous**: `discover_peers()` will integrate with `DiscoveryService`
- **aura-wot**: `update_peer_capabilities()` validates via `CapabilityEvaluator`
- **aura-verify**: Peer identity verification before tracking

### Retry Logic API

**Key Types:**
- `RetryPolicy` - Configurable retry behavior
- `BackoffStrategy` - Exponential, linear, or fixed backoff
- `RetryResult<T, E>` - Result with attempt statistics

**Usage Pattern:**
```rust
use aura_sync::infrastructure::{RetryPolicy, with_exponential_backoff};
use std::time::Duration;

// Builder pattern
let policy = RetryPolicy::exponential()
    .with_max_attempts(5)
    .with_initial_delay(Duration::from_millis(100))
    .with_jitter(true);

// Execute with retry
let result = policy.execute(|| async {
    perform_sync_operation().await
}).await?;

// Convenience function
let result = with_exponential_backoff(
    || async { perform_sync_operation().await },
    5
).await?;
```

### Cache Management API

**Key Types:**
- `CacheManager` - High-level cache interface
- `CacheEpochTracker` - Epoch-based invalidation tracking
- `CacheInvalidation` - Invalidation event

**Usage Pattern:**
```rust
use aura_sync::infrastructure::CacheManager;
use aura_core::tree::Epoch;

let mut cache = CacheManager::new();

// Invalidate keys at epoch boundary
cache.invalidate_keys(&["key1", "key2"], 10_u64);

// Check freshness
if cache.is_fresh("key1", current_epoch) {
    // Use cached data
} else {
    // Refresh data
}
```

**Integration Points:**
- **Maintenance events**: `apply_invalidation()` processes `MaintenanceEvent::CacheInvalidated`
- **OTA coordination**: Epoch floors updated on upgrade activation

### Connection Pool API

**Key Types:**
- `ConnectionPool` - Connection lifecycle management
- `ConnectionHandle` - RAII handle for acquired connections
- `ConnectionMetadata` - Connection state and metrics
- `PoolConfig` - Pool behavior configuration

**Usage Pattern:**
```rust
use aura_sync::infrastructure::{ConnectionPool, PoolConfig};

let config = PoolConfig::default();
let mut pool = ConnectionPool::new(config);

// Acquire connection
let conn = pool.acquire(peer_id).await?;

// Use connection...
perform_sync(&conn).await?;

// Release back to pool
pool.release(peer_id, conn)?;

// Cleanup
pool.evict_expired();
```

**Integration Points:**
- **aura-transport**: `acquire()` will use `TransportHandler` for actual connections
- **aura-rendezvous**: `SecureChannel` establishment through rendezvous

### Rate Limiting API

**Key Types:**
- `RateLimiter` - Token bucket-based rate limiting
- `RateLimitResult` - Allow/deny with retry-after
- `RateLimitConfig` - Global and per-peer limits

**Usage Pattern:**
```rust
use aura_sync::infrastructure::{RateLimiter, RateLimitConfig};

let config = RateLimitConfig::default();
let mut limiter = RateLimiter::new(config);

// Check before operation
match limiter.check_rate_limit(peer_id, cost).await {
    RateLimitResult::Allowed => {
        // Proceed with operation
    }
    RateLimitResult::Denied { retry_after, reason } => {
        // Backoff and retry
        tokio::time::sleep(retry_after).await;
    }
}
```

**Integration Points:**
- **FlowBudget**: Integrates with `aura-core::FlowBudget` for privacy enforcement
- **Guard chain**: Used by `FlowGuard` for budget enforcement

## API Evolution

### Phase 2 (Current) - ✅ Complete
- [x] Peer management with capability filtering
- [x] Retry logic with configurable backoff
- [x] Cache management with epoch tracking
- [x] Connection pooling infrastructure
- [x] Rate limiting for flow budgets

### Phase 3 (Next)
- [ ] Full aura-rendezvous integration in `PeerManager::discover_peers()`
- [ ] aura-transport connection establishment in `ConnectionPool::acquire()`
- [ ] aura-verify identity verification in peer tracking
- [ ] aura-wot capability evaluation in peer selection

### Phase 4
- [ ] Service-level coordination using infrastructure
- [ ] Health check integration
- [ ] Metrics aggregation

### Phase 5
- [ ] Performance optimization
- [ ] Advanced connection strategies
- [ ] Adaptive rate limiting

## Testing Strategy

Each infrastructure component includes:

### Unit Tests
- Core logic validation
- Edge case handling
- Configuration validation

### Integration Tests (Phase 3+)
- Effect system integration
- Multi-peer coordination
- Network simulation

### Property Tests (Phase 5)
- Rate limiter token bucket properties
- Connection pool invariants
- Cache consistency

## Documentation Standards

All public APIs include:
- **Module-level docs**: Architecture and usage overview
- **Type docs**: Purpose and integration points
- **Method docs**: Parameters, returns, integration notes
- **Examples**: Realistic usage patterns

## Anti-Patterns to Avoid

❌ **Direct handler dependencies**
```rust
// BAD: Depends on concrete handler
pub fn discover(transport: &TcpNetworkHandler) { }

// GOOD: Effect-based
pub async fn discover<E>(effects: &E) where E: NetworkEffects { }
```

❌ **Global mutable state**
```rust
// BAD: Global state
static mut PEER_CACHE: HashMap<DeviceId, PeerInfo> = ...;

// GOOD: Explicit state management
pub struct PeerManager {
    peers: HashMap<DeviceId, PeerInfo>,
}
```

❌ **Layer violations**
```rust
// BAD: Infrastructure implementing protocol logic
impl PeerManager {
    pub async fn sync_journal(&self) { } // Protocol logic
}

// GOOD: Infrastructure provides building blocks
impl PeerManager {
    pub fn select_sync_peers(&self, count: usize) -> Vec<DeviceId>
}
```

## Migration Notes

### From Legacy Code

Old code using deprecated modules should migrate to infrastructure:

```rust
// OLD: Phase 1 placeholder
use aura_sync::peer_discovery::PeerDiscoveryService;

// NEW: Phase 2 infrastructure
use aura_sync::infrastructure::{PeerManager, PeerDiscoveryConfig};
```

```rust
// OLD: Legacy cache
use aura_sync::cache::CacheEpochFloors;

// NEW: Infrastructure cache
use aura_sync::infrastructure::CacheEpochTracker;
```

### Deprecation Timeline

- **Phase 2**: Infrastructure available, legacy deprecated
- **Phase 3**: Protocol migration uses new infrastructure
- **Phase 4**: Service layer adopts infrastructure
- **Phase 5**: All legacy code removed

## Contributing Guidelines

When adding new infrastructure:

1. **Follow Layer 5 patterns**: Reusable building blocks, not applications
2. **Effect-based design**: Parameterize by traits, not concrete types
3. **Clean boundaries**: Import from appropriate layers
4. **Comprehensive docs**: Architecture, usage, integration points
5. **Test coverage**: Unit tests minimum, integration tests preferred
6. **Migration path**: Provide examples for legacy code users

## API Stability

- **Core interfaces**: Stable after Phase 2
- **Integration details**: May evolve through Phase 3
- **Performance optimizations**: Phase 5 without breaking changes
- **Deprecation policy**: Minimum 2 phases notice
