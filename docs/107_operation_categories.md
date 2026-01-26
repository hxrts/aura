# Operation Categories

This document defines the three-tier classification system for distributed operations in Aura. It specifies the ceremony contract for Category C operations, the consistency metadata types for each category, and the decision framework for categorizing new operations. The core insight is that not all operations require consensus. Many can proceed optimistically with background reconciliation.

## 1. Overview

Operations in Aura fall into three categories based on their effect timing and security requirements.

| Category | Name | Effect Timing | When Used |
|----------|------|---------------|-----------|
| A | Optimistic | Immediate local effect | Low-risk operations within established contexts |
| B | Deferred | Pending until confirmed | Medium-risk policy/membership changes |
| C | Consensus-Gated | Blocked until ceremony completes | Cryptographic context establishment |

Agreement modes are orthogonal to categories. Operations can use provisional or soft-safe fast paths, but any durable shared state must be consensus-finalized (A3). See [Consensus](106_consensus.md) for the fast-path and finalization taxonomy.

### 1.1 Key Generation Methods

Aura separates key generation from agreement:

| Code | Method | Description |
|------|--------|-------------|
| K1 | Single-signer | No DKG required. Local key generation. |
| K2 | Dealer-based DKG | Trusted coordinator distributes shares. |
| K3 | Consensus-finalized DKG | BFT-DKG with transcript commit. |
| DKD | Distributed key derivation | Multi-party derivation without DKG. |

### 1.2 Agreement Levels

| Code | Level | Description |
|------|-------|-------------|
| A1 | Provisional | Usable immediately but not final. |
| A2 | Coordinator Soft-Safe | Bounded divergence with convergence certificate. |
| A3 | Consensus-Finalized | Unique, durable, non-forkable. |

Fast paths (A1/A2) are provisional. Durable shared state must be finalized by A3.

### 1.3 The Key Architectural Insight

Ceremonies establish shared cryptographic context. Operations within that context are cheap.

```
Ceremony (Category C)                    Optimistic Operations (Category A)
─────────────────────                    ─────────────────────────────────
• Runs once per relationship             • Within established context
• Establishes ContextId + shared roots   • Derive keys from context
• Creates relational context journal     • Just emit CRDT facts
• All future encryption derives here     • No new agreement needed
```

## 2. Category A: Optimistic Operations

Category A operations have immediate local effect via CRDT fact emission. Background sync via anti-entropy propagates facts to peers. Failure shows a status indicator but does not block functionality. Partial success is acceptable.

### 2.1 Examples

| Operation | Immediate Action | Background Sync | On Failure |
|-----------|-----------------|-----------------|------------|
| Create channel | Show channel, enable messaging | Fact syncs to members | Show "unsynced" badge |
| Send message | Display in chat immediately | Delivery receipts | Show "undelivered" indicator |
| Add contact (within context) | Show in list | Mutual acknowledgment | Show "pending" status |
| Block contact | Hide from view immediately | Propagate to context | Already effective locally |
| Update profile | Show changes immediately | Propagate to contacts | Show sync indicator |
| React to message | Show reaction | Fact syncs | Show "pending" |

### 2.2 Implementation Pattern

```rust
async fn create_channel_optimistic(&mut self, config: ChannelConfig) -> ChannelId {
    let channel_id = ChannelId::derive(&config);

    self.emit_fact(ChatFact::ChannelCheckpoint {
        channel_id,
        epoch: 0,
        base_gen: 0,
        window: 1024,
    }).await;

    channel_id
}
```

This pattern emits a fact into the existing relational context journal. The channel is immediately usable. Key derivation uses `KDF(ContextRoot, ChannelId, epoch)`.

### 2.3 Why This Works

Category A operations work because encryption keys already exist (derived from established context), facts are CRDTs (eventual consistency is sufficient), no coordination is needed (shared state already agreed upon), and the worst case is delay rather than a security issue.

## 3. Category B: Deferred Operations

Category B operations have local effect pending until agreement is reached. The UI shows intent immediately with a "pending" indicator. Operations may require approval from capability holders. Automatic rollback occurs on rejection.

### 3.1 Examples

| Operation | Immediate Action | Agreement Required | On Rejection |
|-----------|-----------------|-------------------|--------------|
| Change channel permissions | Show "pending" | Admin approval | Revert, notify |
| Remove channel member | Show "pending removal" | Admin consensus | Keep member |
| Transfer ownership | Show "pending transfer" | Recipient acceptance | Cancel transfer |
| Rename channel | Show "pending rename" | Member acknowledgment | Keep old name |
| Archive channel | Show "pending archive" | Admin approval | Stay active |

### 3.2 Implementation Pattern

```rust
async fn change_permissions_deferred(
    &mut self,
    channel_id: ChannelId,
    changes: PermissionChanges,
) -> ProposalId {
    let proposal = Proposal {
        operation: Operation::ChangePermissions { channel_id, changes },
        requires_approval_from: vec![CapabilityRequirement::Role("admin")],
        threshold: ApprovalThreshold::Any,
        timeout_ms: 24 * 60 * 60 * 1000,
    };

    let proposal_id = self.emit_proposal(proposal).await;
    proposal_id
}
```

This pattern creates a proposal that does not apply the effect yet. The UI shows "pending" state. The effect applies when threshold approvals are received. Auto-revert occurs on timeout or rejection.

### 3.3 Approval Thresholds

```rust
pub enum ApprovalThreshold {
    Any,
    Unanimous,
    Threshold { required: u32 },
    Percentage { percent: u8 },
}
```

`Any` requires any single holder of the required capability. `Unanimous` requires all holders to approve. `Threshold` requires k-of-n approval. `Percentage` requires a percentage of holders.

## 4. Category C: Consensus-Gated Operations

Category C operations do NOT proceed until a ceremony completes. Partial state would be dangerous or irrecoverable. The user must wait for confirmation. These operations use choreographic protocols with session types.

### 4.1 Examples

| Operation | Why Blocking Required | Risk if Optimistic |
|-----------|----------------------|-------------------|
| Add contact (new relationship) | Creates cryptographic context | No shared keys possible |
| Create group | Multi-party key agreement | Inconsistent member views |
| Add member to group | Changes group keys | Forward secrecy violation |
| Device enrollment | Key shares distributed atomically | Partial enrollment unusable |
| Guardian rotation | Key shares distributed atomically | Partial rotation unusable |
| Recovery execution | Account state replacement | Partial recovery corruption |
| OTA hard fork | Breaking protocol change | Network split |
| Device revocation | Security-critical removal | Attacker acts first |

### 4.2 Implementation Pattern

```rust
async fn add_contact(&mut self, invitation: Invitation) -> Result<ContactId> {
    let ceremony_id = self.ceremony_executor
        .initiate_invitation_ceremony(invitation)
        .await?;

    loop {
        match self.ceremony_executor.get_status(&ceremony_id)? {
            CeremonyStatus::Committed => {
                return Ok(ContactId::from_ceremony(&ceremony_id));
            }
            CeremonyStatus::Aborted { reason } => {
                return Err(AuraError::ceremony_failed(reason));
            }
            _ => {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}
```

This pattern blocks until the ceremony completes. The user sees progress UI during execution. Context is established only on successful commit.

## 5. Ceremony Contract

All Category C ceremonies follow a shared contract that ensures atomic commit/abort semantics.

### 5.1 Ceremony Phases

1. **Compute prestate**: Derive a stable prestate hash from the authority/context state being modified. Include the current epoch and effective participant set.

2. **Propose operation**: Define the operation being performed. Compute an operation hash bound to the proposal parameters.

3. **Enter pending epoch**: Generate new key material at a pending epoch without invalidating the old epoch. Store metadata for commit or rollback.

4. **Collect responses**: Send invitations/requests to participants. Participants respond using their full runtimes. Responses must be authenticated and recorded as facts.

5. **Commit or abort**: If acceptance/threshold conditions are met, commit the pending epoch and emit resulting facts. Otherwise abort, emit an abort fact with a reason, and leave the prior epoch active.

### 5.2 Ceremony Properties

All Category C ceremonies implement:

1. **Prestate Binding**: `CeremonyId = H(prestate_hash, operation_hash, nonce)` prevents concurrent ceremonies on same state and ensures exactly-once semantics.

2. **Atomic Commit/Abort**: Either fully committed or no effect. No partial state possible.

3. **Epoch Isolation**: Uncommitted key packages are inert. No explicit rollback needed on abort.

4. **Session Types**: Protocol compliance enforced at compile time via choreographic projection.

### 5.3 Per-Ceremony Policy Matrix

#### Authority and Device Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| Authority bootstrap | K1 | A3 | None | Local, immediate |
| Device enrollment | K2 | A1→A2→A3 | A1/A2 | Provisional → soft-safe → finalize |
| Device MFA rotation | K3 | A2→A3 | A2 | Consensus-finalized keys |
| Device removal | K3 | A2→A3 | A2 | Remove via rotation |

#### Guardian Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| Guardian setup/rotation | K3 | A2→A3 | A2 | Consensus-finalized for durability |
| Recovery approval | — | A2→A3 | A2 | Soft-safe approvals → consensus |
| Recovery execution | — | A2→A3 | A2 | Consensus-finalized commit |

#### Channel and Group Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| AMP channel epoch bump | — | A1→A2→A3 | A1/A2 | Proposed → cert → commit |
| AMP channel bootstrap | — | A1→A2→A3 | A1/A2 | Provisional → group key rotation |
| Group/Block creation | K3 | A1→A2→A3 | A1/A2 | Provisional bootstrap → consensus |
| Rendezvous secure-channel | — | A1→A2→A3 | A1/A2 | Provisional → consensus |

#### Other Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| Invitation (contact/channel/guardian) | — | A3 | None | Consensus-finalized only |
| OTA activation | — | A2→A3 | A2 | Threshold-signed → consensus |
| DKD ceremony | DKD | A2→A3 | A2 | Multi-party derivation → commit |

### 5.4 Bootstrap Exception

When creating a new group/channel before the group key ceremony completes, Aura allows a bootstrap epoch using a trusted-dealer key (K2/A1). The dealer distributes a bootstrap key with the channel invite, enabling immediate encrypted messaging. This is explicitly provisional and superseded by the consensus-finalized group key (K3/A3) once the ceremony completes.

## 6. Consistency Metadata

Each operation category has a purpose-built status type for tracking consistency.

### 6.1 Core Types

```rust
pub enum Agreement {
    Provisional,
    SoftSafe { cert: Option<ConvergenceCert> },
    Finalized { consensus_id: ConsensusId },
}

pub enum Propagation {
    Local,
    Syncing { peers_reached: u16, peers_known: u16 },
    Complete,
    Failed { retry_at: PhysicalTime, retry_count: u32, error: String },
}

pub struct Acknowledgment {
    pub acked_by: Vec<AckRecord>,
}
```

Agreement indicates the finalization level (A1/A2/A3). Propagation tracks anti-entropy sync status. Acknowledgment tracks explicit per-peer delivery confirmation.

### 6.2 Category A: OptimisticStatus

```rust
pub struct OptimisticStatus {
    pub agreement: Agreement,
    pub propagation: Propagation,
    pub acknowledgment: Option<Acknowledgment>,
}
```

Use cases include send message, create channel, update profile, and react to message.

UI patterns:
- `◐` Sending: propagation == Local
- `✓` Sent: propagation == Complete
- `✓✓` Delivered: acknowledgment.count() >= expected.len()
- `◆` Finalized: agreement == Finalized

### 6.3 Category B: DeferredStatus

```rust
pub struct DeferredStatus {
    pub proposal_id: ProposalId,
    pub state: ProposalState,
    pub approvals: ApprovalProgress,
    pub applied_agreement: Option<Agreement>,
    pub expires_at: PhysicalTime,
}

pub enum ProposalState {
    Pending,
    Approved,
    Rejected { reason: String, by: AuthorityId },
    Expired,
    Superseded { by: ProposalId },
}
```

Use cases include change permissions, remove member, transfer ownership, and archive channel.

### 6.4 Category C: CeremonyStatus

```rust
pub struct CeremonyStatus {
    pub ceremony_id: CeremonyId,
    pub state: CeremonyState,
    pub responses: Vec<ParticipantResponse>,
    pub prestate_hash: Hash32,
    pub committed_agreement: Option<Agreement>,
}

pub enum CeremonyState {
    Preparing,
    PendingEpoch { pending_epoch: Epoch, required_responses: u16, received_responses: u16 },
    Committing,
    Committed { consensus_id: ConsensusId, committed_at: PhysicalTime },
    Aborted { reason: String, aborted_at: PhysicalTime },
    Superseded { by: CeremonyId, reason: SupersessionReason },
}
```

Use cases include add contact, create group, guardian rotation, device enrollment, and recovery.

When a ceremony commits successfully, `committed_agreement` is set to `Agreement::Finalized` with the consensus ID, indicating A3 durability.

### 6.5 Unified Consistency Type

For cross-category queries and generic handling:

```rust
pub struct Consistency {
    pub category: OperationCategory,
    pub agreement: Agreement,
    pub propagation: Propagation,
    pub acknowledgment: Option<Acknowledgment>,
}

pub enum OperationCategory {
    Optimistic,
    Deferred { proposal_id: ProposalId },
    Ceremony { ceremony_id: CeremonyId },
}
```

## 7. Ceremony Supersession

When a new ceremony replaces an old one, Aura emits explicit supersession facts that propagate via anti-entropy.

### 7.1 Supersession Reasons

```rust
pub enum SupersessionReason {
    PrestateStale,
    NewerRequest,
    ExplicitCancel,
    Timeout,
    Precedence,
}
```

`PrestateStale` indicates the prestate changed while the ceremony was pending. `NewerRequest` indicates an explicit newer request from the same initiator. `ExplicitCancel` indicates manual cancellation by an authorized participant. `Timeout` indicates the ceremony exceeded its validity window. `Precedence` indicates a concurrent ceremony won via conflict resolution.

### 7.2 Supersession Facts

Each ceremony fact enum includes a `CeremonySuperseded` variant:

```rust
CeremonySuperseded {
    superseded_ceremony_id: String,
    superseding_ceremony_id: String,
    reason: String,
    trace_id: Option<String>,
    timestamp_ms: u64,
}
```

### 7.3 CeremonyTracker API

The `CeremonyTracker` in `aura-agent` maintains supersession records for auditability:

| Method | Purpose |
|--------|---------|
| `supersede(old_id, new_id, reason)` | Record a supersession event |
| `check_supersession_candidates(prestate_hash, op_type)` | Find stale ceremonies |
| `get_supersession_chain(ceremony_id)` | Get full supersession history |
| `is_superseded(ceremony_id)` | Check if ceremony was replaced |

Supersession facts propagate via the existing anti-entropy mechanism. Peers receiving a `CeremonySuperseded` fact update their local ceremony state accordingly.

## 8. Decision Tree

Use this tree to categorize new operations:

```
Does this operation establish or modify cryptographic relationships?
│
├─ YES: Does the user need to wait for completion?
│       │
│       ├─ YES (new context, key changes) → Category C (Blocking Ceremony)
│       │   Examples: add contact, create group, guardian rotation
│       │
│       └─ NO (removal from existing context) → Category B (Deferred)
│           Examples: remove from group (epoch rotation in background)
│
└─ NO: Does this affect other users' access or policies?
       │
       ├─ YES: Is this high-security or irreversible?
       │       │
       │       ├─ YES → Category B (Deferred)
       │       │   Examples: transfer ownership, delete channel, kick member
       │       │
       │       └─ NO → Category A (Optimistic)
       │           Examples: pin message, update topic
       │
       └─ NO → Category A (Optimistic)
           Examples: send message, create channel, block contact
```

## 9. UI Feedback Patterns

### 9.1 Category A: Instant Result with Sync Indicator

```
┌───────────────────────────────────┐
│ You: Hello everyone!         ◆ ✓✓ │  ← Finalized + Delivered
│ You: Check this out            ✓✓ │  ← Delivered (not yet finalized)
│ You: Another thought           ✓  │  ← Sent
│ You: New idea                  ◐  │  ← Sending
└───────────────────────────────────┘
```

Effect already applied. Indicators show delivery status (◐ → ✓ → ✓✓ → ✓✓ blue) and finalization (◆ appears when A3 consensus achieved).

### 9.2 Category B: Pending Indicator

```
┌─────────────────────────────────────────────────────────────────────┐
│ Channel: #project                                                   │
├─────────────────────────────────────────────────────────────────────┤
│ Pending: Remove Carol (waiting for Bob to confirm)                  │
├─────────────────────────────────────────────────────────────────────┤
│ Members:                                                            │
│   Alice (admin)        ✓                                            │
│   Bob (admin)          ✓                                            │
│   Carol                ✓  ← Still has access until confirmed        │
└─────────────────────────────────────────────────────────────────────┘
```

Proposal shown. Effect NOT applied yet.

### 9.3 Category C: Blocking Wait

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Adding Bob to group...                           │
│                                                                     │
│    ✓ Invitation sent                                                │
│    ✓ Bob accepted                                                   │
│    ◐ Deriving group keys...                                         │
│    ○ Ready                                                          │
│                                                                     │
│                      [Cancel]                                       │
└─────────────────────────────────────────────────────────────────────┘
```

User waits. Cannot proceed until ceremony completes.

## 10. Effect Policy Configuration

Operations use configurable policies that reference the capability system:

```rust
pub struct EffectPolicy {
    pub operation: OperationType,
    pub timing: EffectTiming,
    pub security_level: SecurityLevel,
}

pub enum EffectTiming {
    Immediate,
    Deferred {
        requires_approval_from: Vec<CapabilityRequirement>,
        timeout_ms: u64,
        threshold: ApprovalThreshold,
    },
    Blocking {
        ceremony: CeremonyType,
    },
}
```

### 10.1 Context-Specific Overrides

Contexts can override default policies:

```rust
// Strict security channel: unanimous admin approval for kicks
channel.set_effect_policy(RemoveFromChannel, EffectTiming::Deferred {
    requires_approval_from: vec![CapabilityRequirement::Role("admin")],
    timeout_ms: 48 * 60 * 60 * 1000,
    threshold: ApprovalThreshold::Unanimous,
});

// Casual channel: any admin can kick immediately
channel.set_effect_policy(RemoveFromChannel, EffectTiming::Immediate);
```

## 11. Full Operation Matrix

| Operation | Category | Effect Timing | Security | Notes |
|-----------|----------|---------------|----------|-------|
| **Within Established Context** |
| Send message | A | Immediate | Low | Keys already derived |
| Create channel | A | Immediate | Low | Just facts into context |
| Update topic | A | Immediate | Low | CRDT, last-write-wins |
| React to message | A | Immediate | Low | Local expression |
| **Local Authority** |
| Block contact | A | Immediate | Low | Your decision |
| Mute channel | A | Immediate | Low | Local preference |
| **Policy Changes** |
| Change permissions | B | Deferred | Medium | Others affected |
| Kick from channel | B | Deferred | Medium | Affects access |
| Archive channel | B | Deferred | Low-Med | Reversible |
| **High Risk** |
| Transfer ownership | B | Deferred | High | Irreversible |
| Delete channel | B | Deferred | High | Data loss |
| Remove from context | B | Deferred | High | Affects encryption |
| **Cryptographic** |
| Add contact | C | Blocking | Critical | Creates context |
| Create group | C | Blocking | Critical | Multi-party keys |
| Add group member | C | Blocking | Critical | Changes group keys |
| Device enrollment | C | Blocking | Critical | DeviceEnrollment choreography |
| Guardian rotation | C | Blocking | Critical | Key shares |
| Recovery execution | C | Blocking | Critical | Account state |
| Device revocation | C | Blocking | Critical | Security response |

## 12. Common Mistakes to Avoid

### Mistake 1: Making Everything Category C

Wrong: "Adding a channel member requires ceremony"

Right: If the member is already in the relational context, it is Category A. Just emit a fact. Only if they need to join the context first is it Category C.

### Mistake 2: Forgetting Context Existence

Wrong: Trying to create a channel before establishing relationship

Right: Contact invitation (Category C) must complete before any channel operations (Category A) are possible.

### Mistake 3: Optimistic Key Operations

Wrong: "User can start using new guardians while ceremony runs"

Right: Guardian changes affect key shares. Partial state means unusable keys. Must be Category C.

### Mistake 4: Blocking on Low-Risk Operations

Wrong: "Wait for all members to confirm before showing channel"

Right: Channel creation is optimistic. Show immediately, sync status later.

## See Also

- [Consensus](106_consensus.md) for fast path and fallback consensus
- [Journal](103_journal.md) for fact semantics and reduction flows
- [AMP Protocol](110_amp.md) for channel encryption and key derivation
- [Relational Contexts](112_relational_contexts.md) for context vs channel distinction
- [Choreography Guide](108_mpst_and_choreography.md) for session types in Category C
- [Transport](109_transport_and_information_flow.md) for sync status tracking
- [Effect System](105_effect_system_and_runtime.md) for effect policies
