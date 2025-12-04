# Distributed Maintenance Architecture

This document describes distributed maintenance in Aura. It explains snapshots, garbage collection, cache invalidation, OTA upgrades, admin replacement, epoch handling, and backup procedures. All maintenance operations align with the authority and relational context model. All maintenance operations insert facts into appropriate journals. All replicas converge through join-semilattice rules.

## 1. Maintenance Facts

Maintenance uses facts stored in an authority journal. Facts represent monotone knowledge. Maintenance logic evaluates local predicates over accumulated facts. These predicates implement constraints such as GC eligibility or upgrade readiness.

```rust
pub enum MaintenanceFact {
    SnapshotProposed {
        proposal_id: Uuid,
        target_epoch: u64,
        digest: Hash32,
    },
    SnapshotCompleted {
        proposal_id: Uuid,
        snapshot: SnapshotV1,
        threshold_signature: Vec<u8>,
    },
    CacheInvalidated {
        keys: Vec<CacheKey>,
        epoch_floor: u64,
    },
    UpgradeActivated {
        package_id: Uuid,
        to_version: ProtocolVersion,
        activation_fence: u64,
    },
}
```

This fact model defines snapshot, cache, and upgrade events. Each fact is immutable and merges by set union. Devices reduce maintenance facts with deterministic rules.

## 2. Snapshots and Garbage Collection

Snapshots bound storage size. A snapshot proposal announces a target epoch and a digest of the journal prefix. Devices verify the digest. If valid, they contribute signatures. A threshold signature completes the snapshot.

Snapshot completion inserts a `SnapshotCompleted` fact. Devices prune facts whose epochs fall below the snapshot epoch. Devices prune blobs whose tombstones precede the snapshot. This pruning does not affect correctness because the snapshot represents a complete prefix.

```rust
pub struct SnapshotV1 {
    pub epoch: u64,
    pub digest: Hash32,
    pub blob_cid: Cid,
}
```

This structure identifies a snapshot blob. Devices fetch the blob when restoring state. Devices hydrate journal state and replay the tail of post-snapshot facts.

## 3. Cache Invalidation

State mutations publish `CacheInvalidated` facts. A cache invalidation fact contains cache keys and an epoch floor. Devices maintain local maps from keys to epoch floors. A cache entry is valid only when the current epoch exceeds its floor.

Cache invalidation is local. No CRDT cache is replicated. Devices compute validity using meet predicates on epoch constraints.

```rust
pub struct CacheKey(pub String);
```

This structure identifies a cached entry. Devices invalidate cached data when they observe newer invalidation facts.

## 4. OTA Upgrades

Upgrades appear as monotone activation facts. Devices advertise supported versions and upgrade policies inside device metadata. Operators publish upgrade metadata containing package identifiers and version information.

Devices verify upgrade artifacts using their hashes. Devices mark readiness when policies allow. A hard fork upgrade uses an activation fence. An activation fact contains the target version and the fence epoch. Devices reject sessions when their local epoch does not satisfy the fence.

```rust
pub struct UpgradeMetadata {
    pub package_id: Uuid,
    pub version: ProtocolVersion,
    pub artifact_hash: Hash32,
}
```

This structure identifies an upgrade package. Devices must install the binary before publishing readiness. OTA upgrades rely on device metadata and maintenance facts.

## 5. Admin Replacement

Admin replacement uses a maintenance fact. The fact records the old admin, new admin, and activation epoch. Devices use this fact to ignore operations from retired administrators.

```rust
pub struct AdminReplacement {
    pub old_admin: AuthorityId,
    pub new_admin: AuthorityId,
    pub activation_epoch: u64,
}
```

This structure defines an admin replacement. Devices enforce this rule locally. The replacement fact is monotone and resolves disputes using journal evidence.

## 6. Epoch Handling

Maintenance logic uses identity epochs for consistency. A maintenance session uses a tuple containing the identity epoch and snapshot epoch. A session aborts if the identity epoch advances. Devices retry under the new epoch.

Snapshot completion sets the snapshot epoch equal to the identity epoch. Garbage collection rules use the snapshot epoch to prune data safely. Upgrade fences use the same epoch model to enforce activation.

```rust
pub struct MaintenanceEpoch {
    pub identity_epoch: u64,
    pub snapshot_epoch: u64,
}
```

This structure captures epoch state for maintenance workflows. Devices use this structure for guard checks.

## 7. Backup and Restore

Backup uses the latest snapshot and recent journal facts. Devices export an encrypted archive containing the snapshot blob and journal tail. Restore verifies the snapshot signature, hydrates state, and replays the journal tail.

Backups use existing storage and verification effects. No separate protocol exists. Backup correctness follows from snapshot correctness.

## 8. Automatic Synchronization

Automatic synchronization implements periodic journal replication between devices. The synchronization service coordinates peer discovery, session management, and fact exchange. All synchronization uses the journal primitives described in [Journal](102_journal.md).

### 8.1 Peer Discovery and Selection

Devices discover sync peers through the rendezvous system described in [Rendezvous](110_rendezvous.md). The peer manager maintains metadata for each discovered peer. This metadata includes connection state, trust level, sync success rate, and active session count.

```rust
pub struct PeerMetadata {
    pub device_id: DeviceId,
    pub status: PeerStatus,
    pub trust_level: u8,
    pub successful_syncs: u64,
    pub failed_syncs: u64,
    pub active_sessions: usize,
}
```

This structure tracks peer state for selection decisions. The peer manager calculates a score for each peer using weighted factors. Trust level contributes 50 percent. Success rate contributes 30 percent. Load factor contributes 20 percent. Higher scores indicate better candidates for synchronization.

Devices select peers when their score exceeds a threshold. Devices limit concurrent sessions per peer. This prevents resource exhaustion. Devices skip peers that have reached their session limit.

### 8.2 Session Management

The session manager tracks active synchronization sessions. Each session has a unique identifier and references a peer device. Sessions enforce rate limits and concurrency bounds.

```rust
pub struct SessionManager<T> {
    pub active_sessions: HashMap<DeviceId, SessionState<T>>,
    pub max_concurrent: usize,
}
```

This structure maintains session state. Devices close sessions after fact exchange completes. Devices abort sessions when the identity epoch advances. Session cleanup releases resources for new synchronization rounds.

### 8.3 Rate Limiting and Metrics

The synchronization service enforces rate limits per peer and globally. Rate limiting prevents network saturation. Metrics track sync latency, throughput, and error rates.

Devices record metrics for each sync operation. These metrics include fact count, byte count, and duration. Devices aggregate metrics to monitor service health. Degraded peers receive lower priority in future rounds.

### 8.4 Integration with Journal Effects

Automatic synchronization uses `JournalEffects` to read and write facts. The service queries local journals for recent facts. The service sends these facts to peers. Peers merge incoming facts using join-semilattice rules.

All fact validation rules apply during automatic sync. Devices reject invalid facts. Devices do not rollback valid facts already merged. This maintains journal monotonicity.

## 9. Evolution

Maintenance evolves in phases. Phase one includes snapshots, GC, cache invalidation, and OTA activation. Phase two includes replicated cache CRDTs, staged OTA rollouts, and automatic synchronization. Phase three includes proxy re-encryption for historical blobs and automated snapshot triggers.

Future phases build on the same journal schema. Maintenance semantics remain compatible with older releases.

## 10. Summary

Distributed maintenance uses journal facts to coordinate snapshots, cache invalidation, upgrades, and admin replacement. All operations use join-semilattice semantics. All reductions are deterministic. Devices prune storage only after observing snapshot completion. Epoch rules provide safety during upgrades and recovery. The system remains consistent across offline and online operation.
