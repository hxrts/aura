# Distributed Maintenance

This document defines the minimum surface necessary for shipping a reliable maintenance stack on day one while staying aligned with the system algebra defined in:

- `docs/001_theoretical_foundations.md`
- `docs/002_system_architecture.md`
- `docs/003_distributed_applications.md`

Future OTA releases can extend what is described here without changing the foundational model.

## 0. Goals and Constraints

1. **Single authoritative journal** – All maintenance operations emit facts into the journal CRDT so offline replicas converge.
2. **Join / meet discipline** – Maintenance facts (snapshots, cache states, upgrade manifests) are modelled as join-semilattice data; constraints (GC eligibility, cache validity) are meet-semilattice predicates evaluated locally.
3. **Choreography-first** – Every distributed flow (snapshot, upgrade) is expressed as an MPST choreography that binds capability guards and journal coupling, following docs/001_theoretical_foundations.md.
4. **Minimal effect surface** – We reuse the existing effect interfaces (journal, storage, crypto, network). No new handler stacks are introduced for launch.
5. **Deterministic rollouts** – Launch behaviour favors determinism and manual control; sophistication (automatic triggers, staged upgrades) is deferred to OTA updates.

## 1. Day-One Feature Set

| Feature | User-visible goal | Day-1 implementation | Evolves via OTA |
|---------|------------------|----------------------|-----------------|
| **Snapshot + GC** | Keep ledger bounded; enable restore | Manual `Snapshot_v1` ceremony with fixed high-water mark (100 MB). GC prunes local data strictly older than latest committed snapshot. | Automatic triggers, incremental snapshots, lease-based GC. |
| **Cache invalidation** | Prevent stale derived data | Journal emits `CacheInvalidated` facts. Each device enforces per-key epoch floors locally. | Distributed cache CRDT + push invalidations. |
| **OTA upgrade** | Roll out new choreographies/effect stacks securely | Single `UpgradeCoordinator` choreography with semantic versioning (MAJOR = hard fork, MINOR = soft fork). Each identity decides whether devices auto-upgrade or require explicit operator approval. Activation fences are only used for hard forks. | Staged rollouts, multi-binary bundles, richer policy controls. |
| **Admin override / fork** | Users can replace a malicious admin and continue as a fork | Emit `AdminReplaced` facts (stub) that record an account-scoped admin transition; enforcement deferred. | Automatic capability handoff, rekeying, policy migration. |
| **Epoch/session fences** | Ensure maintenance sessions respect authority windows | All maintenance choreographies run under `(identity_epoch, snapshot_epoch)`; sessions abort on epoch advance. | Finer-grained maintenance epochs if needed. |
| **Backup / restore** | Bring a new device online quickly | CLI surfaces to export latest snapshot plus journal tail; restore verifies threshold signature then replays tail. | Automated / differential backups. |

## 2. Shared Maintenance Primitives

### 2.1 Journal schema

We extend the journal's `MaintenanceEvent` sum type with four launch-time variants. They obey the CRDT rules from docs/001_theoretical_foundations.md (grow-only facts).

```rust
pub enum MaintenanceEvent {
    SnapshotProposed {
        proposal_id: Uuid,
        proposer: DeviceId,
        target_epoch: u64, // identity epoch fence
        state_digest: Hash32,
    },
    SnapshotCompleted {
        proposal_id: Uuid,
        snapshot: SnapshotV1,
        participants: BTreeSet<DeviceId>,
        threshold_signature: ThresholdSignature,
    },
    CacheInvalidated {
        keys: Vec<CacheKey>,
        epoch_floor: u64, // identity epoch
    },
    UpgradeActivated {
        package_id: Uuid,
        to_version: ProtocolVersion,
        activation_fence: IdentityEpochFence,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IdentityEpochFence {
    pub account_id: AccountId,
    pub epoch: u64,
}
```

Devices subscribe to these facts and update local state monotonically (join). GC eligibility, cache validity, and upgrade readiness are computed via local meet predicates over the accumulated facts.

### 2.2 Effect usage

- **JournalEffects**: append `MaintenanceEvent`s, apply writer fences during snapshots.
- **StorageEffects**: read/write snapshot blobs, delete pruned segments.
- **CryptoEffects**: compute hashes and threshold signatures for snapshots and upgrades.
- **NetworkEffects**: transfer snapshot payloads and signed upgrade artifacts (e.g., binaries or choreography bundles).

No new trait is required: maintenance flows simply compose the existing effect interfaces in their choreographies.

### 2.3 Serialization / wire formats

Maintenance data uses the canonical CBOR-based serialization defined in the whole-system model:

- `MaintenanceEvent` facts share the journal’s CBOR encoder. Launch version (`MaintenanceEventVersion = 1`) fixes the field layout; additions must be backward-compatible and guarded by optional flags.
- `Snapshot_v1` blobs are deterministic CBOR structures (sorted maps, canonical byte arrays) hashed via `hash_canonical`. This ensures every participant derives the same digest.
- `CacheInvalidated { keys, epoch_floor }` serializes as a CBOR array `[Vec<CacheKey>, u64]`.
- OTA metadata (`UpgradeProposal`) stores the artifact hash (BLAKE3) and URL/identifier, so devices verify the payload after downloading via HTTP/git/other transports.

Rule of thumb: every maintenance artifact has (a) a canonical CBOR encoding if it lives in the journal and (b) a content hash stored in the journal if it lives off-chain (snapshots, upgrade bundles). No ad-hoc binary formats or nondeterministic encoders are allowed at launch.

## 3. Feature Specifications

### 3.1 Snapshot + Garbage Collection

**Trigger**: operator runs `aura-cli snapshot propose` or the agent daemon crosses a 100 MB journal watermark.

**Choreography** (`Snapshot_v1`):
1. **Propose**: proposer publishes `SnapshotProposed` with current identity epoch and state digest (deterministic hash of journal snapshot). Capability guard: `need(snapshot_propose)`.
2. **Validate**: peers recompute digest; on success they append `ApproveSnapshot` votes (local state, not journal) and sign the digest.
3. **Commit**: proposer aggregates an `M-of-N` signature, publishes `SnapshotCompleted`.

**Writer fencing**: while a proposal is outstanding, new operations buffer locally (session type guard `[▷ Δfacts]` prevents commits) until either the snapshot completes or is abandoned.

**GC action**: when a device observes `SnapshotCompleted` and verifies the signature:
- Drop journal segments whose operations precede the snapshot digest.
- Delete storage blobs whose tombstones predate the snapshot epoch.
- Delete SBB envelopes tied to key-rotation epochs < snapshot epoch.

**Restore**:
1. Fetch the latest `SnapshotCompleted` event and download the referenced snapshot blob (via `StorageEffects`).
2. Verify threshold signature and digest; hydrate local journal state.
3. Replay post-snapshot journal tail to catch up.

### 3.2 Cache Invalidation

- Every state mutation that affects derived data (tree policies, storage manifest updates) emits `CacheInvalidated { keys, epoch_floor }`.
- Clients maintain a map `cache_floor: CacheKey -> u64`. Serving data requires `current_identity_epoch >= cache_floor[key]`. Otherwise the client recomputes that entry from authoritative state.
- This mechanism is purely local; no replicated cache CRDT is shipped day one. The semantics follow the meet predicate pattern: serveable cache entries = join(state) ∧ meet(epoch constraint).

### 3.3 OTA Upgrade

**State**: every device advertises two monotone sets inside `DeviceMetadata`:
- `supported_protocols`: protocol versions (choreography/effect bundles) the device can execute. This list is derived from the installed binary; if a device has not yet installed the update it simply omits the new version, ensuring it cannot start a choreography other peers cannot run.
- `upgrade_policy`: either `Manual` (operator approval required) or `Auto` (device consents to any upgrade signed by the admin roster).

**Choreography** (`UpgradeCoordinator`):
1. **Proposal**: admin publishes upgrade metadata (package ID, semantic `ProtocolVersion`, artifact hash, download URL/git ref) and specifies `upgrade_kind`:
   - `SoftFork` (MINOR bump): no activation fence by default. Devices that install the new binary and opt in can use the new version with other opted-in peers; legacy peers continue with the previous version.
   - `HardFork` (MAJOR bump): includes an identity-scoped activation fence `IdentityEpochFence { account_id, epoch }`. After the ratchet tree reaches that epoch, everyone must use the new version.
2. **Opt-in**: devices fetch the artifact (signed choreography/effect bundle), verify the hash, and, depending on `upgrade_policy`, either auto-acknowledge or wait for operator approval before appending a readiness fact to the journal. For soft forks, this readiness simply signals “I can speak version V”; no cutover is enforced.
3. **Activation**:
   - **Soft fork**: no automatic aborts. New sessions negotiate the highest mutually supported `ProtocolVersion`; older versions remain available until the admin later decides to stage a fence (optional).
  - **Hard fork**: once enough devices have opted in *and* the account’s ratchet tree reaches the `activation_fence.epoch`, the admin writes `UpgradeActivated`. From that point the session runtime aborts any still-running session whose `(identity_epoch, version)` no longer satisfies the fence, and orchestrators refuse to start new sessions with peers lacking readiness for V. This is the disruptive path; operators should drain long-running flows first.

**Distribution channels**: mobile apps (iOS, Android) and browser bundles are still delivered via their native stores. The OTA protocol only coordinates the *protocol logic* (choreographies/effect stacks) that those binaries already contain—no bytecode VM or Wasm payloads are shipped at launch. Operators must publish a binary that includes the new code path before opting the device into version V. Devices running an older binary simply keep their `supported_protocols` manifest capped at the older version until they download the updated app. Once the new binary is installed, the operator (or `Auto` policy) can mark readiness in the journal, and the OTA choreography handles activation. This keeps app-store deployments and protocol upgrades loosely coupled while ensuring on-device code always matches the declared capability.

*Future option*: nothing here forbids adding an interpreted choreography runtime later. If we introduce a portable IR + sandbox, the OTA payload would simply reference that IR artifact via the same content-addressed mechanism, and devices advertising interpreter support could opt in dynamically. Until that runtime exists, OTA activation remains a switch for already-compiled code.

**Group coordination**: because binaries may lag, before starting any maintenance or domain session the orchestrator inspects each participant’s `supported_protocols` set. If any peer lacks the requested version, the call fails fast (soft fork) or refuses entirely (hard fork once the fence is active). Admin dashboards can surface which peers are missing readiness facts so group operators can prod them to upgrade before initiating critical ceremonies.

Because epochs are scoped to an identity/domain, there is no global upgrade clock. For soft forks the ledger simply records "device X now supports version V" and negotiation chooses the best mutual version. For hard forks we additionally record an activation fence, enforced by the session typing rules: a session carrying `(identity_epoch, version)` must abort if its identity epoch is older than the fence recorded in the journal. This is the same "session epoch locks a slice of journal state" property described in the whole-system calculus (docs/001_theoretical_foundations.md).

Launch-time simplification: upgrades are single-wave (no canaries), but devices can refuse by keeping `upgrade_policy = Manual` and withholding approval. If too many devices refuse, the admin can remove them via normal governance.

### 3.4 Admin Replacement / Fork Workflow

Administrators can become unavailable or malicious. Devices must be able to fork away from such an admin and install a new one while keeping the same identity state. Launch behaviour provides a stub mechanism that captures the intent in the journal and defers full enforcement to later OTA updates.

- `MaintenanceEvent::AdminReplaced(AdminReplaced)` records `{ account_id, previous_admin, new_admin, activation_epoch }`.
- `aura-agent::MaintenanceController::replace_admin_stub` appends this fact and persists it under `maintenance:admin_override:<account_id>` for local enforcement.
- `aura-cli admin replace --account <uuid> --new-admin <device_uuid> --activation-epoch <u64>` exposes the operation to operators; it piggybacks on the agent’s maintenance controller so the same flow is available to daemons.
- Replay semantics: replicas treat the fact as a monotone declaration. Higher-layer tooling can refuse future admin-signed operations whose `(account_id, epoch)` precede the recorded activation epoch, effectively letting users fork away by ignoring the old admin’s facts.

A future OTA will:
1. Tie `AdminReplaced` facts into the capability lattice (revoking `previous_admin` once the activation epoch passes).
2. Trigger rekeying choreographies so the new admin receives the appropriate signing capability.
3. Surface CLI/agent UX for discovering and approving competing admin replacements (multi-branch forks).

Until then, emitting the fact provides the documented escape hatch—users can coordinate out-of-band, agree on a new admin, and rely on the journal evidence if disputes occur. Local runtimes store the override facts and expose `ensure_admin_allowed` helpers so capability bridges can start enforcing the activation epoch without waiting for a network-wide upgrade.

### 3.5 Epoch / Session Handling

- Maintenance sessions run under a `(identity_epoch, snapshot_epoch)` tuple passed through the `choreography!` macro. Guards (`need_epoch(identity_epoch)`) ensure the session operates on a consistent slice of the journal.
- Identity epochs are per-account, not global. A session “locks” the subset of the journal it reads/writes; if that identity epoch advances (new tree operation), the runtime aborts the session and the caller retries under the new epoch.
- Snapshot completion sets `snapshot_epoch = identity_epoch` at the moment the snapshot fact is appended. GC predicates require the local snapshot epoch to dominate the epoch of the data being pruned.
- OTA activation refers to the same identity epoch fence. When `UpgradeActivated { activation_fence }` appears, orchestrators for that account compare the session’s epoch with the fence before executing.

### 3.6 Backup / Restore

CLI workflow:
1. `aura-cli backup export` downloads the latest snapshot blob plus the last 1 000 journal events, packages them into an encrypted TAR.
2. `aura-cli backup restore <tar>` verifies snapshot signature and replays the included journal tail.

This is a user-facing wrapper around the same snapshot/restore primitives described above; no new protocol is introduced.

## 4. Evolution Roadmap

| Phase | Additions (via OTA) |
|-------|---------------------|
| **Phase 1 (launch)** | Ship exactly the mechanisms above. |
| **Phase 2** | Lease-based GC evidence (join/meet tokens), replicated cache CRDT, staged OTA rollouts. |
| **Phase 3** | Proxy re-encryption for old blobs, automatic snapshot triggers, managed backups to external storage. |

Each phase builds on the day-one journal schema and effect surface; no foundational changes are necessary.

## 5. Testing & Operations

- Use the deterministic simulator (`docs/600_simulation_framework.md`) to execute maintenance choreographies with injected faults (network partitions, Byzantine voters). Scripts live alongside other protocol tests.
- Provide CLI hooks for every manual action (`snapshot propose`, `snapshot approve`, `snapshot restore`, `cache invalidate`, `upgrade propose`, `upgrade activate`).
- Publish operator runbooks that explain the invariants (writer fencing expectations, cache invalidation semantics, restore verification steps).

## 6. Summary

This spec captures the entire day-one distributed maintenance surface in one place. It aligns directly with the algebraic model (facts are join-semilattice data; constraints are meet predicates), uses the existing effect system and session type machinery, and keeps behaviour deterministic. Day-one deliverables include:

- Manual-but-safe snapshots + GC with deterministic restore.
- Journal-driven cache invalidation enforced locally.
- OTA upgrades distributing signed choreography/effect bundles, with per-device `Manual` / `Auto` opt-in policies enforced via identity-epoch fences.
- Epoch/session guards that abort maintenance flows when the underlying authority window changes.
- CLI-driven backup/restore built on the snapshot primitives.
