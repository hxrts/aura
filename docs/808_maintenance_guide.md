# Distributed Maintenance Guide

This guide covers practical workflows for the Maintenance and OTA (Over-the-Air) update system in Aura. Use it for snapshots, cache invalidation, and distributed upgrades.

For the maintenance architecture specification, see [Distributed Maintenance Architecture](115_maintenance.md).

## Maintenance Philosophy

The maintenance system is built on three key principles:

1. **Coordinated Operations** - Threshold approval is used where the chosen maintenance scope actually has a quorum or authority set that can approve
2. **Epoch Fencing** - Hard fork upgrades may use identity epochs for safe coordination, but only inside scopes that own that fence
3. **Journal-Based Facts** - All maintenance events are replicated through the journal CRDT

The system supports snapshots for garbage collection, cache management, and both soft and hard fork upgrades.

## Maintenance Events

The maintenance service publishes events to the journal as facts. These events are replicated across all replicas and interpreted deterministically.

### Event Types

The system defines several event families:

**SnapshotProposed** marks the beginning of a snapshot operation. It contains the proposal identifier, proposer authority, target epoch, and state digest of the candidate snapshot.

**SnapshotCompleted** records a successful snapshot. It includes the accepted proposal identifier, finalized snapshot payload, participating authorities, and threshold signature attesting to the snapshot.

**CacheInvalidated** signals cache invalidation. It specifies which cache keys must be refreshed and the earliest identity epoch the cache entry remains valid for.

**ReleaseDistribution** facts announce release declarations, build certificates, and artifact availability.

**ReleasePolicy** facts announce discovery, sharing, and activation policy publications.

**UpgradeExecution** facts announce scoped staging, residency changes, transition changes, cutover results, partition outcomes, and rollback execution.

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

Aura OTA is split into two layers:

- release distribution: manifests, artifacts, and build certificates spread through Aura storage and anti-entropy
- scoped activation: each device, authority, context, or managed quorum decides when a staged release may activate

There is no network-wide authoritative cutover phase for the whole Aura network. Hard cutover is valid only inside a scope that actually has agreement or a legitimate fence.

### Upgrade Types

**Soft Fork** upgrades are compatibility-preserving. Old and new code can interoperate while one scope is in `ReleaseResidency::Coexisting`.

**Hard Fork** upgrades are scope-bound incompatibility transitions. They reject incompatible new sessions after local cutover and require explicit in-flight handling for old sessions. A hard fork may use threshold approval or epoch fencing, but only if the chosen scope actually owns that mechanism.

### Basic Upgrade Operation

```rust
use aura_maintenance::AuraReleaseActivationPolicy;
use aura_sync::services::{ActivationCandidate, OtaPolicyEvaluator};

async fn evaluate_activation(
    policy: &AuraReleaseActivationPolicy,
    candidate: &ActivationCandidate<'_>,
) {
    // Activation is local or scope-bound.
    // Discovery and sharing do not imply activation.
    // The evaluator checks trust, compatibility, staged artifacts,
    // health gates, threshold approval, and scope-owned fences.
    let _decision = OtaPolicyEvaluator::new().evaluate_activation(policy, candidate);
}
```

If policy enables it, `suggested_activation_time_unix_ms` acts only as a local "not before" hint against the local clock. It is advisory metadata, not a global synchronization fence.

### Soft Fork Workflow

```rust
use aura_agent::runtime::services::OtaManager;
use aura_maintenance::{AuraActivationScope, AuraCompatibilityClass};
use aura_sync::services::InFlightIncompatibilityAction;

async fn soft_fork_workflow(
    manager: &OtaManager,
    scope: AuraActivationScope,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = manager
        .begin_scoped_cutover(&scope, InFlightIncompatibilityAction::Drain, false)
        .await?;
    assert!(!plan.partition_required);
    Ok(())
}
```

Soft forks do not require a globally shared instant. Each scope moves from legacy-only residency to coexistence and then to target-only residency based on its own evidence and policy.

### Hard Fork Workflow

```rust
use aura_agent::runtime::services::OtaManager;
use aura_maintenance::AuraActivationScope;
use aura_sync::services::InFlightIncompatibilityAction;

async fn execute_managed_quorum_cutover(
    manager: &OtaManager,
    scope: AuraActivationScope,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = manager
        .begin_scoped_cutover(&scope, InFlightIncompatibilityAction::Delegate, true)
        .await?;
    assert!(plan.partition_required || plan.in_flight == InFlightIncompatibilityAction::Delegate);
    Ok(())
}
```

For hard forks, the operator must define:

- the activation scope
- the compatibility class
- how incompatible in-flight sessions are handled: drain, abort, or delegate
- whether threshold approval or an epoch fence is actually available in that scope

If post-cutover checks fail, rollback is explicit and deterministic.

### Managed Quorum Approval Runbook

Use managed quorum cutover only when the scope has an explicit participant set. Record approval from every participant in the quorum before starting cutover. Reject approval from authorities that are not members of that scope.

If one participant has not approved, keep the scope waiting for cutover evidence. Do not begin launcher activation for that scope. Resolve membership or policy disagreement before retrying.

### Failed Rollout Runbook

Check the failure classification before acting. `AuraUpgradeFailureClass::HealthGateFailed` means the new release started and failed local verification. `AuraUpgradeFailureClass::LauncherActivationFailed` means the launcher handoff failed before healthy activation.

If policy uses `AuraRollbackPreference::Automatic`, allow the queued rollback to execute and confirm the scope returns to legacy-only residency with an idle transition state. If policy uses `AuraRollbackPreference::ManualApproval`, keep the scope failed and require operator approval before rollback.

### Revoked Release Runbook

Treat a revoked staged release differently from a revoked active release. If the target release is only staged, cancel the staged scope and remove it from activation consideration. Do not proceed to cutover for that scope.

If the revoked release is already active, follow the configured rollback preference. Automatic rollback should queue a rollback to the prior staged release. Manual rollback should leave the scope failed until an operator approves the revert path.

### Partition Response Runbook

If `SessionCompatibilityPlan.partition_required` is true, assume incompatible peers may separate cleanly rather than interoperate. Stop admitting incompatible new sessions in that scope. Drain, abort, or delegate in-flight sessions according to the recorded incompatibility action.

Record partition observations with the associated failure classification and scope. This keeps rollback and peer-partition handling auditable.

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

Configuration controls timeouts, limits, and behavior. Snapshot timeouts should be shorter than OTA staging and activation timeouts since snapshots are more frequent. OTA policies should separately configure discovery, sharing, and activation rather than bundling them into one setting.

### Snapshot Best Practices

Keep snapshots frequent but not excessive. Snapshot every 100-500 epochs depending on journal size. Too frequent snapshots create overhead. Too infrequent snapshots reduce garbage collection effectiveness.

Always verify state digest before approving. Use canonical serialization for digest computation. Record all snapshots in the journal for audit trail.

### Upgrade Best Practices

Plan upgrades carefully. Soft forks can be deployed flexibly inside one scope. Hard forks require a clear activation scope, an explicit compatibility class, and an operator decision about whether in-flight incompatible sessions drain, abort, or delegate.

Always test upgrades in simulation before deployment. Use threshold approval or epoch fences only in scopes that actually own those mechanisms. Treat `suggested_activation_time_unix_ms` as advisory rollout metadata, not as a coordination primitive.

Include rollback procedures for hard forks. Document migration paths for state format changes and keep launcher activation/rollback steps separate from the running runtime.

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

The Maintenance and OTA system provides coordinated maintenance operations with threshold approval and epoch fencing where those mechanisms actually exist. Snapshots enable garbage collection with writer fencing. OTA release distribution is eventual, while activation is local or scope-bound. Cache invalidation is replicated through the journal for consistency.

Use snapshots regularly for garbage collection. Plan upgrades carefully with sufficient notice for hard forks. Test all upgrades in simulation. Monitor snapshot and upgrade cycles to ensure system health.

## Implementation References

- **Maintenance Service**: `aura-sync/src/services/maintenance.rs`
- **Snapshot Protocol**: `aura-sync/src/protocols/snapshots.rs`
- **OTA Protocol**: `aura-sync/src/protocols/ota.rs`
- **Cache Management**: `aura-sync/src/infrastructure/cache_manager.rs`
- **Integration Examples**: `aura-agent/src/handlers/maintenance.rs`
