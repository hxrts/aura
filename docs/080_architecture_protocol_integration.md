# 080 · Architecture & Protocol Integration

**Status:** Integration Specification  
**Target:** Phase 1 Completion

## Introduction

This document specifies the complete architecture and protocol design for Aura's decentralized identity and threshold cryptography system. It presents a production-ready design that integrates peer-to-peer threshold protocols, CRDT-based coordination, guardian recovery, and transport abstraction into a coherent, layered architecture.

**What This Document Covers:**

Aura enables multiple devices to jointly control a threshold Ed25519 identity without any central coordinator. This document specifies how devices coordinate through three core distributed protocols:

1. **Peer-to-Peer Deterministic Key Derivation (P2P DKD)** - Devices jointly derive context-specific identities from a shared master secret using a commitment-reveal protocol anchored in a CRDT ledger
2. **Peer-to-Peer Resharing** - Devices coordinate changes to the participant set (add/remove devices, adjust threshold) while preserving the group public key through Shamir secret sharing
3. **Guardian-Based Recovery** - Account recovery through trusted guardians who hold encrypted shares, with mandatory cooldown periods and veto capabilities

These protocols are orchestrated through a replicated CRDT ledger that provides eventually consistent state synchronization, eliminating the need for a trusted coordinator. The architecture is transport-agnostic, with a clean abstraction layer that allows different network implementations.

**Architectural Principles:**

- **Three-Layer Architecture**: Clear separation between Application APIs, Orchestration (protocol state machines), and Execution (cryptographic primitives and CRDT operations)
- **Ledger-Driven Coordination**: All protocol phases materialize as signed events in the CRDT ledger, enabling auditing, crash recovery, and Byzantine detection
- **Transport Abstraction**: Core protocols depend only on a `Transport` trait; default implementation provided but swappable by library users
- **Threshold Security**: Critical operations require M-of-N agreement to prevent unilateral actions by compromised devices
- **Forward Secrecy**: Resharing invalidates old shares; session epochs prevent replay attacks
- **Byzantine Tolerance**: Commitment-reveal schemes, verification phases, and quorum requirements protect against malicious participants

**Key Integration Points:**

- **P2P DKD** drives threshold derivation by writing protocol phases (commitment, reveal, aggregation) into the ledger
- **P2P Resharing** coordinates participant changes through the same ledger workflow, enabling add/remove/adjust operations
- **Guardian Recovery** reuses the resharing machinery after reconstructing shares from guardian approvals with cooldown enforcement
- **CRDT Operations** anchor each P2P phase and are submitted directly via the ledger API
- **Three-layer architecture** maps cleanly to all protocol phases, separating high-level APIs from low-level primitives
- **Unified error handling** across all protocols with context-rich error types and natural propagation through layers
- **Transport Abstraction** enables network protocol flexibility (Noise XX, libp2p, WebRTC, custom) via dependency injection

This specification provides the foundation for Phase 1 implementation, with all protocol details, security properties, error handling, testing strategies, and integration patterns necessary for production deployment.

---

## Part 1: P2P DKD Integration

### Conceptual Mapping

The P2P DKD protocol maps a high-level API call for threshold identity derivation into a series of coordinated CRDT operations. This layered approach separates the user-facing API from the underlying cryptographic protocol:

```
Application Layer (conceptual):
    agent.derive_context_identity_threshold(capsule, participants, threshold)
                            ↓
Protocol Implementation (concrete):
    - Phase 0: Initiate DKD session in CRDT
    - Phase 1: Commitment phase (each peer)
    - Phase 2: Reveal phase (each peer)
    - Phase 3: Aggregation (any peer)
    - Phase 4: Identity expansion (local)
```

### Implementation Integration

The DKD orchestration module coordinates the commitment, reveal, and aggregation phases by layering the protocol onto the CRDT ledger:

1. **Session bootstrap** – `dkd.rs` publishes a `DkdSessionInit` event to the ledger with participant IDs, threshold, and capsule hash.
2. **Commitment phase** – each device writes a `DkdCommitment` event containing its nonce commitment; peers validate via the CRDT feed.
3. **Reveal phase** – devices follow with `DkdReveal` events (nonce + share contribution). Receiving peers verify against commitments.
4. **Aggregation** – once the CRDT reflects the required number of valid reveals, any device aggregates points locally and derives the context identity.

Because all phases materialize as CRDT entries, the agent can resume interrupted sessions and auditors can review protocol transcripts.

### Usage Example

```rust
// High-level API (existing crate)
let identity = agent.derive_context_identity_threshold(
    &capsule,
    DerivationParticipants {
        devices: vec![device1, device2, device3],
        threshold: 2,
    },
).await?;
```

Internally this helper coordinates CRDT updates and gating logic described above; no additional transform abstraction is required in the prototype.

### Session Lifecycle Management

**Session States**: `Pending`, `Active`, `Completed`, `Aborted`, `TimedOut`

**Lifecycle Controls:**
- **Collision Handling**: First valid `InitiateDkdSession` wins; duplicates rejected with proof
- **Timeout**: Sessions have `ttl_in_epochs`; any peer can propose `SessionFinalized { status: TimedOut }` when `current_epoch > start_epoch + ttl`
- **Abort**: Threshold-signed `SessionAbort` when quorum agrees session is dead
- **Garbage Collection**: Terminal sessions (`Completed`, `Aborted`, `TimedOut`) eligible for compaction

**Byzantine Validation:**
- **Commitment Phase**: Verify signature, enforce one submission per participant before storing
- **Reveal Phase**: Validate reveal ↔ commitment linkage (hash matches); on mismatch emit `ParticipantBlamed { session_id, participant_id }` and abort
- **Participant Blacklist**: Blamed participants enter cooldown period

**Participant Redemption Path:**

**Problem**: Transient network errors or client bugs can cause false positives, unfairly blacklisting a device that should remain in the quorum.

**Solution - Health Check Protocol:**

After cooldown expires, blamed participant can request reinstatement:
1. **Request**: Device proposes `ParticipantRedemptionRequest { device_id, blame_event_ref }`
2. **Health Check**: Device must successfully complete a no-op threshold signing operation with other participants (proves it can behave correctly)
3. **Reinstatement**: On success, any participant can propose `ParticipantReinstated { device_id, reinstated_at_epoch }` to restore device to active set
4. **Failure**: If health check fails, device remains blacklisted and must wait for another cooldown cycle

**CRDT Events:**
- `ParticipantBlamed { session_id, participant_id, blamed_at_epoch, reason }`
- `ParticipantRedemptionRequest { participant_id, blame_ref, requested_at_epoch }`
- `ParticipantHealthCheckInitiated { participant_id, test_message_hash }`
- `ParticipantHealthCheckCompleted { participant_id, signature, verified_by }`
- `ParticipantReinstated { participant_id, reinstated_at_epoch }`

**Implementation Notes:**
- Cooldown duration: configurable, default 1 hour (≈360 epochs)
- Health check uses threshold signature on deterministic test message: `blake3("health_check" || device_id || current_epoch)`
- Multiple devices verify health check signature; requires threshold agreement for reinstatement
- Repeat offenders (blamed multiple times) may face exponentially increasing cooldowns

**Logical Clock for Epoch-Based Timeouts:**

**Problem**: Epoch-based TTLs don't expire without a mechanism to advance epochs during idle periods.

**Solution - Self-Contained Logical Clock:**

The system uses a purely logical clock embedded in the CRDT, advancing on every write or explicit tick event. No reliance on wall-clock synchronization.

**State Structure:**

```rust
pub struct AccountState {
    pub logical_epoch: u64,  // Monotonically increasing logical time
    // ... other fields
}

pub struct Event {
    pub epoch_at_write: u64,  // Epoch when this event was written
    pub device_id: DeviceId,
    pub signature: Vec<u8>,
    pub payload: EventPayload,
}

pub struct DkdSessionState {
    pub start_epoch: u64,
    pub ttl_in_epochs: u64,  // e.g., 100 epochs
    // ... other fields
}
```

**Monotonic Progression Rules:**

1. **On Ingestion**: Device sets `local_epoch = max(local_epoch, logical_epoch_from_state)`
2. **Before Write**: Device increments `local_epoch += 1`, stamps event with `epoch_at_write = local_epoch`
3. **State Update**: Event application updates `AccountState.logical_epoch = epoch_at_write`
4. **Validation**: Peers verify monotonic property: `epoch_at_write > previous_epoch` and reject out-of-order events

**Heartbeat Tick Events:**

During idle periods, any device can advance the epoch with a tick event:

```rust
pub enum EventPayload {
    // ... other events
    EpochTick {
        state_hash: [u8; 32],  // Hash of current CRDT state (proof tick is on latest view)
        previous_epoch: u64,   // Expected previous epoch for verification
    },
}
```

**Tick Preconditions:**
- `current_epoch - last_write_epoch >= tick_min_gap` (e.g., 10 seconds worth of "idle" operations)
- No other writes in-flight (device has synchronized view)
- Includes `state_hash` so peers can verify tick was computed on consistent state
- Device-signed to prevent unauthorized time manipulation

**Tick Rate Limiting:**
- Maximum tick rate: 1 per device per `tick_min_gap` epochs
- If device exceeds rate, peers reject tick (prevents fast-forwarding attacks)
- Multiple devices can tick simultaneously; CRDT merge handles conflicts deterministically

**Session Timeout Implementation:**

```rust
impl DeviceAgent {
    pub async fn check_session_timeouts(&self) -> Result<Vec<Uuid>> {
        let current_epoch = self.ledger.get_logical_epoch().await?;
        let mut timed_out = Vec::new();
        
        for session in self.ledger.get_active_sessions().await? {
            if current_epoch > session.start_epoch + session.ttl_in_epochs {
                // Session expired
                self.ledger.finalize_session(
                    session.id,
                    SessionStatus::TimedOut,
                    current_epoch,
                ).await?;
                timed_out.push(session.id);
            }
        }
        
        Ok(timed_out)
    }
    
    pub async fn maybe_emit_tick(&self) -> Result<()> {
        let state = self.ledger.get_account_state().await?;
        let last_write = self.ledger.get_last_event().await?;
        
        // Check if tick is needed and allowed
        if state.logical_epoch - last_write.epoch_at_write >= self.config.tick_min_gap {
            let state_hash = self.ledger.compute_state_hash().await?;
            
            self.ledger.append_event(Event {
                epoch_at_write: state.logical_epoch + 1,
                device_id: self.device_id,
                signature: self.sign_tick(state_hash, state.logical_epoch)?,
                payload: EventPayload::EpochTick {
                    state_hash,
                    previous_epoch: state.logical_epoch,
                },
            }).await?;
        }
        
        Ok(())
    }
}

impl AccountLedger {
    pub fn validate_event(&self, event: &Event) -> Result<()> {
        // Verify monotonic progression
        let current_epoch = self.state.logical_epoch;
        
        if event.epoch_at_write <= current_epoch {
            return Err(LedgerError::NonMonotonicEpoch {
                expected_gt: current_epoch,
                got: event.epoch_at_write,
            });
        }
        
        // For tick events, verify state hash matches
        if let EventPayload::EpochTick { state_hash, previous_epoch } = &event.payload {
            if *previous_epoch != current_epoch {
                return Err(LedgerError::TickEpochMismatch);
            }
            
            let computed_hash = self.compute_state_hash()?;
            if state_hash != &computed_hash {
                return Err(LedgerError::TickStateHashMismatch);
            }
        }
        
        // Verify signature
        self.verify_event_signature(event)?;
        
        Ok(())
    }
}
```

**Security Properties:**

- **Monotonic Guarantee**: Every event increments epoch; time never goes backward
- **Rate Limiting**: Tick frequency bounded by `tick_min_gap` per device
- **State Binding**: Tick includes `state_hash` proving it was computed on consistent view
- **Byzantine Resistance**: Invalid ticks rejected; malicious device cannot fast-forward arbitrarily
- **Liveness**: Any device can emit tick; stuck sessions eventually expire even if some devices offline

**Operational Notes:**

- Normal protocol operations (DKD commitments, resharing events, recovery approvals) all increment epoch naturally
- Ticks only needed during extended idle periods or to ensure timeout detection
- A session with `ttl_in_epochs = 100` expires after 100 CRDT writes (or equivalent tick events)
- Approximate wall-time correlation: if average write rate is 1/second, 100 epochs ≈ 100 seconds

---

## Part 2: Recovery Protocol Integration

### Overview

The recovery protocol enables account recovery through guardian-based share reconstruction with mandatory cooldown. It's a multi-phase stateful protocol that naturally fits the unified architecture.

**Recovery Workflow:**
1. **Initiation**: User creates recovery request with guardians (flexible quorum: invite N, require M where N > M)
2. **Approval Collection**: Guardians approve by providing encrypted shares with replay protection
3. **Cooldown Enforcement**: Mandatory wait period (24-48 hours default) with nudge capability
4. **Execution**: Shares reconstructed → resharing protocol → new device added
5. **Veto/Cancel**: Optional guardian veto or user cancellation during cooldown

**Flexible Quorum Design:**
- Invite more guardians than required threshold (e.g., invite 5, require 3)
- Provides social redundancy against unresponsive guardians
- Recovery proceeds when threshold met, doesn't wait for all

**Guardian Nudging:**
- `NudgeGuardian` CRDT event triggers high-priority UI notification
- In-band signal observable in protocol (not just out-of-band contact)
- Transport layer converts ledger event to device notification

### Recovery Implementation Hooks

The recovery manager models the recovery state machine and coordinates with the CRDT ledger and resharing protocols:

1. **Initiate** – `RecoveryManager::start_request` writes `RecoveryInitiated(request_id, ...)` to the CRDT and tracks the in-memory state.
2. **Collect approvals** – guardians call `approve_recovery`, which:
   - fetches the `RecoverySessionState` from the CRDT,
   - decrypts the guardian envelope locally,
   - re-encrypts the share for the new device,
   - emits `RecoveryApprovalRecorded { guardian_id, commitment_hash }` into the ledger.
3. **Cooldown** – once the CRDT reflects `required_approvals`, an `RecoveryCooldownStarted` entry records the deadline; the manager enforces veto/cancel semantics until expiry.
4. **Execute** – after cooldown, the agent reconstructs the share set (verifying commitments as described below), runs resharing, and publishes `RecoveryCompleted { new_session_epoch }`.
5. **Cancel/Veto** – `RecoveryCancelled` and `RecoveryVetoed` events short-circuit the flow.

### Recovery CRDT State Structure

Recovery state is stored in the CRDT ledger, enabling coordination between guardians and the recovering device:

```rust
pub struct RecoverySessionState {
    pub session_id: Uuid,
    pub account_id: AccountId,
    pub new_device_id: DeviceId,
    pub guardian_ids: HashSet<GuardianId>,
    pub required_approvals: usize,
    pub cooldown_seconds: u64,
    pub status: RecoveryStatus,  // PendingApprovals | CooldownActive | ReadyToExecute | Completed
    pub approvals: HashMap<GuardianId, GuardianApproval>,
    // ... veto, cancellation fields
}
```

**Key methods**: `threshold_met()`, `cooldown_elapsed()`, `remaining_cooldown()`, `ready_to_execute()`

### Guardian Agent Integration

```rust
impl GuardianAgent {
    pub async fn approve_recovery(&self, request_id: Uuid) -> Result<GuardianApproval> {
        // 1. Verify authorization
        // 2. Decrypt guardian envelope → recovery share
        // 3. Encrypt share for new device with HPKE (request_id, guardian_id as associated data)
        // 4. Compute commitment: blake3(request_id || guardian_id || hpke_ciphertext || nonce)
        // 5. Sign and record approval in CRDT
        // 6. Store nonce to prevent replay
    }
    
    pub async fn veto_recovery(&self, request_id: Uuid, reason: Option<String>) -> Result<()> {
        // Veto during cooldown, record in CRDT, terminate recovery
    }
}
```

**Replay Protection:**
- HPKE encryption includes `(request_id, guardian_id)` as associated data
- Commitment hash binds: `blake3(request_id || guardian_id || hpke_ciphertext || nonce)`
- Per-guardian nonces stored to reject repeat approvals
- Prevents cross-session replay attacks

**Ciphertext Management:**
- **Retention**: HPKE ciphertexts eligible for deletion after `RecoveryCompleted`
- **Privacy**: Pad ciphertexts to fixed sizes (e.g., 4KB buckets) to prevent size-based correlation
- **TTL**: Completed recovery sessions enqueue ciphertexts for garbage collection or re-encryption to cold storage

### Recovery Protocol Phases Across Three Layers

```
┌─────────────────────────────────────────────────────────────┐
│ Layer 3: Application API                                    │
│                                                             │
│  // Recovering user (on new device)                         │
│  let recovery_id = agent.initiate_recovery(                 │
│      guardians,                                             │
│      required_approvals: 2,                                 │
│      cooldown: Duration::from_hours(24)                     │
│  ).await?;                                                  │
│                                                              │
│  // Guardian side                                           │
│  guardian.approve_recovery(recovery_id).await?;             │
│                                                              │
│  // After cooldown                                          │
│  let device = agent.complete_recovery(recovery_id).await?;  │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 2: Orchestration (Recovery State Machine)            │
│                                                              │
│  Recovery Protocol Phases:                                 │
│  ├─ Phase 0: Initiation                                    │
│  │   ├─ Validate guardian set                              │
│  │   ├─ Create recovery request                            │
│  │   └─ CRDT: InitiateRecoverySession                      │
│  │                                                          │
│  ├─ Phase 1: Approval Collection                           │
│  │   ├─ Guardians decrypt their envelopes (local)          │
│  │   ├─ Encrypt shares for new device (HPKE)              │
│  │   ├─ Sign approval                                      │
│  │   ├─ CRDT: RecordRecoveryApproval (per guardian)       │
│  │   └─ Check threshold → transition to cooldown           │
│  │                                                          │
│  ├─ Phase 2: Cooldown Enforcement                          │
│  │   ├─ Wait for cooldown_completes_at timestamp          │
│  │   ├─ Allow guardian veto (CRDT: VetoRecovery)          │
│  │   ├─ Allow user cancellation (CRDT: CancelRecovery)    │
│  │   └─ Check elapsed → transition to ready                │
│  │                                                          │
│  ├─ Phase 3: Share Reconstruction                          │
│  │   ├─ Decrypt guardian shares (HPKE)                     │
│  │   ├─ Lagrange interpolation → master share             │
│  │   └─ Verify reconstructed share (test signature)        │
│  │                                                          │
│  ├─ Phase 4: Resharing (Add New Device)                   │
│  │   ├─ Initiate resharing session                         │
│  │   ├─ Old participants: guardians                        │
│  │   ├─ New participants: guardians + new device          │
│  │   ├─ Execute P2P resharing protocol                    │
│  │   │   (See Part 1: P2P Resharing)                      │
│  │   └─ Wait for resharing complete                       │
│  │                                                          │
│  └─ Phase 5: Completion                                    │
│      ├─ Bump session epoch                                 │
│      ├─ Invalidate old presence tickets                    │
│      ├─ Apply Event::RecoveryComplete                      │
│      └─ CRDT: CompleteRecovery                             │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 1: Execution (Primitive Operations)                   │
│                                                              │
│  Cryptographic Primitives:                                  │
│  ├─ HPKE::encrypt(share, recipient_pk)                     │
│  ├─ HPKE::decrypt(encrypted_share, device_secret)          │
│  ├─ Lagrange::interpolate(shares, participant_ids)         │
│  ├─ FROST::sign(message, reconstructed_share)              │
│  └─ Ed25519::verify(signature, group_pk, message)          │
│                                                              │
│  CRDT Operations:                                           │
│  ├─ ledger.initiate_recovery_session()                     │
│  ├─ ledger.record_recovery_approval()                      │
│  ├─ ledger.veto_recovery()                                 │
│  ├─ ledger.cancel_recovery()                               │
│  ├─ ledger.mark_recovery_ready()                           │
│  ├─ ledger.complete_recovery()                             │
│  └─ ledger.get_recovery_session()                          │
│                                                              │
│  Time Operations:                                           │
│  ├─ current_timestamp()                                     │
│  ├─ cooldown_elapsed()                                      │
│  └─ remaining_cooldown()                                    │
│                                                              │
│  Transport Operations:                                      │
│  ├─ transport.notify_guardians(recovery_request)           │
│  ├─ transport.sync_crdt_state()                            │
│  └─ transport.notify_completion(new_device)                │
└─────────────────────────────────────────────────────────────┘
```

### Recovery CRDT Operations

The ledger types include the following recovery-specific entries (see `aura_ledger::operation`):
- `RecoveryInitiated { request_id, invited_guardians, required_threshold, cooldown }` - Supports flexible quorum (invited > required)
- `RecoveryApprovalRecorded { request_id, guardian_id, commitment_hash, nonce }` - Replay-protected approval
- `GuardianNudged { request_id, guardian_id, nudge_count }` - Trigger UI notification for unresponsive guardian
- `RecoveryVetoed { request_id, guardian_id, reason }`
- `RecoveryCancelled { request_id, device_id, reason }`
- `RecoveryCooldownStarted { request_id, completes_at }`
- `RecoveryCompleted { request_id, new_session_epoch }`
- `RecoveryFailed { request_id, reason }`

Each entry carries enough context for replicas to reconstruct state and enforce invariants (threshold counts, cooldown timers, veto rules). There is no automatic protocol derivation layer in the prototype; instead the agent emits and applies these events explicitly through the `aura_ledger` API.

**Share-Integrity Checks (Enhanced with Replay Protection and Merkle Proofs)**
- Each guardian approval stores `commitment_hash = blake3(request_id || guardian_id || HPKE_ciphertext || nonce)` binding the commitment to this specific recovery session
- HPKE encryption includes `(request_id, guardian_id)` as associated data, preventing cross-session ciphertext replay
- Per-guardian nonces are stored and checked; duplicate nonces cause `RecoveryFailed { reason: ReplayDetected }`
- The recovering device recomputes commitment hash and compares before accepting a share
- **Post-Compaction Verification**: Guardian submits `MerkleProof` with their approval; recovering device verifies decrypted share against persisted `DkdCommitmentRoot` (see Part 3: CRDT Compaction). This enables recovery even after original commitment events are pruned.
- Verification flow: decrypt share → recompute partial point → hash commitment → verify Merkle proof against `DkdCommitmentRoot.merkle_root`
- Only after all shares pass Merkle proof validation does the orchestrator interpolate and proceed to resharing; otherwise it emits `RecoveryFailed { reason: CommitmentMismatch }` and reverts

**Guardian Nudge Mechanism:**
- `GuardianNudged` event has no cryptographic effect but serves as observable in-band signal
- Transport layer watches for nudge events and generates high-priority UI notifications
- Multiple nudges tracked with `nudge_count` to prevent spam
- Provides protocol-level "social reminder" without requiring out-of-band communication

### Security Properties

Recovery protocol security properties:

| Property | Mechanism |
|----------|-----------|
| **Authorization** | Only authorized guardians can approve; threshold required |
| **Cooldown Enforcement** | Mandatory 24-48 hour wait; allows veto/cancellation |
| **Guardian Veto** | Any guardian can veto during cooldown |
| **User Cancellation** | Threshold-signed cancellation anytime during cooldown |
| **Share Confidentiality** | Guardian shares HPKE-encrypted for new device |
| **Forward Secrecy** | Resharing invalidates guardian shares after recovery |
| **Audit Trail** | All recovery events logged in CRDT; immutable history |
| **Replay Protection** | Session IDs, signatures, timestamps prevent replay |
| **Byzantine Tolerance** | Threshold requirement; test signature validates reconstruction |

### Recovery Error Types

Recovery errors extend `AgentError`:
- `RecoverySessionNotFound` - Session ID not in CRDT
- `RecoveryNotAuthorized` - Not an authorized guardian
- `RecoveryNotReady` - Cooldown not complete or insufficient approvals
- `RecoveryAlreadyComplete` - Session already executed
- `RecoveryTerminated` - Vetoed, cancelled, or failed
- `RecoveryCannotVeto/Cancel` - Invalid state for operation
- `RecoveryInsufficientApprovals` - Threshold not met
- `GuardianEnvelopeDecryptionFailed` - HPKE decryption error
- `RecoveryReconstructionFailed` - Lagrange interpolation error
- `RecoveryTimeout` - Recovery session expired

---

## Part 3: CRDT Choreography & State Management

### Ledger Compaction and Historical Verification

**Problem**: CRDT ledger grows indefinitely with transient session data; verifying historical facts (especially for recovery) requires storing all events.

**Solution - Merkleized Commitments with Persistent Roots:**

**Core Principle**: Before compacting session events, persist Merkle root in account state. Participants store individual proofs for future verification.

**DKD Commitment Root Persistence:**

```rust
pub struct DkdCommitmentRoot {
    pub session_id: Uuid,
    pub context_id: ContextId,
    pub merkle_root: [u8; 32],
    pub participant_count: usize,
    pub finalized_at_epoch: u64,
}

pub struct DeviceMetadata {
    pub device_id: DeviceId,
    pub dkd_proofs: HashMap<Uuid, MerkleProof>,  // session_id -> proof
    // ... other fields
}

pub struct MerkleProof {
    pub leaf_hash: [u8; 32],        // blake3(participant_id || commitment)
    pub sibling_path: Vec<[u8; 32]>,  // Path from leaf to root
    pub leaf_index: usize,
}
```

**Session Finalization with Root:**

When DKD session completes:
1. Build Merkle tree from all commitments (sorted by participant_id for determinism)
2. Emit `DkdSessionFinalized { session_id, merkle_root, participant_proofs }`
3. Each participant extracts and stores their proof in `DeviceMetadata`
4. `DkdCommitmentRoot` written to `AccountState` (persists after compaction)

**Recovery Verification After Compaction:**

Guardian submits recovery approval with proof:
```rust
pub struct GuardianApproval {
    pub guardian_id: GuardianId,
    pub encrypted_share: Vec<u8>,
    pub merkle_proof: MerkleProof,  // Proof of original DKD commitment
    pub signature: Vec<u8>,
}

impl RecoveryOrchestrator {
    fn verify_guardian_share(
        &self,
        approval: &GuardianApproval,
        commitment_root: &DkdCommitmentRoot,
    ) -> Result<()> {
        // Decrypt share
        let share = self.decrypt_guardian_share(&approval.encrypted_share)?;
        
        // Recompute commitment
        let recomputed_commitment = blake3::hash(&self.compute_partial_point(&share));
        
        // Verify against Merkle root (not deleted commitment events)
        let leaf_hash = blake3::hash(&[
            approval.guardian_id.as_bytes(),
            &recomputed_commitment,
        ].concat());
        
        if !approval.merkle_proof.verify(leaf_hash, commitment_root.merkle_root) {
            return Err(AgentError::GuardianProofVerificationFailed);
        }
        
        Ok(())
    }
}
```

**Quorum-Authorized Compaction:**

**Problem**: Unauthorized compaction allows malicious device to prune data honest replicas still need.

**Solution - Threshold-Signed Compaction:**

```rust
pub struct CompactionProposal {
    pub proposal_id: Uuid,
    pub before_epoch: u64,
    pub sessions_to_compact: Vec<Uuid>,
    pub preserved_roots: Vec<DkdCommitmentRoot>,
    pub proposer: DeviceId,
    pub proposed_at_epoch: u64,
}

pub struct CompactionAck {
    pub proposal_id: Uuid,
    pub device_id: DeviceId,
    pub has_all_proofs: bool,  // Can this device verify all preserved roots?
    pub signature: Vec<u8>,
}

pub struct CompactionCommit {
    pub proposal_id: Uuid,
    pub before_epoch: u64,
    pub threshold_signature: Vec<u8>,  // M-of-N devices agreed
    pub ack_count: usize,
}
```

**Two-Phase Compaction Protocol:**

1. **Proposal Phase**:
   - Proposer emits `CompactionProposal` with sessions to prune and roots to preserve
   - Includes `preserved_roots` listing all Merkle roots that remain in `AccountState`
   - Device-signed with valid presence ticket

2. **Acknowledgement Phase**:
   - Each device verifies:
     - All `sessions_to_compact` are finalized
     - Device has Merkle proofs for all roots it cares about (its own commitments)
     - No active sessions would be affected
   - Devices emit `CompactionAck` with `has_all_proofs = true` if ready
   - If any device lacks proofs, it emits `CompactionDenied { proposal_id, missing_proofs }`

3. **Commit Phase**:
   - After threshold acks received, proposer emits `CompactionCommit` with threshold signature
   - All participants verify threshold signature before pruning
   - Participants prune events with `epoch_at_write < before_epoch`
   - Preserved `DkdCommitmentRoot` entries remain in `AccountState`

**Compaction Safety Rules:**

- **Proof Requirement**: Compaction only proceeds if all active participants confirm they have necessary proofs
- **Guardian Proofs**: Guardians must store proofs for all sessions they participated in (for future recovery)
- **Active Session Protection**: Sessions in `Pending`, `Active`, or `CooldownActive` states exempt from compaction
- **Root Persistence**: `DkdCommitmentRoot` entries never compacted; they're permanent account state
- **Threshold Authorization**: Requires M-of-N device signatures to prevent unilateral pruning

**Implementation Notes:**

- Compaction window: typically 1000+ epochs old and finalized
- Proof storage: lightweight (log₂(N) sibling hashes per participant)
- Recovery cost: Guardian submits share + proof; verifier checks against root (O(log N) verification)
- Denial handling: If compaction denied, proposer retries after participants sync proofs

### Distributed Locking for Concurrent Operations

**Problem**: Concurrent conflicting operations (e.g., two resharing sessions) lead to undefined behavior.

**Solution - Threshold-Granted Distributed Lock:**

**Core Principle**: Lock acquisition requires threshold agreement, not just CRDT merge order. Prevents races under eventual consistency.

```rust
pub struct OperationLock {
    pub operation_type: OperationType,  // DKD, Resharing, Recovery
    pub holder: DeviceId,
    pub session_id: Uuid,
    pub acquired_at_epoch: u64,
    pub ttl_in_epochs: u64,
}
```

**Three-Phase Lock Protocol:**

**Phase 1: Request**

**Problem with Simple Ordering**: Using "lowest device_id wins" causes starvation—devices with high IDs never acquire lock if low-ID devices are more active.

**Solution - Hash-Based Lottery for Winner Selection:**

Each device computes a deterministic "ticket" derived from current ledger state:

```rust
pub struct RequestOperationLock {
    pub request_id: Uuid,
    pub operation_type: OperationType,
    pub device_id: DeviceId,
    pub ticket: [u8; 32],  // blake3(device_id || last_event_hash)
    pub last_event_hash: [u8; 32],  // For verification
    pub requested_at_epoch: u64,
    pub signature: Vec<u8>,  // Device-signed with presence ticket
}
```

Device emits `RequestOperationLock` event with computed ticket.

**Phase 2: Grant (Threshold-Signed)**

**Problem**: CRDT eventual consistency allows multiple devices to believe they won the lottery based on stale local views. Need explicit quorum agreement.

**Solution - Threshold-Granted Lock:**

After requests merge into CRDT, quorum devices co-sign grant:

```rust
pub struct GrantOperationLock {
    pub request_id: Uuid,
    pub operation_type: OperationType,
    pub winner: DeviceId,
    pub session_id: Uuid,  // Newly created session
    pub granted_at_epoch: u64,
    pub threshold_signature: Vec<u8>,  // M-of-N devices agreed
    pub signers: Vec<DeviceId>,
}
```

**Grant Protocol:**
1. After seeing `RequestOperationLock` events, each device independently computes winner (highest ticket)
2. Devices emit `LockGrantProposal { request_id, winner, partial_signature }`
3. Once M-of-N partial signatures collected, aggregator publishes `GrantOperationLock` with threshold signature
4. All participants verify threshold signature before recognizing grant

**Phase 3: Operation Execution**

Winner observes threshold-signed `GrantOperationLock`:
- Verifies threshold signature
- Confirms `winner == self.device_id`
- Proceeds with critical operation (DKD, resharing, etc.)
- Other devices see grant and know they lost; they abandon their requests

**Phase 4: Release**

```rust
pub struct ReleaseOperationLock {
    pub session_id: Uuid,
    pub released_at_epoch: u64,
    pub signature: Vec<u8>,
}
```

Winner emits `ReleaseOperationLock` after operation completes or TTL expires.

**Race Prevention Properties:**

- **Threshold Agreement**: No device proceeds without M-of-N confirmation
- **Determinism**: Given identical CRDT view, all devices agree on winner
- **Fairness**: Hash-based lottery prevents starvation
- **Safety**: Threshold signature prevents unilateral lock claims
- **Liveness**: If winner crashes, TTL ensures lock eventually released

**Implementation:**

```rust
impl AccountLedger {
    pub fn determine_lock_winner(&self, requests: &[RequestOperationLock]) -> DeviceId {
        // Verify all requests computed tickets from same last_event_hash
        let canonical_hash = self.get_last_event_hash();
        
        requests.iter()
            .filter(|req| {
                req.last_event_hash == canonical_hash &&
                blake3::hash(&[req.device_id.as_bytes(), &canonical_hash].concat()) == req.ticket
            })
            .max_by_key(|req| req.ticket)
            .map(|req| req.device_id)
            .expect("At least one valid request")
    }
    
    pub fn verify_grant(&self, grant: &GrantOperationLock) -> Result<()> {
        // Verify threshold signature from M-of-N devices
        let message = [
            grant.request_id.as_bytes(),
            grant.operation_type.as_bytes(),
            grant.winner.as_bytes(),
            &grant.granted_at_epoch.to_le_bytes(),
        ].concat();
        
        if !self.verify_threshold_signature(&message, &grant.threshold_signature) {
            return Err(LedgerError::InvalidThresholdSignature);
        }
        
        Ok(())
    }
}
```

**Edge Cases:**

- **Concurrent Requests**: Multiple devices request simultaneously. Lottery + threshold grant ensures single winner.
- **Grant Timeout**: If threshold grant not achieved within timeout (e.g., 50 epochs), devices re-request with new lottery.
- **Stale Grant**: Device observes grant but TTL expired. Must re-request; old grant ignored.
- **Byzantine Proposal**: Invalid partial signature → rejected before aggregation.

**Authentication**: All phases require valid signatures (device signatures for requests, threshold signatures for grants) to prevent DoS and unauthorized lock claims.

### Declarative State Machine Orchestration

**Problem**: Hand-written protocol logic in agents is tightly coupled and error-prone.

**Solution - Pure State Machine Runner:**

Define protocols as declarative enum state machines. Create generic `ProtocolRunner` as pure function:
```rust
fn step(state: ProtocolState, event: Event) -> (ProtocolState, Vec<Action>)
```

Agent isolates side effects (CRDT reads/writes), keeping protocol logic pure and testable. Benefits: explicit state transitions, easy testing, protocol variations simple to add.

---

## Part 4: CRDT Operations & Transport Hooks

### Extending CrdtOperation for P2P Protocols

The P2P protocols are expressed as `CrdtOperation` variants that the agent submits explicitly:

**Basic Operations:**
- `FetchState`, `ProposeEvent`, `MergeState`

**P2P DKD Operations:**
- `InitiateDkdSession`, `RecordDkdCommitment`, `RecordDkdReveal`, `RecordDkdResult`

**P2P Resharing Operations:**
- `InitiateResharing`, `RecordResharingSubShare`, `MarkResharingShareReady`, `RecordResharingVerification`, `CompleteResharing`

**Recovery Operations:**
- `InitiateRecoverySession`, `RecordRecoveryApproval`, `VetoRecovery`, `CancelRecovery`, `CompleteRecovery`

Each operation type is wrapped in explicit request/response structs. There is no code generation layer; instead:
- `aura_transport` exposes helpers such as `broadcast_commitment` and `fetch_session_state` that marshall the required data.
- Operations that need batching (e.g., commitment fan-out) rely on manual aggregation before emitting ledger events.

Future work may explore automatic protocol derivation from declarative specifications, but Phase 1 execution assumes hand-written transport glue.

---

## Part 4: Three-Layer Architecture Mapping

### P2P DKD Across Layers

The P2P DKD protocol naturally maps to the three-layer architecture:

```
┌─────────────────────────────────────────────────────────────┐
│ Layer 3: Application API                                    │
│                                                             │
│  agent.derive_context_identity_threshold(                   │
│      &capsule,                                              │
│      DerivationParticipants {                               │
│          devices,                                           │
│          threshold: 2,                                      │
│      },                                                     │
│  )                                                          │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 2: Orchestration (P2P Protocol Coordination)          │
│                                                             │
│  DKD Protocol Phases:                                       │
│  ├─ Phase 0: Session Initiation                             │
│  │   └─ CRDT: InitiateDkdSession                            │
│  │                                                          │
│  ├─ Phase 1: Commitment Phase                               │
│  │   ├─ Compute H_i·G (local crypto)                        │
│  │   ├─ Hash point (local crypto)                           │
│  │   ├─ CRDT: RecordDkdCommitment                           │
│  │   └─ Wait for threshold (CRDT sync)                      │
│  │                                                          │
│  ├─ Phase 2: Reveal Phase                                   │
│  │   ├─ CRDT: RecordDkdReveal                               │
│  │   ├─ Verify reveals match commitments (Byzantine)        │
│  │   └─ Wait for threshold (CRDT sync)                      │
│  │                                                          │
│  ├─ Phase 3: Aggregation                                    │
│  │   ├─ Sum revealed points (local crypto)                  │
│  │   ├─ Clear cofactor (local crypto)                       │
│  │   ├─ Map to seed (local crypto)                          │
│  │   └─ CRDT: RecordDkdResult                               │
│  │                                                          │
│  └─ Phase 4: Identity Expansion                             │
│      └─ HKDF expansion (local crypto)                       │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 1: Execution (Primitive Operations)                   │
│                                                             │
│  Cryptographic Primitives:                                  │
│  ├─ BLAKE3(share_i || context_id) → H_i                     │
│  ├─ H_i · G → Point                                         │
│  ├─ BLAKE3(Point) → Commitment                              │
│  ├─ Point + Point → Aggregated                              │
│  ├─ [8] · Point → Cleared                                   │
│  └─ HKDF(seed, context) → Key                               │
│                                                             │
│  CRDT Operations:                                           │
│  ├─ ledger.initiate_dkd_session()                           │
│  ├─ ledger.record_dkd_commitment()                          │
│  ├─ ledger.record_dkd_reveal()                              │
│  ├─ ledger.record_dkd_result()                              │
│  └─ ledger.get_dkd_session()                                │
│                                                             │
│  Transport Operations (via Transport trait):                │
│  ├─ transport.broadcast(session_initiation)                 │
│  ├─ transport.sync_crdt_state()                             │
│  └─ transport.fetch_session_state()                         │
└─────────────────────────────────────────────────────────────┘
```

**Note on Transport Abstraction**: Layer 1 transport operations use the `Transport` trait interface, not a hardcoded protocol. The actual network layer (Noise XX, libp2p, WebRTC, etc.) is injected at runtime. See Part 5 for transport abstraction details.

### P2P Resharing Across Layers

Similarly, resharing maps to the three layers:

```
┌─────────────────────────────────────────────────────────────┐
│ Layer 3: Application API                                    │
│                                                             │
│  agent.add_device(new_device).await?                        │
│  // or                                                      │
│  agent.remove_device(lost_device).await?                    │
│  // or                                                      │
│  agent.adjust_threshold(new_threshold).await?               │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 2: Orchestration (Resharing Protocol)                 │
│                                                             │ 
│  Resharing Protocol Phases:                                 │
│  ├─ Phase 0: Initiation                                     │
│  │   ├─ Collect threshold signatures on proposal            │
│  │   └─ CRDT: InitiateResharing                             │ 
│  │                                                          │
│  ├─ Phase 1: Sub-share Distribution                         │
│  │   ├─ Generate Shamir polynomial (crypto)                 │
│  │   ├─ Evaluate at new participant IDs (crypto)            │
│  │   ├─ Encrypt for recipients (HPKE)                       │
│  │   └─ CRDT: RecordResharingSubShare                       │
│  │                                                          │
│  ├─ Phase 2: Share Reconstruction                           │
│  │   ├─ Collect sub-shares from CRDT                        │
│  │   ├─ Decrypt with device secret (HPKE)                   │
│  │   ├─ Lagrange interpolation (crypto)                     │
│  │   └─ CRDT: MarkResharingShareReady                       │
│  │                                                          │
│  ├─ Phase 3: Verification                                   │
│  │   ├─ Coordinate test signature (FROST)                   │
│  │   ├─ Verify against group PK                             │
│  │   └─ CRDT: RecordResharingVerification                   │
│  │                                                          │
│  └─ Phase 4: Commit                                         │
│      ├─ Bump session epoch                                 │
│      ├─ Apply Event::ResharingComplete                     │
│      └─ Invalidate old shares                              │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 1: Execution                                          │
│                                                             │
│  Cryptographic Primitives:                                  │
│  ├─ Polynomial::from_secret(share, threshold)               │
│  ├─ polynomial.evaluate(participant_id)                     │
│  ├─ Lagrange::interpolate(sub_shares, target_id)            │
│  ├─ HPKE::encrypt(sub_share, recipient_pk)                  │
│  └─ HPKE::decrypt(encrypted, device_secret)                 │
│                                                             │
│  CRDT Operations:                                           │
│  ├─ ledger.initiate_resharing()                             │
│  ├─ ledger.record_resharing_sub_share()                     │
│  ├─ ledger.mark_resharing_share_ready()                     │
│  ├─ ledger.record_resharing_verification()                  │
│  └─ ledger.complete_resharing()                             │
│                                                             │
│  Transport Operations (via Transport trait):                │
│  ├─ transport.broadcast(resharing_proposal)                 │
│  ├─ transport.sync_crdt_state()                             │
│  └─ transport.send_encrypted_sub_share()                    │
└─────────────────────────────────────────────────────────────┘
```

**Note on Transport Abstraction**: Layer 1 transport operations use the `Transport` trait, not a specific protocol. The diagrams show logical operations; actual network implementation (Noise XX, libp2p, WebRTC, etc.) is swappable via dependency injection. See Part 5 for full transport abstraction specification.

### Resharing Delivery Failure Handling

**Problem**: Resharing assumes every new participant receives all sub-shares; offline recipients block progress.

**Solution - Sub-Share Acknowledgement & Redelivery:**

**Phase 1 Enhancement: Per-Sub-Share ACKs**
- Each `RecordResharingSubShare` requires acknowledgement from recipient
- `SubShareAcknowledged { session_id, from_participant, to_participant }` event confirms receipt
- Senders track unacknowledged sub-shares

**Redelivery Mechanism:**
- Timeout per sub-share: `sub_share_timeout_in_epochs` (e.g., 50 epochs ≈ 5 minutes)
- If no ACK after timeout, sender retransmits via CRDT: `RetransmitSubShare { session_id, to_participant }`
- Maximum retries: 3 attempts, then mark participant as unreachable

**Automatic Rollback for Failed Resharing:**

**Problem**: Manual intervention required when resharing fails leaves system in partially transitioned state, requiring user to diagnose and retry.

**Solution - Atomic Resharing with Automatic Rollback:**

If threshold of new participants cannot be reached:
1. **Detect Failure**: Orchestrator detects insufficient participants after max retries
2. **Abort Session**: Emit `ResharingAborted { session_id, reason: "insufficient_participants" }`
3. **Discard Incomplete Shares**: All participants discard new (incomplete) shares from failed session
4. **Revert to Stable State**: Participants continue using previous share set (old_threshold, old_participants)
5. **Maintain Invariants**: Session epoch unchanged, presence tickets still valid, no manual cleanup needed

**Atomicity Guarantees:**
- Either all new participants complete share reconstruction OR all revert to old shares
- No intermediate state where some devices have new shares and others have old shares
- Session epoch only bumps on `ResharingCompleted`, never on `ResharingAborted`
- Verification phase (test signature) catches any inconsistencies before commit

**Recovery from Abort:**
- System returns to known-good state automatically
- User notified: "Resharing aborted: 2 of 4 devices offline. System stable, retry when devices online."
- Automatic retry can be scheduled with exponential backoff if desired
- No data loss, no manual intervention required

**CRDT Events:**
- `SubShareAcknowledged { session_id, from_participant, to_participant, received_at_epoch }`
- `SubShareRetransmitted { session_id, to_participant, attempt_number }`
- `ParticipantUnreachable { session_id, participant_id, failed_after_attempts }`
- `ResharingAborted { session_id, reason, aborted_at_epoch, old_state_preserved }`

**Implementation Notes:**
- Abort decision requires threshold agreement (M-of-N old participants must agree to abort)
- New participants that received incomplete shares mark them as invalid and ignore
- CRDT merge ensures all participants converge on abort decision
- Abort is final; new resharing session must be initiated for retry

---

## Part 5: Transport & Presence Tickets

### Transport Abstraction Design

**Design Principle**: Aura core protocols are transport-agnostic. The library provides a clean `Transport` trait that can be implemented for any underlying network layer.

**Default Implementation**: This project provides a single reference implementation (Noise XX over HTTPS relay), but library users can swap in alternative transports (libp2p, WebRTC, Bluetooth, custom protocols) by implementing the trait.

**Transport Trait Interface:**

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    /// Establish connection to a peer with mutual authentication
    async fn connect(&self, peer_id: &PeerId, ticket: &PresenceTicket) -> Result<Connection>;
    
    /// Send message to connected peer
    async fn send(&self, conn: &Connection, message: &[u8]) -> Result<()>;
    
    /// Receive message from peer with timeout
    async fn receive(&self, conn: &Connection, timeout: Duration) -> Result<Vec<u8>>;
    
    /// Broadcast message to multiple peers (best-effort)
    async fn broadcast(&self, peer_ids: &[PeerId], message: &[u8]) -> Result<BroadcastResult>;
    
    /// Close connection gracefully
    async fn disconnect(&self, conn: &Connection) -> Result<()>;
    
    /// Check if connection is still valid
    async fn is_connected(&self, conn: &Connection) -> bool;
}

pub struct Connection {
    pub peer_id: PeerId,
    pub session_epoch: u64,
    pub established_at: u64,
    // Transport-specific state is opaque to core protocols
    inner: Box<dyn Any + Send + Sync>,
}
```

**Protocol Layer Requirements:**

The core protocols (DKD, Resharing, Recovery) only depend on:
- `Transport::connect()` - establish authenticated channel
- `Transport::send()` / `receive()` - message passing
- `Transport::broadcast()` - fan-out for coordination

They do NOT depend on:
- Specific transport protocol (Noise, TLS, libp2p, WebRTC)
- Network topology (relay, mesh, star)
- Connection management strategy (persistent, ephemeral, pooled)

**Adapter Pattern:**

```rust
// Default implementation
pub struct NoiseHttpsTransport {
    relay_url: String,
    // ... Noise-specific state
}

// Alternative implementation (user-provided)
pub struct Libp2pTransport {
    swarm: Libp2p::Swarm,
    // ... libp2p-specific state
}

// Both implement Transport trait
impl Transport for NoiseHttpsTransport { /* ... */ }
impl Transport for Libp2pTransport { /* ... */ }

// Core protocols work with either
pub struct DeviceAgent {
    transport: Arc<dyn Transport>,  // Injected, not hardcoded
    // ...
}
```

**Transport Selection:**

```rust
// Default transport (provided by aura-transport crate)
let transport = NoiseHttpsTransport::new("https://relay.example.com");
let agent = DeviceAgent::new(Arc::new(transport), ...);

// Custom transport (user-provided)
let custom_transport = MyCustomTransport::new(...);
let agent = DeviceAgent::new(Arc::new(custom_transport), ...);
```

### Default Transport: Noise XX over HTTPS Relay

**What follows is the specification for the default transport implementation. Library users implementing alternative transports should provide equivalent security guarantees.**

### Transport Handshake Specification

**Problem**: Handshake transcript, signed fields, and revocation semantics underspecified.

**Solution - Detailed Handshake Protocol (for default Noise XX implementation):**

**Phase 1: Connection Establishment (TLS/Noise)**
1. Client initiates Noise XX handshake
2. Server authenticates with certificate (relay) or static key (P2P)
3. Encrypted channel established

**Phase 2: Presence Ticket Exchange**
1. Client sends `PresenceTicket { device_id, account_id, session_epoch, issued_at, expires_at, signature }`
2. Server verifies:
   - Signature valid against account's group public key
   - `session_epoch` matches server's view (from CRDT)
   - `expires_at > current_time`
   - Device not revoked (check CRDT for `DeviceRevoked` event)
3. Server responds with own presence ticket
4. Both parties bind session to `(device_id, session_epoch)` tuple

**Handshake Transcript Binding:**
- Final handshake message includes `blake3(handshake_hash || presence_ticket)` as additional data
- Prevents ticket replay in different TLS sessions

**Epoch Mismatch Handling:**
- If `presence_ticket.session_epoch < server_current_epoch`: graceful close with `EpochMismatch` error
- Client must sync CRDT, get updated epoch, regenerate presence ticket
- If `presence_ticket.session_epoch > server_current_epoch`: server syncs CRDT first, then re-verifies

**Revocation Propagation:**
- `DeviceRevoked { device_id, revoked_at_epoch }` event in CRDT
- Active connections check revocation status every N seconds (e.g., 60s)
- On revocation detected: hard drop connection, no graceful close
- Revoked device attempting connection: reject during ticket verification

**Failure Semantics:**
- **Graceful Close**: Invalid ticket format, expired ticket, epoch mismatch → send error message, allow retry
- **Hard Drop**: Revoked device, Byzantine behavior detected, signature verification failure → immediate disconnect, temporary IP ban

### Presence Ticket Structure

```rust
pub struct PresenceTicket {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub session_epoch: u64,
    pub issued_at: u64,
    pub expires_at: u64,  // TTL typically 1 hour
    pub capabilities: Vec<Capability>,  // Optional: what operations this device can perform
    pub signature: Vec<u8>,  // Threshold signature over (device_id || account_id || session_epoch || issued_at || expires_at)
}
```

**Ticket Issuance:**
- Generated during session establishment or epoch bump
- Threshold-signed by M-of-N devices
- Short TTL forces regular re-signing (liveness proof)

---

## Part 6: Unified Error Handling

### Integrated Error Types

Unified `AgentError` hierarchy:

**General Errors:**
- `OrchestratorError`, `LedgerError`, `InvalidContext`, `SerializationError`, `CryptoError`

**P2P DKD Errors:**
- `DkdSessionNotFound`, `DkdMissingCommitment`, `DkdCommitmentMismatch` (Byzantine), `DkdInvalidPoint`, `DkdInsufficientParticipants`, `DkdTimeout`

**P2P Resharing Errors:**
- `ResharingSessionNotFound`, `ResharingNotAuthorized`, `ResharingInsufficientSubShares`, `ResharingReconstructionFailed`, `ResharingVerificationFailed`, `ResharingTimeout`, `ResharingProposalNotSigned`

**Recovery Errors:**
- `RecoverySessionNotFound`, `RecoveryNotAuthorized`, `RecoveryNotReady`, `RecoveryAlreadyComplete`, `RecoveryTerminated`, `RecoveryCannotVeto`, `RecoveryInsufficientApprovals`, `GuardianEnvelopeDecryptionFailed`, `RecoveryTimeout`

**Presence/Transport Errors:**
- `UnsupportedRoute`, `DeviceNotFound`, `EpochMismatch`

### Error Propagation Pattern

Errors propagate naturally through the three layers:

**Layer 3 (Application)**: User-facing errors with recovery actions
```rust
match agent.derive_context_identity(&capsule, location).await {
    Err(AgentError::DkdCommitmentMismatch(pid)) => {
        // Byzantine participant detected, exclude and retry
    }
    Err(AgentError::DkdTimeout(seconds)) => {
        // Timeout, retry with different participants
    }
    // ... handle other errors
}
```

**Layer 2 (Orchestration)**: Context-rich errors from protocol phases
- Timeout detection, threshold checking, state validation

**Layer 3 (Execution)**: Primitive-level errors
- Cryptographic failures, CRDT errors, transport failures

---

## Part 6: Implementation Checklist (Prototype Scope)

Focus areas to bring the integration live:

- **Cryptography**
  - [ ] Harden commitment → reveal → aggregation helpers in `crates/crypto`
  - [ ] Add adversarial tests (commitment mismatch, malformed share, stale epoch)

- **Ledger integration**
  - [ ] Ensure `DkdSessionState`, `ResharingSessionState`, and recovery events are stored in `aura_ledger`
  - [ ] Provide helper APIs on `AccountLedger` for initiating sessions, recording approvals, and marking completion

- **Agent orchestration**
  - [ ] Finish wiring DKD flows in the agent module
  - [ ] Complete recovery flow, including commitment verification and resharing hand-off
  - [ ] Surface CLI-friendly commands (`derive-threshold`, `recovery-*`)

- **Transport**
  - [ ] Implement HTTPS relay adapter capable of broadcasting ledger entries and fetching session state
  - [ ] Enforce presence-ticket verification and simple retry/backoff loops

- **End-to-end validation**
  - [ ] Scenario: derive context identity with 2-of-3 devices
  - [ ] Scenario: add device via resharing and confirm session epoch bump
  - [ ] Scenario: guardian recovery (initiate → approvals → cooldown → complete)
  - [ ] Scenario: storage write/read using the recovered device

---

## Part 7: Testing Strategy

### Testing Across Layers

Each layer has distinct testing requirements:

#### Layer 1: Primitive Tests

Test individual primitives in isolation:
- **Crypto**: Commitment-reveal binding, point arithmetic, Lagrange interpolation
- **CRDT**: State transitions, event recording, session management
- **Transport**: Send/receive, connection management, protocol execution

#### Layer 2: Orchestration Tests

Test protocol phases and state transitions:

**DKD Protocol:**
- Initiation → commitment phase → reveal phase → aggregation
- Threshold checking, timeout handling, Byzantine detection

**Resharing Protocol:**
- Add device: 2-of-3 → 2-of-4
- Remove device: 2-of-3 → 2-of-2
- Adjust threshold: 2-of-3 → 3-of-3
- Verification with test signatures

**Recovery Protocol:**
- Initiate → approvals → cooldown → execute
- Veto and cancellation during cooldown
- Share reconstruction and device addition

#### Layer 3: API Tests

Test high-level application APIs:

**DKD API:**
- `derive_context_identity()` with threshold location
- Determinism verification (same inputs → same identity)
- Context isolation (different contexts → different identities)

**Resharing API:**
- `add_device()`, `remove_device()`, `adjust_threshold()`
- Verify new participants can sign
- Verify forward secrecy (old shares invalidated)

**Recovery API:**
- Full workflow: initiate → approvals → cooldown → complete
- Guardian veto during cooldown
- User cancellation during cooldown
- Cooldown enforcement (cannot complete early)
- Session epoch bump verification

### Negative Test Scenarios (Critical for MVP)

**Problem**: Happy-path testing insufficient; negative scenarios expose latent bugs in timeout, error, and Byzantine paths.

**Solution - Comprehensive Negative Testing:**

**DKD Protocol Failures:**
- **Session Timeout**: Initiate DKD, only N-1 devices respond, verify timeout triggers and session moves to `TimedOut` state
- **Byzantine Commitment**: Submit commitment, then reveal mismatched point, verify `ParticipantBlamed` emitted and session aborted
- **Duplicate Commitment**: Same participant submits two commitments, verify second rejected with proof
- **Reveal Without Commitment**: Participant reveals without prior commitment, verify rejection
- **Insufficient Participants**: Only M-1 of M required participants online, verify graceful failure

**Resharing Protocol Failures:**
- **Offline Recipient**: New device offline during sub-share distribution, verify redelivery mechanism kicks in
- **Threshold Unreachable**: Too many new participants offline, verify session fails with operator guidance
- **Verification Failure**: New share produces invalid test signature, verify resharing aborted
- **Concurrent Resharing**: Two devices attempt resharing simultaneously, verify distributed lock prevents both

**Recovery Protocol Failures:**
- **Guardian Veto**: Guardian vetoes during cooldown, verify recovery terminated and audit logged
- **Duplicate Recovery Request**: User initiates second recovery while first active, verify rejection
- **Cooldown Bypass Attempt**: Try to complete recovery before cooldown elapsed, verify rejection
- **Guardian Approval Replay**: Guardian reuses old approval ciphertext in new recovery, verify replay detection
- **Insufficient Approvals**: Only M-1 guardians approve (need M), verify recovery stuck in cooldown

**Transport Failures:**
- **Revoked Ticket Mid-Sync**: Device's presence ticket revoked while sync in progress, verify hard drop
- **Epoch Mismatch**: Device with old epoch attempts connection, verify graceful close with sync instruction
- **Expired Ticket**: Device presents expired presence ticket, verify graceful rejection
- **Invalid Signature**: Presence ticket with invalid threshold signature, verify immediate disconnect

**CRDT Consistency:**
- **Concurrent Conflicting Operations**: Concurrent DKD and resharing initiation, verify lock prevents conflict
- **Network Partition**: Split devices into two groups, each runs protocol, verify convergence on merge
- **Compaction During Active Session**: Attempt compaction while session still active, verify rejection

**CI Integration:**
- All negative scenarios run in CI on every commit
- Test matrix: [DKD, Resharing, Recovery] × [Timeout, Byzantine, Offline, Concurrent]
- Failure budget: 0 flaky tests, all negative paths must deterministically pass

---

## Part 8: Security Considerations

### Unified Threat Model

Combining security considerations from both documents:

| Threat | Affected Component | Mitigation |
|--------|-------------------|------------|
| **Malicious Participant (Byzantine)** | P2P DKD, P2P Resharing | Commitment-reveal scheme, verification phase, threshold requirement |
| **Network MITM** | Transport | HPKE encryption, TLS/Noise channels, presence tickets |
| **Compromised Old Share** | P2P Resharing | Session epoch bump, forward secrecy, guardian envelope rotation |
| **Denial of Service** | All P2P protocols | Only threshold required (not all), timeout handling, fallback participants |
| **Replay Attack** | CRDT operations | Session IDs, nonces, monotonic session epoch, event parent hashing |
| **CRDT Poisoning** | Ledger | Threshold-signed events, audit log, Byzantine detection |
| **Single Point of Failure** | Architecture | No coordinator, peer-to-peer, CRDT-based coordination |
| **Location-Based Attacks** | Transport + presence enforcement | Presence tickets, location verification, capability tokens |
| **Unauthorized Recovery** | Recovery | Guardian threshold, cooldown period, veto mechanism |
| **Recovery Replay** | Recovery | Session IDs, timestamps, signatures, nonces |
| **Guardian Collusion** | Recovery | Cooldown allows account owner to detect and cancel, audit trail |
| **Cooldown Bypass** | Recovery | Cooldown enforced at CRDT level, cannot be circumvented |
| **CRDT Metadata Leakage** | CRDT Ledger | Protocol metadata visible to all participants; content encrypted but event patterns observable |

### CRDT Metadata Leakage Threat

**Problem**: While share contents and sensitive data are encrypted, the CRDT ledger is replicated among all account participants, making protocol metadata visible.

**What Can Be Observed:**

An observer of the CRDT log (semi-trusted relay, former guardian with stale ledger copy, revoked device) can infer:
- **User Activity Patterns**: Flurry of `GuardianNudged` events followed by `RecoveryInitiated` reveals user struggling to access account
- **Device Usage**: `DkdSessionInit` participant lists show which devices are being used together for specific contexts
- **Guardian Relationships**: Recovery events reveal guardian identities and response patterns
- **Protocol Timing**: Event timestamps reveal when protocols run, potentially correlating with user behavior
- **Threshold Configuration**: Session parameters (threshold, participant count) visible in events
- **Failure Patterns**: `ParticipantBlamed`, `SessionAborted` events reveal system health and reliability issues

**Current Mitigations:**

1. **Content Encryption**: All sensitive data (shares, keys, personal data) is HPKE-encrypted
2. **Participant-Only Access**: Only account participants have CRDT access; external observers cannot read ledger
3. **Minimal Metadata**: Events include only protocol-necessary fields, no extraneous user data
4. **Audit Trail**: Visibility is feature, not bug; enables accountability and Byzantine detection

**Acknowledged Limitations:**

- Protocol metadata IS visible to all account participants (devices, guardians)
- Event patterns can reveal behavioral information
- Former participants retain historical view until compaction
- Semi-trusted relay operators could observe event timing/frequency

**Future Hardening (Phase 2+):**

1. **Padding/Dummy Events**: Add random padding events to obfuscate real protocol activity
2. **Batched Operations**: Bundle multiple operations to hide individual event timing
3. **Anonymous Credentials**: Use cryptographic techniques to hide participant lists in transactions (high complexity)
4. **Event Encryption**: Encrypt event payloads with shared participant key (requires key management)
5. **Differential Privacy**: Add noise to timing/frequency of events
6. **Selective Disclosure**: Allow participants to see only events relevant to their operations (breaks CRDT convergence guarantees)

**Recommendation for Library Users:**

Document this threat in your system's threat model. Users should understand that:
- **Content is private** (shares, keys, personal data are encrypted)
- **Protocol metadata is visible** to all account participants
- Participants include: all devices, all guardians, any semi-trusted infrastructure
- This is fundamental to transparent, decentralized coordination
- Trade-off: metadata visibility enables auditability and Byzantine detection

### Security Properties Checklist

- [ ] **P2P DKD Security**
  - [ ] Threshold requirement enforced (M-of-N)
  - [ ] Determinism verified (same inputs → same output)
  - [ ] Context isolation (different contexts → different identities)
  - [ ] Binding (commitment prevents changing contribution)
  - [ ] Hiding (commitment hides contribution until reveal)
  - [ ] Byzantine fault tolerance (commitment verification)
  - [ ] Forward secrecy (resharing invalidates old shares)

- [ ] **P2P Resharing Security**
  - [ ] Authorization (threshold-signed proposal required)
  - [ ] Confidentiality (sub-shares HPKE encrypted)
  - [ ] Integrity (verification phase detects invalid shares)
  - [ ] Forward secrecy (session epoch bump)
  - [ ] Byzantine fault tolerance (test signature)
  - [ ] Availability (only threshold needed)
  - [ ] Group key preservation (constant group public key)

- [ ] **Transport & Presence Security**
  - [ ] Location transparency (guard against rogue relays / replay)
  - [ ] Presence-ticket validation on every session
  - [ ] Consistent error propagation (transport ↔ agent)
  - [ ] Audit logging for transport-level failures

- [ ] **Recovery Protocol Security**
  - [ ] Guardian authorization (only authorized guardians can approve)
  - [ ] Threshold enforcement (M-of-N guardians required)
  - [ ] Cooldown enforcement (mandatory wait period)
  - [ ] Veto capability (any guardian can veto during cooldown)
  - [ ] Cancellation capability (account owner can cancel during cooldown)
  - [ ] Share confidentiality (HPKE encryption for shares)
  - [ ] Forward secrecy (guardian shares invalidated after recovery)
  - [ ] Audit trail (all recovery events logged immutably)
  - [ ] Replay protection (session IDs, signatures, timestamps)

---

## Part 9: Future Enhancements

### Phase 2+ Extensions

Once Phase 1 is complete with integrated P2P protocols and unified architecture, these enhancements become natural:

#### 1. Content-Addressed Architecture Enhancement
Systematic content addressing for P2P protocol artifacts (sessions, commitments, sub-shares). Benefits: deduplication, verifiable references, global caching.

#### 2. Proactive Resharing
Automatic periodic resharing (e.g., every 30 days) to refresh shares without changing participants. Provides forward secrecy against future compromise.

#### 3. Batch DKD
Derive multiple contexts in a single protocol run. Single commitment-reveal-aggregate phase for all capsules, amortizing protocol overhead.

#### 4. Zero-Knowledge DKD
Prove correct partial point generation without revealing share. ZK proof: "I computed H_i·G correctly without revealing share_i". Stronger Byzantine fault tolerance.

#### 5. Additional Transport Implementations
Library users can implement additional `Transport` trait adapters for different network layers:
- **BLE Mesh**: For local device-to-device communication
- **WebRTC**: For browser-based peer-to-peer
- **libp2p**: For decentralized P2P networks
- **Custom Protocols**: Enterprise users can wrap proprietary transports

All transport implementations reuse the same ledger-driven coordination; only the Layer 1 network primitives change. Core protocols remain unchanged.

---

## Part 10: CLI Integration

### Minimal CLI Surface

For the prototype we target a narrow CLI that exercises the full loop without promising richer session management:

```bash
# Threshold DKD (interactive demo)
$ aura derive-threshold --context <capsule.json> --threshold 2 --participants dev1,dev2,dev3

# Guardian recovery lifecycle
$ aura recovery initiate --request request.json
$ aura recovery approve --request-id <uuid>
$ aura recovery cancel  --request-id <uuid>
$ aura recovery complete --request-id <uuid>

# Status inspection (read-only view of CRDT state)
$ aura status --account <account-id>
```

Each command is a thin wrapper around the agent APIs described earlier; more advanced operations (automatic threshold adjustment, per-session inspection) remain future enhancements rather than MVP commitments.

---

## Conclusion

This specification provides a complete, production-ready architecture for Aura's decentralized identity system. It integrates:

1. **Three-Layer Architecture**
   - Clear separation: Application APIs → Orchestration → Execution
   - Conceptual framing for transport and ledger responsibilities

2. **Concrete Distributed Protocols**
   - P2P DKD: commitment/reveal/aggregation via ledger events
   - P2P Resharing: state machine for participant changes
   - Guardian Recovery: cooldown enforcement with share integrity checks

This unified design delivers:

[x] **Clear Implementation Path** – ledger-driven session states tie the protocols to concrete code  
[x] **Testability** – each layer (crypto, ledger, orchestration) exposes discrete hooks for testing  
[x] **Security** – commitment verification, epoch bumps, and presence tickets close the loop  
[x] **Recovery** – guardian flows reuse resharing without bespoke code paths  
[x] **Transport Abstraction** – core protocols depend only on `Transport` trait; this project provides Noise XX over HTTPS relay, but library users can inject libp2p, WebRTC, or custom implementations  
[x] **CLI Surface** – minimal commands exercise the end-to-end loop for demos  

**Critical Improvements Incorporated (from design feedback):**

[x] **Session Lifecycle Management** – Explicit states, timeouts, collision handling, Byzantine validation  
[x] **Logical Clock for Timeouts** – Self-contained epoch progression via CRDT writes and tick events; no wall-clock synchronization required  
[x] **CRDT Compaction with Merkle Proofs** – DKD commitment roots persist in account state; participants store individual proofs for post-compaction recovery verification  
[x] **Threshold-Authorized Compaction** – Two-phase propose/ack protocol with quorum signatures prevents unilateral pruning  
[x] **Threshold-Granted Distributed Locking** – Lock acquisition requires M-of-N agreement, not just CRDT merge order; prevents races under eventual consistency  
[x] **Declarative State Machines** – Pure protocol logic separated from side effects for testability  
[x] **Replay Protection** – Guardian approvals bind to request_id/guardian_id, nonce tracking  
[x] **Flexible Quorums** – Recovery supports N invited guardians, M required (N > M) for social redundancy  
[x] **Guardian Nudging** – In-band protocol events trigger UI notifications for unresponsive guardians  
[x] **Resharing Redelivery** – Sub-share acknowledgements and retry logic handle offline recipients  
[x] **Transport Handshake Spec** – Detailed presence ticket validation, epoch mismatch handling, revocation  
[x] **Negative Test Coverage** – Comprehensive failure scenarios (timeout, Byzantine, replay, concurrent) in CI  
[x] **Participant Redemption** – Blamed participants can rejoin via health check protocol after cooldown  
[x] **Automatic Rollback** – Failed resharing sessions revert to stable state atomically, no manual intervention  
[x] **Metadata Leakage Documentation** – Acknowledged CRDT metadata visibility threat with current mitigations and future hardening paths  

The checklist above replaces the older "7-week" proposal; teams can execute items incrementally as the private prototype evolves.

---

## References

**External Standards:**
- **FROST Threshold Signatures** - [IETF Draft](https://datatracker.ietf.org/doc/draft-irtf-cfrg-frost/)
