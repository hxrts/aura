# Standard Orchestration Patterns

This document describes standard coordination patterns available in `aura-protocol` (Layer 4: Orchestration). These patterns solve common distributed systems challenges with reusable, well-tested implementations.

## Overview

Orchestration patterns in Aura handle multi-party coordination, stateful composition, and cross-cutting distributed concerns. They build on the stateless effect handlers from `aura-effects` (Layer 3) to provide higher-level coordination primitives.

**When to use orchestration patterns:**
- Coordinating multiple effect handlers
- Managing multi-party distributed protocols
- Implementing stateful coordination logic
- Handling cross-cutting concerns (retries, timeouts, circuit breakers)

## Available Patterns

### 1. CRDT Coordination Pattern

**Purpose**: Synchronize distributed state using Conflict-Free Replicated Data Types

**Location**: `crates/aura-protocol/src/effects/semilattice/crdt_coordinator.rs`

**Use cases**:
- Synchronizing journal state across devices
- Coordinating capability sets with meet-semilattice operations
- Managing distributed counters and registers

**Example**:
```rust
use aura_protocol::effects::semilattice::CrdtCoordinator;
use aura_journal::JournalState;

// Create coordinator for state-based CRDT
let coordinator = CrdtCoordinator::with_cv_state(device_id, journal_state);

// Synchronize state with peer
let sync_request = coordinator.create_sync_request(peer_id)?;
effects.send_to_peer(peer_id, sync_request).await?;

// Handle sync response
let response = effects.receive_from_peer(peer_id).await?;
coordinator.handle_sync_response(response)?;
```

**Key features**:
- Supports 4 CRDT types: CvRDT, CmRDT, Delta-CRDT, MvRDT
- Automatic causal ordering with vector clocks
- Type-safe synchronization with compile-time guarantees
- Ergonomic builder pattern for setup

**Builder patterns**:
```rust
// Simple convergent CRDT
CrdtCoordinator::with_cv_state(device_id, state);

// Delta CRDT with compaction threshold
CrdtCoordinator::with_delta_threshold(device_id, 100);

// Multiple handlers composed
CrdtCoordinator::new(device_id)
    .with_cv_handler(cv_handler)
    .with_delta_handler(delta_handler);
```

### 2. Anti-Entropy Pattern

**Purpose**: Eventual consistency through periodic state reconciliation

**Location**: `crates/aura-protocol/src/handlers/sync_anti_entropy.rs`

**Use cases**:
- Repairing network partitions
- Ensuring eventual consistency
- Gossip-based state propagation

**Example**:
```rust
use aura_protocol::handlers::AntiEntropyConfig;

let config = AntiEntropyConfig {
    sync_interval: Duration::from_secs(30),
    peer_selection_strategy: PeerSelectionStrategy::Random(3),
    bloom_filter_size: 1024,
};

// Execute anti-entropy round
let peers = select_peers(&config);
for peer in peers {
    perform_anti_entropy_sync(peer, &config, effects).await?;
}
```

**Key features**:
- Configurable sync intervals
- Multiple peer selection strategies (random, round-robin, priority)
- Bloom filter optimization for large state
- Automatic reconciliation on mismatch detection

**Best practices**:
- Use random peer selection for fault tolerance
- Tune sync intervals based on network conditions
- Enable bloom filters for state > 1KB
- Monitor sync metrics for performance tuning

### 3. Storage Coordination Pattern

**Purpose**: Coordinate storage operations across namespaces, access control, and replication

**Locations**:
- `crates/aura-protocol/src/handlers/storage/storage_coordinator.rs` - Main coordinator
- `crates/aura-protocol/src/handlers/storage/access_coordinator.rs` - Access control
- `crates/aura-protocol/src/handlers/storage/replication_coordinator.rs` - Replication

**Use cases**:
- Multi-namespace storage management
- Capability-based storage access control
- Coordinated replication across devices

**Example**:
```rust
use aura_protocol::handlers::storage::StorageCoordinator;

let coordinator = StorageCoordinator::new(device_id, storage_effects);

// Store with namespace isolation
coordinator.store_in_namespace(
    "user_documents",
    "document_id",
    document_data,
    capabilities
).await?;

// Coordinate replication
coordinator.replicate_to_peers(
    "document_id",
    vec![peer1_id, peer2_id],
    replication_policy
).await?;
```

**Key features**:
- Namespace-based isolation
- Capability integration for access control
- Configurable replication strategies
- Automatic conflict resolution

### 4. Session Management Pattern

**Purpose**: Coordinate multi-party choreographic sessions

**Location**: `crates/aura-protocol/src/handlers/agent/session.rs`

**Use cases**:
- Multi-device protocol coordination
- Session lifecycle management
- Role-based session participation

**Example**:
```rust
use aura_protocol::handlers::agent::SessionManager;

let session_manager = SessionManager::new(device_id);

// Create session for threshold ceremony
let session = session_manager.create_session(
    SessionType::ThresholdCeremony,
    vec![device1, device2, device3],
    session_config
).await?;

// Join existing session
session_manager.join_session(session_id, my_role).await?;

// Execute choreographed protocol
let result = session.execute_protocol(protocol, effects).await?;

// Clean up on completion
session_manager.end_session(session_id).await?;
```

**Key features**:
- Type-safe session lifecycle (Created → Active → Completed)
- Automatic participant tracking
- Integration with choreographic effects
- Session metadata and metrics

### 5. Transport Coordination Pattern

**Purpose**: Coordinate connection lifecycle and channel management

**Location**: `crates/aura-protocol/src/handlers/transport_coordinator.rs`

**Use cases**:
- QUIC connection pooling
- SecureChannel lifecycle management
- Transport-level retries and failover

**Example**:
```rust
use aura_protocol::handlers::TransportCoordinator;

let coordinator = TransportCoordinator::new(device_id, network_effects);

// Establish secure channel
let channel = coordinator.establish_channel(
    peer_id,
    context_id,
    channel_config
).await?;

// Coordinated send with automatic retry
coordinator.send_with_retry(
    &channel,
    message,
    retry_policy
).await?;

// Automatic channel cleanup on budget/epoch changes
coordinator.cleanup_stale_channels(context_manager).await?;
```

**Key features**:
- Connection pooling and reuse
- Automatic channel lifecycle management
- Budget-aware channel teardown
- Epoch-based channel invalidation

### 6. Timeout Coordination Pattern

**Purpose**: Coordinate timeouts across distributed operations

**Location**: `crates/aura-protocol/src/handlers/timeout_coordinator.rs`

**Use cases**:
- Distributed timeout enforcement
- Deadline propagation
- Timeout-based error recovery

**Example**:
```rust
use aura_protocol::handlers::TimeoutCoordinator;

let coordinator = TimeoutCoordinator::new(time_effects);

// Execute with distributed timeout
coordinator.with_timeout(
    Duration::from_secs(30),
    async {
        perform_distributed_operation(effects).await
    }
).await?;

// Coordinate timeouts across multiple operations
coordinator.with_coordinated_timeouts(
    vec![op1, op2, op3],
    timeout_policy
).await?;
```

**Key features**:
- Distributed deadline propagation
- Coordinated cancellation across parties
- Timeout metrics and observability
- Configurable timeout policies (fail-fast, best-effort)

## Pattern Selection Guide

Use this decision tree to select the appropriate orchestration pattern:

```
Need to synchronize distributed state?
  ├─ YES → Use CRDT Coordination Pattern
  └─ NO
     └─ Need eventual consistency after partitions?
        ├─ YES → Use Anti-Entropy Pattern
        └─ NO
           └─ Need multi-party session coordination?
              ├─ YES → Use Session Management Pattern
              └─ NO
                 └─ Need coordinated storage operations?
                    ├─ YES → Use Storage Coordination Pattern
                    └─ NO
                       └─ Need connection lifecycle management?
                          ├─ YES → Use Transport Coordination Pattern
                          └─ NO
                             └─ Need distributed timeouts?
                                ├─ YES → Use Timeout Coordination Pattern
                                └─ NO → Consider composing multiple patterns
```

## Composition Patterns

Patterns can be composed for complex coordination scenarios:

### Example: Replicated Storage with Anti-Entropy

```rust
// Combine storage coordination with anti-entropy
let storage_coordinator = StorageCoordinator::new(device_id, storage);
let crdt_coordinator = CrdtCoordinator::with_cv_state(device_id, state);

// Store locally
storage_coordinator.store("key", data).await?;

// Trigger anti-entropy sync
let sync_request = crdt_coordinator.create_sync_request(peer_id)?;
storage_coordinator.replicate_to_peer(peer_id, sync_request).await?;
```

### Example: Session with Timeout Coordination

```rust
// Combine session management with timeout coordination
let session_manager = SessionManager::new(device_id);
let timeout_coordinator = TimeoutCoordinator::new(time_effects);

// Execute session protocol with timeout
timeout_coordinator.with_timeout(
    Duration::from_mins(5),
    async {
        let session = session_manager.create_session(...).await?;
        session.execute_protocol(protocol, effects).await
    }
).await?;
```

## Best Practices

### 1. Prefer Composition Over Custom Implementation

```rust
// ❌ Don't implement custom coordination from scratch
async fn my_custom_sync(peer: DeviceId) {
    // 100 lines of custom state reconciliation...
}

// ✅ Do use standard patterns
let coordinator = CrdtCoordinator::with_cv_state(device_id, state);
coordinator.sync_with_peer(peer).await?;
```

### 2. Use Builder Patterns for Configuration

```rust
// ✅ Ergonomic configuration with builders
let coordinator = CrdtCoordinator::new(device_id)
    .with_cv_handler(state_handler)
    .with_delta_threshold(100)
    .with_metrics_enabled(true);
```

### 3. Handle Errors at the Right Level

```rust
// ✅ Let patterns handle transient errors
coordinator.sync_with_retry(peer, retry_policy).await?;

// Handle application errors at caller level
match coordinator.sync(peer).await {
    Ok(result) => process_result(result),
    Err(AuraError::PermissionDenied { .. }) => handle_authz_error(),
    Err(e) => Err(e), // Propagate other errors
}
```

### 4. Monitor Pattern Metrics

```rust
// ✅ Enable metrics for production deployments
let coordinator = CrdtCoordinator::new(device_id)
    .with_metrics_enabled(true);

// Collect metrics periodically
let metrics = coordinator.metrics();
log::info!(
    "Sync stats: {} successful, {} failed, avg latency: {}ms",
    metrics.sync_count,
    metrics.error_count,
    metrics.avg_latency_ms
);
```

## Testing Patterns

All orchestration patterns support testing through mock effects:

```rust
#[tokio::test]
async fn test_crdt_coordination() {
    let fixture = TestFixture::new();
    let device_id = fixture.create_device_id();

    let coordinator = CrdtCoordinator::with_cv_state(
        device_id,
        initial_state
    );

    // Test with mock effects
    let result = coordinator.sync_with_peer(
        peer_id,
        fixture.effects()
    ).await;

    assert!(result.is_ok());
    assert_eq!(coordinator.state(), expected_state);
}
```

## Performance Considerations

### CRDT Coordination
- **Latency**: ~5-10ms per sync (local network)
- **Throughput**: 100-1000 syncs/sec depending on state size
- **Memory**: O(n) where n = state size
- **Optimization**: Use delta CRDTs for large state (> 1KB)

### Anti-Entropy
- **Latency**: ~50-200ms per round (depends on peer count)
- **Throughput**: 10-100 rounds/sec
- **Memory**: O(p) where p = peer count
- **Optimization**: Tune sync interval based on consistency requirements

### Storage Coordination
- **Latency**: ~1-5ms per operation (local storage)
- **Throughput**: 1000-10000 ops/sec
- **Memory**: O(n) where n = namespace count
- **Optimization**: Use namespace isolation to reduce contention

## Migration Guide

### From Manual Coordination to Standard Patterns

**Before** (manual coordination):
```rust
async fn sync_state(peer: DeviceId, state: State) -> Result<()> {
    let digest = compute_digest(&state);
    send_digest_to_peer(peer, digest).await?;
    let peer_digest = receive_digest(peer).await?;

    if digest != peer_digest {
        send_full_state(peer, state).await?;
    }
    Ok(())
}
```

**After** (using pattern):
```rust
async fn sync_state(peer: DeviceId, state: State) -> Result<()> {
    let coordinator = CrdtCoordinator::with_cv_state(device_id, state);
    coordinator.sync_with_peer(peer).await
}
```

**Benefits**:
- 90% less code
- Automatic error handling and retries
- Built-in metrics and observability
- Well-tested implementation

## See Also

- [System Architecture](002_system_architecture.md) - Overall architecture context
- [Coordination Systems Guide](803_coordination_systems_guide.md) - Lower-level coordination primitives
- [Advanced Choreography Guide](804_advanced_choreography_guide.md) - Choreographic patterns
- [Project Structure](999_project_structure.md) - Crate organization and layer boundaries
