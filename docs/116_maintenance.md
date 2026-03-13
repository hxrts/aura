# Distributed Maintenance Architecture

This document describes distributed maintenance in Aura. It explains snapshots, garbage collection, cache invalidation, OTA upgrades, admin replacement, epoch handling, and backup procedures. All maintenance operations align with the authority and relational context model. All maintenance operations insert facts into appropriate journals. All replicas converge through join-semilattice rules.

## 1. Maintenance Facts

Maintenance uses facts stored in an authority journal. Facts represent monotone knowledge. Maintenance logic evaluates local predicates over accumulated facts. These predicates implement constraints such as GC eligibility or upgrade readiness. The authoritative schema lives in `crates/aura-maintenance/src/facts.rs`.

```rust
pub enum MaintenanceFact {
    SnapshotProposed(SnapshotProposed),
    SnapshotCompleted(SnapshotCompleted),
    CacheInvalidated(CacheInvalidated),
    AdminReplacement(AdminReplacement),
    ReleaseDistribution(ReleaseDistributionFact),
    ReleasePolicy(ReleasePolicyFact),
    UpgradeExecution(UpgradeExecutionFact),
}
```

This fact model defines snapshot, cache, release-distribution, policy-publication, and scoped upgrade-execution events. Each fact is immutable and merges by set union. Devices reduce maintenance facts with deterministic rules.

## 2. Snapshots and Garbage Collection

Snapshots bound storage size. A snapshot proposal announces a target epoch and a digest of the journal prefix. Devices verify the digest. If valid, they contribute signatures. A threshold signature completes the snapshot.

Snapshot completion inserts a `SnapshotCompleted` fact. Devices prune facts whose epochs fall below the snapshot epoch. Devices prune blobs whose retractions precede the snapshot. This pruning does not affect correctness because the snapshot represents a complete prefix.

DKG transcript blobs follow the same garbage collection fence: once a snapshot is finalized, transcripts with epochs older than the snapshot retention window may be deleted. This keeps long-lived key ceremonies from accumulating unbounded storage while preserving the ability to replay from the latest snapshot.

```rust
pub struct Snapshot {
    pub epoch: Epoch,
    pub commitment: TreeHash32,
    pub roster: Vec<LeafId>,
    pub policies: BTreeMap<NodeIndex, Policy>,
    pub state_cid: Option<TreeHash32>,
    pub timestamp: u64,
    pub version: u8,
}
```

This structure defines the snapshot type from `aura_core::tree`. Devices fetch the blob when restoring state. Devices hydrate journal state and replay the tail of post-snapshot facts.

## 3. Cache Invalidation

State mutations publish `CacheInvalidated` facts. A cache invalidation fact contains cache keys and an epoch floor. Devices maintain local maps from keys to epoch floors. A cache entry is valid only when the current epoch exceeds its floor.

Cache invalidation is local. No CRDT cache is replicated. Devices compute validity using meet predicates on epoch constraints.

```rust
pub struct CacheKey(pub String);
```

This structure identifies a cached entry. Devices invalidate cached data when they observe newer invalidation facts.

## 4. OTA Upgrades

OTA in Aura separates two concerns:

- global and eventual release distribution
- local or scope-bound staging, activation, cutover, and rollback

Aura does not model "the whole network is now in cutover" as a valid primitive. Release propagation is multi-directional and eventual. Hard cutover is meaningful only inside a scope that actually has agreement or a legitimate fence.

### 4.1 Release Identity and Provenance

```rust
pub struct AuraReleaseProvenance {
    pub source_repo_url: String,
    pub source_bundle_hash: Hash32,
    pub build_recipe_hash: Hash32,
    pub output_hash: Hash32,
    pub nix_flake_hash: Hash32,
    pub nix_flake_lock_hash: Hash32,
}

pub struct AuraReleaseManifest {
    pub series_id: AuraReleaseSeriesId,
    pub release_id: AuraReleaseId,
    pub version: SemanticVersion,
    pub provenance: AuraReleaseProvenance,
    pub artifacts: Vec<AuraArtifactDescriptor>,
    pub compatibility: AuraCompatibilityManifest,
    pub suggested_activation_time_unix_ms: Option<u64>,
}
```

`AuraReleaseId` is derived from the release series and the full provenance. `source_repo_url` participates in that derivation, so the declared upstream repository location is part of canonical release identity. Builder authorities may publish deterministic build certificates over the same provenance. TEE attestation is optional hardening, not the source of release identity.

### 4.2 Policy Surfaces

OTA policy is not one switch. Aura distinguishes:

- discovery policy: what release authorities, builders, and contexts a device is willing to learn from
- sharing policy: what manifests, artifacts, certificates, or recommendations it is willing to forward or pin
- activation policy: what trust, compatibility, health, approval, and fence conditions must hold before local activation

Discovering a release does not imply forwarding it. Forwarding it does not imply activating it.

### 4.3 Activation Scopes and State

Activation is modeled per scope, not globally.

```rust
pub enum AuraActivationScope {
    DeviceLocal { device_id: DeviceId },
    AuthorityLocal { authority_id: AuthorityId },
    RelationalContext { context_id: ContextId },
    ManagedQuorum {
        context_id: ContextId,
        participants: BTreeSet<AuthorityId>,
    },
}

pub enum ReleaseResidency {
    LegacyOnly,
    Coexisting,
    TargetOnly,
}

pub enum TransitionState {
    Idle,
    AwaitingCutover,
    CuttingOver,
    RollingBack,
}
```

`ReleaseResidency` describes which release set may currently run in the scope. `TransitionState` describes whether the scope is stable, waiting on evidence, actively switching, or rolling back.

### 4.4 Cutover and Rollback

Scoped activation uses journal facts plus local policy evaluation. A scope may move toward cutover only when the relevant evidence is present:

- manifest and certificate verification
- compatibility classification
- staged artifacts
- local trust policy satisfaction
- optional local-policy respect for `suggested_activation_time_unix_ms`
- threshold approval, if that scope actually supports threshold approval
- epoch fence, if that scope actually owns the relevant fence
- health gate checks

Hard-fork behavior is explicit. After local cutover, incompatible new sessions are rejected. In-flight incompatible sessions must drain, abort, or delegate according to policy. If post-cutover validation fails, rollback is deterministic and recorded in `UpgradeExecutionFact`.

Managed quorum cutover requires explicit approval from the participant set bound into `AuraActivationScope::ManagedQuorum`. Staged revoked releases are canceled before cutover. Active revoked releases follow the local rollback preference. Automatic rollback queues the revert path immediately. Manual rollback leaves the scope failed until an operator approves rollback.

### 4.5 Updater / Launcher Boundary

Aura does not rely on in-place self-replacement of the running runtime. Layer 6 owns an updater/launcher control plane that:

- stages manifests, artifacts, and certificates
- emits explicit activate/rollback commands
- records scoped upgrade state
- restores the previous release deterministically when rollback is required

## 5. Admin Replacement

Admin replacement uses a maintenance fact. The fact records the old admin, new admin, and activation epoch. Devices use this fact to ignore operations from retired administrators.

```rust
pub struct AdminReplacement {
    pub authority_id: AuthorityId,
    pub old_admin: AuthorityId,
    pub new_admin: AuthorityId,
    pub activation_epoch: Epoch,
}
```

This structure defines an admin replacement. Devices enforce this rule locally. The replacement fact is monotone and resolves disputes using journal evidence.

## 6. Epoch Handling

Maintenance logic uses identity epochs for consistency. A maintenance session uses a tuple containing the identity epoch and snapshot epoch. A session aborts if the identity epoch advances. Devices retry under the new epoch.

Snapshot completion sets the snapshot epoch equal to the identity epoch. Garbage collection rules use the snapshot epoch to prune data safely. Upgrade fences use the same epoch model to enforce activation.

```rust
use aura_maintenance::MaintenanceEpoch;

pub struct MaintenanceEpoch {
    pub identity_epoch: Epoch,
    pub snapshot_epoch: Epoch,
}
```

This structure captures epoch state for maintenance workflows. Devices use this structure for guard checks.

## 7. Backup and Restore

Backup uses the latest snapshot and recent journal facts. Devices export an encrypted archive containing the snapshot blob and journal tail. Restore verifies the snapshot signature, hydrates state, and replays the journal tail.

Backups use existing storage and verification effects. No separate protocol exists. Backup correctness follows from snapshot correctness.

## 8. Automatic Synchronization

Automatic synchronization implements periodic journal replication between devices. The synchronization service coordinates peer discovery, session management, and fact exchange. All synchronization uses the journal primitives described in [Journal](105_journal.md).

### 8.1 Peer Discovery and Selection

Devices discover sync peers through the rendezvous system described in [Rendezvous](113_rendezvous.md). The peer manager maintains metadata for each discovered peer. This metadata includes connection state, trust level, sync success rate, and active session count.

```rust
pub struct PeerMetadata {
    pub device_id: DeviceId,
    pub status: PeerStatus,
    pub discovered_at: PhysicalTime,
    pub last_status_change: PhysicalTime,
    pub successful_syncs: u64,
    pub failed_syncs: u64,
    pub average_latency_ms: u64,
    pub last_seen: PhysicalTime,
    pub last_successful_sync: PhysicalTime,
    pub trust_level: u8,
    pub has_sync_capability: bool,
    pub active_sessions: usize,
}
```

This structure tracks peer state for selection decisions. All timestamp fields use `PhysicalTime` from the unified time system. The peer manager calculates a score for each peer using weighted factors. Trust level contributes 50 percent. Success rate contributes 30 percent. Load factor contributes 20 percent. Higher scores indicate better candidates for synchronization.

Devices select peers when their score exceeds a threshold. Devices limit concurrent sessions per peer. This prevents resource exhaustion. Devices skip peers that have reached their session limit.

### 8.2 Session Management

The session manager tracks active synchronization sessions. Each session has a unique identifier and references a peer device. Sessions enforce rate limits and concurrency bounds.

```rust
pub struct SessionManager<T> {
    sessions: HashMap<SessionId, SessionState<T>>,
    config: SessionConfig,
    metrics: Option<MetricsCollector>,
    last_cleanup: PhysicalTime,
    session_counter: u64,
}
```

This structure maintains session state. Sessions are indexed by `SessionId` rather than `DeviceId`. Configuration is provided via `SessionConfig`. All timestamp fields use `PhysicalTime` from the unified time system. Devices close sessions after fact exchange completes. Devices abort sessions when the identity epoch advances. Session cleanup releases resources for new synchronization rounds.

### 8.3 Rate Limiting and Metrics

The synchronization service enforces rate limits per peer and globally. Rate limiting prevents network saturation. Metrics track sync latency, throughput, and error rates.

Devices record metrics for each sync operation. These metrics include fact count, byte count, and duration. Devices aggregate metrics to monitor service health. Degraded peers receive lower priority in future rounds.

### 8.4 Integration with Journal Effects

Automatic synchronization uses `JournalEffects` to read and write facts. The service queries local journals for recent facts. The service sends these facts to peers. Peers merge incoming facts using join-semilattice rules.

All fact validation rules apply during automatic sync. Devices reject invalid facts. Devices do not rollback valid facts already merged. This maintains journal monotonicity.

## 9. Migration Infrastructure

The `MigrationCoordinator` in `aura-agent/src/runtime/migration.rs` orchestrates data migrations between protocol versions.

### 9.1 Migration Trait

```rust
#[async_trait]
pub trait Migration: Send + Sync {
    fn source_version(&self) -> SemanticVersion;
    fn target_version(&self) -> SemanticVersion;
    fn name(&self) -> &str;
    async fn validate(&self, ctx: &MigrationContext) -> Result<(), MigrationError>;
    async fn execute(&self, ctx: &MigrationContext) -> Result<(), MigrationError>;
    async fn rollback(&self, ctx: &MigrationContext) -> Result<bool, MigrationError> {
        Ok(false) // Default: rollback not supported
    }
}
```

Each migration specifies source and target versions, a name for logging, validate/execute methods, and an optional rollback method. The default rollback implementation returns `Ok(false)` to indicate rollback is not supported.

### 9.2 Coordinator API

| Method | Purpose |
|--------|---------|
| `needs_migration(from)` | Check if upgrade is needed |
| `get_migration_path(from, to)` | Find ordered migration sequence |
| `migrate(from, to)` | Execute migrations with validation |
| `validate_migration(from, to)` | Dry-run validation only |

### 9.3 Migration Guarantees

Migrations are ordered by target version. Each migration runs at most once (idempotent via version tracking). Failed migrations leave the system in a consistent state. Progress is recorded in the journal for auditability.

## 10. Evolution

Maintenance evolves in phases. Current OTA work focuses on release identity/provenance, scoped activation, deterministic rollback, and Aura-native distribution. Future phases may add richer replicated cache CRDTs, stronger builder attestation, staged rollout tooling, and automatic snapshot triggers.

Future phases build on the same journal schema. Maintenance semantics remain compatible with older releases.

## 11. Summary

Distributed maintenance uses journal facts to coordinate snapshots, cache invalidation, release distribution, scoped upgrades, and admin replacement. All operations use join-semilattice semantics. All reductions are deterministic. Devices prune storage only after observing snapshot completion. OTA release propagation is eventual, while activation is always local or scope-bound. The system remains consistent across offline and online operation.
