# Maintenance and OTA Guide

This guide covers the Maintenance and OTA (Over-the-Air) update system in Aura. The system provides coordinated maintenance operations including snapshots, cache invalidation, and distributed upgrades using threshold approval.

## Core Maintenance Philosophy

The maintenance system is built on three key principles:

1. **Coordinated Operations** - All maintenance actions require threshold approval from M-of-N authorities
2. **Epoch Fencing** - Hard fork upgrades are gated by identity epochs for safe coordination
3. **Journal-Based Facts** - All maintenance events are replicated through the journal CRDT

The system supports snapshots for garbage collection, cache management, and both soft and hard fork upgrades.

## Maintenance Events

The maintenance service publishes events to the journal as facts. These events are replicated across all replicas and interpreted deterministically.

### Event Types

The system defines five event types:

**SnapshotProposed** marks the beginning of a snapshot operation. It contains the proposal identifier, proposer authority, target epoch, and state digest of the candidate snapshot.

**SnapshotCompleted** records a successful snapshot. It includes the accepted proposal identifier, finalized snapshot payload, participating authorities, and threshold signature attesting to the snapshot.

**CacheInvalidated** signals cache invalidation. It specifies which cache keys must be refreshed and the earliest identity epoch the cache entry remains valid for.

**UpgradeActivated** announces an activated upgrade. It contains the package identifier, target version, and identity epoch fence where the upgrade becomes mandatory.

**AdminReplacement** announces an administrator change. This allows users to fork away from a malicious admin by tracking previous and new administrators with activation epoch.

## Snapshot Protocol

The snapshot protocol coordinates garbage collection with threshold approval. It implements writer fencing to ensure consistent snapshot capture across all devices.

### Snapshot Workflow

The snapshot process follows five steps:

1. Propose snapshot at target epoch with state digest
2. Activate writer fence to block concurrent writes
3. Capture state and verify digest
4. Collect M-of-N threshold approvals
5. Commit snapshot and clean obsolete facts

### Basic Snapshot Operation

```rust
use aura_sync::services::{MaintenanceService, MaintenanceServiceConfig};
use aura_core::{Epoch, Hash32};

async fn propose_snapshot(
    service: &MaintenanceService,
    authority_id: aura_core::AuthorityId,
    target_epoch: Epoch,
    state_digest: Hash32,
) -> Result<(), Box<dyn std::error::Error>> {
    // Propose snapshot at target epoch
    service
        .propose_snapshot(authority_id, target_epoch, state_digest)
        .await?;
    
    // Writer fence is now active - all concurrent writes blocked
    // Collect approvals from M-of-N authorities
    
    // Once threshold reached, commit
    service.commit_snapshot().await?;
    
    Ok(())
}
```

Snapshots provide deterministic checkpoints of authority state at specific epochs. This enables garbage collection of obsolete facts while maintaining verifiable state recovery.

### Snapshot Proposal

```rust
use aura_sync::services::MaintenanceService;
use aura_core::{AuthorityId, Epoch, Hash32};

async fn snapshot_workflow(
    service: &MaintenanceService,
    authority_id: AuthorityId,
) -> Result<(), Box<dyn std::error::Error>> {
    // Determine target epoch and compute state digest
    let target_epoch = 100;
    let current_state = service.get_current_state().await?;
    let state_digest = current_state.compute_digest();
    
    // Propose snapshot
    service
        .propose_snapshot(authority_id, Epoch::new(target_epoch), state_digest)
        .await?;
    
    // Wait for other authorities to activate fence
    // and collect approvals
    
    Ok(())
}
```

Proposals include the proposer authority identifier, unique proposal ID, target epoch, and canonical state digest. All participants must agree on the digest before committing.

### Writer Fence

The writer fence blocks all writes during snapshot capture. This prevents concurrent modifications that could invalidate the snapshot digest.

```rust
use aura_sync::services::MaintenanceService;

async fn capture_with_fence(
    service: &MaintenanceService,
) -> Result<(), Box<dyn std::error::Error>> {
    // Writer fence is automatically activated by snapshot proposal
    // All write operations will be blocked or queued
    
    // Capture state atomically
    let snapshot = service.capture_snapshot().await?;
    
    // Once snapshot is committed, fence is released
    service.commit_snapshot().await?;
    
    Ok(())
}
```

Fence enforcement is implicit in snapshot proposal. The protocol guarantees no conflicting writes occur during snapshot capture.

### Approval Collection

```rust
use aura_sync::services::MaintenanceService;

async fn collect_approvals(
    service: &MaintenanceService,
    threshold: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get pending snapshot proposals
    let proposals = service.pending_snapshots().await?;
    
    for proposal in proposals {
        // Verify state digest
        let current_state = service.get_current_state().await?;
        let digest = current_state.compute_digest();
        
        if digest == proposal.state_digest {
            // Approve the snapshot
            service.approve_snapshot(&proposal.proposal_id).await?;
        }
    }
    
    Ok(())
}
```

Each device verifies the state digest independently and approves if correct. The system collects approvals until threshold is reached.

### Snapshot Commitment

```rust
use aura_sync::services::MaintenanceService;

async fn finalize_snapshot(
    service: &MaintenanceService,
) -> Result<(), Box<dyn std::error::Error>> {
    // Commit snapshot once threshold reached
    service.commit_snapshot().await?;
    
    // This records SnapshotCompleted fact to journal
    // All devices deterministically reduce this fact
    // Obsolete facts before this epoch can be garbage collected
    
    Ok(())
}
```

Commitment publishes `SnapshotCompleted` fact to the journal. All devices deterministically reduce this fact to the same relational state. Facts older than the snapshot epoch can then be safely discarded.

## OTA (Over-the-Air) Upgrade Protocol

The OTA protocol coordinates distributed upgrades with support for both soft and hard forks. Soft forks are backward compatible and require only majority device readiness. Hard forks require coordinated activation at a specific identity epoch.

### Upgrade Types

**Soft Fork** upgrades are backward compatible. Old and new code can interoperate. Soft forks activate immediately once threshold devices are ready.

**Hard Fork** upgrades are incompatible. All devices must upgrade by the activation epoch. Hard forks are gated by identity epoch fences to ensure coordinated activation.

### Basic Upgrade Operation

```rust
use aura_sync::services::{MaintenanceService, MaintenanceServiceConfig};
use aura_sync::protocols::UpgradeKind;
use aura_core::SemanticVersion;
use uuid::Uuid;

async fn propose_soft_fork(
    service: &MaintenanceService,
    package_id: Uuid,
    version: SemanticVersion,
) -> Result<(), Box<dyn std::error::Error>> {
    // Propose soft fork upgrade
    let proposal = service.propose_upgrade(
        package_id,
        version.clone(),
        UpgradeKind::SoftFork,
        None,  // No activation fence for soft fork
    ).await?;
    
    // Collect readiness declarations from devices
    // Soft fork activates when threshold devices are ready
    
    Ok(())
}
```

Soft forks are simpler than hard forks. They activate immediately when enough devices report readiness.

### Soft Fork Workflow

```rust
use aura_sync::services::MaintenanceService;
use aura_sync::protocols::{ReadinessStatus, UpgradeKind};

async fn soft_fork_workflow(
    service: &MaintenanceService,
) -> Result<(), Box<dyn std::error::Error>> {
    // Receive upgrade proposal (soft fork)
    let proposal = service.pending_upgrades().await?
        .into_iter()
        .find(|p| p.kind == UpgradeKind::SoftFork)
        .unwrap();
    
    // Check readiness locally
    if service.is_ready_for_upgrade(&proposal).await? {
        // Declare readiness
        service.declare_readiness(
            &proposal.proposal_id,
            ReadinessStatus::Ready,
        ).await?;
    }
    
    // Once threshold devices are ready, upgrade activates
    
    Ok(())
}
```

Each device independently evaluates readiness and declares status. The protocol activates the upgrade once M-of-N devices are ready.

### Hard Fork Workflow

```rust
use aura_sync::services::{MaintenanceService, MaintenanceServiceConfig};
use aura_sync::protocols::UpgradeKind;
use aura_core::{SemanticVersion, Epoch, AccountId};
use uuid::Uuid;

async fn propose_hard_fork(
    service: &MaintenanceService,
    package_id: Uuid,
    version: SemanticVersion,
    activation_epoch: Epoch,
) -> Result<(), Box<dyn std::error::Error>> {
    // Propose hard fork with epoch fence
    let proposal = service.propose_upgrade(
        package_id,
        version,
        UpgradeKind::HardFork,
        Some(activation_epoch),
    ).await?;
    
    // All devices must upgrade by activation epoch
    // Protocol enforces fence at epoch boundary
    
    Ok(())
}
```

Hard forks require an identity epoch fence. The upgrade becomes mandatory at the specified epoch. This ensures all devices coordinate activation at a specific point in time.

### Activation Fence

The activation fence gates hard fork upgrades. It specifies the account and identity epoch where the upgrade becomes mandatory.

```rust
use aura_sync::services::MaintenanceService;
use aura_sync::services::IdentityEpochFence;
use aura_core::{Epoch, AccountId};

async fn enforce_hard_fork(
    service: &MaintenanceService,
    account_id: AccountId,
    upgrade_epoch: Epoch,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create epoch fence for upgrade
    let fence = IdentityEpochFence::new(account_id, upgrade_epoch);
    
    // All operations after upgrade_epoch must use new protocol version
    // Old version is no longer accepted
    
    // If device hasn't upgraded by epoch, it cannot participate
    if service.current_epoch() >= upgrade_epoch {
        if !service.has_upgraded().await? {
            // Cannot continue - must upgrade
            service.enforce_upgrade().await?;
        }
    }
    
    Ok(())
}
```

Epoch fences are enforced at epoch boundaries. Devices that have not upgraded by the fence epoch are blocked from participating.

### Readiness Declarations

```rust
use aura_sync::services::MaintenanceService;
use aura_sync::protocols::ReadinessStatus;

async fn declare_upgrade_readiness(
    service: &MaintenanceService,
    proposal_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if upgrade artifacts are available
    let artifacts = service.fetch_upgrade_artifacts(proposal_id).await?;
    
    // Verify artifact hash
    if !service.verify_artifact(&artifacts).await? {
        service.declare_readiness(
            proposal_id,
            ReadinessStatus::Rejected {
                reason: "Invalid artifact hash".to_string(),
            },
        ).await?;
        return Ok(());
    }
    
    // Check if local system can support upgrade
    if service.supports_upgrade(&artifacts).await? {
        service.declare_readiness(
            proposal_id,
            ReadinessStatus::Ready,
        ).await?;
    } else {
        service.declare_readiness(
            proposal_id,
            ReadinessStatus::NotReady {
                reason: "Insufficient resources".to_string(),
            },
        ).await?;
    }
    
    Ok(())
}
```

Readiness declarations are self-contained. Each device verifies artifacts and evaluates its own readiness. The protocol does not require coordination for readiness checks.

## Cache Management

The maintenance system coordinates cache invalidation across all devices. Cache invalidation facts specify which keys must be refreshed and the epoch where the cache entry is no longer valid.

### Cache Invalidation

```rust
use aura_sync::services::MaintenanceService;
use aura_core::Epoch;

async fn invalidate_cache(
    service: &MaintenanceService,
    keys: Vec<String>,
    epoch_floor: Epoch,
) -> Result<(), Box<dyn std::error::Error>> {
    // Publish cache invalidation fact
    service.invalidate_cache_keys(keys, epoch_floor).await?;
    
    // All devices receive fact through journal
    // Each device deterministically invalidates matching keys
    
    Ok(())
}
```

Cache invalidation facts are replicated through the journal. All devices apply the invalidation deterministically based on epoch and key matching.

### Cache Query Patterns

```rust
use aura_sync::services::MaintenanceService;
use aura_core::Epoch;

async fn query_cache(
    service: &MaintenanceService,
    key: &str,
    epoch: Epoch,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    // Check if cache entry is valid at epoch
    if service.is_cache_valid(key, epoch).await? {
        service.get_cached(key).await
    } else {
        // Cache invalidated - fetch fresh
        let value = service.fetch_fresh(key).await?;
        service.cache_set(key, value.clone(), epoch).await?;
        Ok(Some(value))
    }
}
```

Cache validity is epoch-based. Entries are valid up to their invalidation epoch. After that, fresh data must be fetched and cached at the new epoch.

## Configuration and Best Practices

### Service Configuration

```rust
use aura_sync::services::{MaintenanceService, MaintenanceServiceConfig};
use aura_sync::protocols::{SnapshotConfig, OTAConfig};
use std::time::Duration;

fn create_maintenance_service() -> Result<MaintenanceService, Box<dyn std::error::Error>> {
    let config = MaintenanceServiceConfig {
        snapshot: SnapshotConfig {
            proposal_timeout: Duration::from_secs(300),
            approval_timeout: Duration::from_secs(600),
            max_proposals: 10,
        },
        ota: OTAConfig {
            readiness_timeout: Duration::from_secs(3600),
            max_pending: 5,
            soft_fork_auto_activate: true,
        },
        cache: Default::default(),
    };
    
    Ok(MaintenanceService::new(config)?)
}
```

Configuration controls timeouts, limits, and behavior. Snapshot timeouts should be shorter than OTA timeouts since snapshots are more frequent.

### Snapshot Best Practices

Keep snapshots frequent but not excessive. Snapshot every 100-500 epochs depending on journal size. Too frequent snapshots create overhead. Too infrequent snapshots reduce garbage collection effectiveness.

Always verify state digest before approving. Use canonical serialization for digest computation. Record all snapshots in the journal for audit trail.

### Upgrade Best Practices

Plan upgrades carefully. Soft forks can be deployed flexibly. Hard forks require scheduling and announcement.

Always test upgrades in simulation before deployment. Use epoch fences with sufficient advance notice (at least 7 days for production).

Include rollback procedures for hard forks. Document migration paths for state format changes.

### Cache Best Practices

Invalidate cache conservatively. Over-invalidation reduces performance. Under-invalidation risks stale data.

Use epoch floors to scope invalidation. Invalidate only keys that actually changed at that epoch.

Monitor cache hit rates. Low hit rates indicate invalidation is too aggressive.

## Monitoring and Debugging

### Snapshot Status

```rust
use aura_sync::services::MaintenanceService;

async fn monitor_snapshots(
    service: &MaintenanceService,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = service.snapshot_status().await?;
    
    println!("Last snapshot: epoch {}", status.last_snapshot_epoch);
    println!("Pending proposals: {}", status.pending_proposals);
    println!("Writer fence active: {}", status.fence_active);
    
    Ok(())
}
```

Check snapshot status regularly to ensure the snapshot cycle is healthy. Long intervals between snapshots may indicate approval delays.

### Upgrade Status

```rust
use aura_sync::services::MaintenanceService;

async fn monitor_upgrades(
    service: &MaintenanceService,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = service.upgrade_status().await?;
    
    for upgrade in status.active_upgrades {
        println!("Upgrade: version {}", upgrade.version);
        println!("  Ready devices: {}/{}", upgrade.ready_count, upgrade.total_devices);
        println!("  Threshold: {}", upgrade.threshold);
    }
    
    Ok(())
}
```

Monitor upgrade progress to catch devices that are not ready. Missing devices may need manual intervention.

## Integration with Choreography

The maintenance system integrates with the choreography runtime for threshold approval ceremonies. Snapshot and upgrade proposals are published to the journal where choreography protocols can coordinate approval.

The maintenance service publishes events as facts. Choreography protocols subscribe to these facts and coordinate the necessary approvals through their own message flows.

## Summary

The Maintenance and OTA system provides coordinated maintenance operations with threshold approval and epoch fencing. Snapshots enable garbage collection with writer fencing. Soft forks activate flexibly while hard forks coordinate activation at specific epochs. Cache invalidation is replicated through the journal for consistency.

Use snapshots regularly for garbage collection. Plan upgrades carefully with sufficient notice for hard forks. Test all upgrades in simulation. Monitor snapshot and upgrade cycles to ensure system health.

## Implementation References

- **Maintenance Service**: `aura-sync/src/services/maintenance.rs`
- **Snapshot Protocol**: `aura-sync/src/protocols/snapshots.rs`
- **OTA Protocol**: `aura-sync/src/protocols/ota.rs`
- **Cache Management**: `aura-sync/src/infrastructure/cache_manager.rs`
- **Integration Examples**: `aura-agent/src/handlers/maintenance.rs`
