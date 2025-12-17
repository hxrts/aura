# Operation Categories

This document defines Aura's three-tier classification system for distributed operations. The core insight is that **not all operations require consensus** - many can proceed optimistically with background reconciliation.

## Overview

Operations in Aura fall into three categories based on their effect timing and security requirements:

| Category | Name | Effect Timing | When Used |
|----------|------|---------------|-----------|
| A | Optimistic | Immediate local effect | Low-risk operations within established contexts |
| B | Deferred | Pending until confirmed | Medium-risk policy/membership changes |
| C | Consensus-Gated | Blocked until ceremony completes | Cryptographic context establishment |

### The Key Architectural Insight

**Ceremonies establish shared cryptographic context. Operations within that context are cheap.**

```
┌─────────────────────────────────────────────────────────────────────┐
│                     CEREMONY (Category C)                           │
│  Invitation acceptance between Alice and Bob                        │
│  - Runs once per relationship                                       │
│  - Establishes ContextId + shared tree roots                        │
│  - Creates relational context journal                               │
│  - All future encryption derives from this                          │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│              OPTIMISTIC OPERATIONS (Category A)                     │
│  Within the established relational context:                         │
│                                                                     │
│  • Create channel     → Just emit ChannelCheckpoint fact            │
│  • Send message       → Derive key from context, encrypt, send      │
│  • Add channel member → Just emit ChannelMemberAdded fact           │
│    (if already in context)                                          │
│                                                                     │
│  Keys derive deterministically: KDF(ContextRoot, ChannelId, epoch)  │
│  No new agreement needed - shared state already exists              │
└─────────────────────────────────────────────────────────────────────┘
```

## Category A: Optimistic Operations

**Characteristics:**
- Immediate local effect via CRDT fact emission
- Background sync via anti-entropy
- Failure shows status indicator, doesn't block functionality
- Partial success is acceptable

### Examples

| Operation | Immediate Action | Background Sync | On Failure |
|-----------|-----------------|-----------------|------------|
| Create channel | Show channel, enable messaging | Fact syncs to members | Show "unsynced" badge |
| Send message | Display in chat immediately | Delivery receipts | Show "undelivered" indicator |
| Add contact (within context) | Show in list | Mutual acknowledgment | Show "pending" status |
| Block contact | Hide from view immediately | Propagate to context | Already effective locally |
| Update profile | Show changes immediately | Propagate to contacts | Show sync indicator |
| React to message | Show reaction | Fact syncs | Show "pending" |

### Implementation Pattern

```rust
// Category A operations emit CRDT facts immediately
async fn create_channel_optimistic(&mut self, config: ChannelConfig) -> ChannelId {
    // 1. Generate deterministic channel ID
    let channel_id = ChannelId::derive(&config);

    // 2. Emit fact into existing relational context journal
    self.emit_fact(ChatFact::ChannelCheckpoint {
        channel_id,
        epoch: 0,
        base_gen: 0,
        window: 1024,
    }).await;

    // 3. Channel is immediately usable
    // Key derivation: KDF(ContextRoot, ChannelId, epoch)
    channel_id
}
```

### Why This Works

Category A operations work because:
1. **Encryption keys already exist** - derived from established context
2. **Facts are CRDTs** - eventual consistency is sufficient
3. **No coordination needed** - shared state already agreed upon
4. **Worst case is delay** - not security issue

## Category B: Deferred Operations

**Characteristics:**
- Local effect pending until agreement reached
- UI shows intent immediately with "pending" indicator
- May require approval from capability holders
- Automatic rollback on rejection

### Examples

| Operation | Immediate Action | Agreement Required | On Rejection |
|-----------|-----------------|-------------------|--------------|
| Change channel permissions | Show "pending" | Admin approval | Revert, notify |
| Remove channel member | Show "pending removal" | Admin consensus | Keep member |
| Transfer ownership | Show "pending transfer" | Recipient acceptance | Cancel transfer |
| Rename channel | Show "pending rename" | Member acknowledgment | Keep old name |
| Archive channel | Show "pending archive" | Admin approval | Stay active |

### Implementation Pattern

```rust
// Category B operations create proposals
async fn change_permissions_deferred(
    &mut self,
    channel_id: ChannelId,
    changes: PermissionChanges,
) -> ProposalId {
    // 1. Create proposal (does not apply effect yet)
    let proposal = Proposal {
        operation: Operation::ChangePermissions { channel_id, changes },
        requires_approval_from: vec![CapabilityRequirement::Role("admin")],
        threshold: ApprovalThreshold::Any,
        timeout_ms: 24 * 60 * 60 * 1000, // 24 hours
    };

    // 2. Emit proposal fact
    let proposal_id = self.emit_proposal(proposal).await;

    // 3. UI shows "pending" state
    // Effect applies when threshold approvals received
    // Auto-reverts on timeout or rejection
    proposal_id
}
```

### Approval Thresholds

```rust
pub enum ApprovalThreshold {
    /// Any single holder of the required capability
    Any,
    /// All holders must approve
    Unanimous,
    /// k-of-n approval
    Threshold { required: usize },
    /// Percentage of holders
    Percentage { percent: u8 },
}
```

## Category C: Consensus-Gated Operations

**Characteristics:**
- Operation does NOT proceed until ceremony completes
- Partial state would be dangerous or irrecoverable
- User must wait for confirmation
- Uses choreographic protocols with session types

### Examples

| Operation | Why Blocking Required | Risk if Optimistic |
|-----------|----------------------|-------------------|
| Add contact (new relationship) | Creates cryptographic context | No shared keys possible |
| Create group | Multi-party key agreement | Inconsistent member views |
| Add member to group | Changes group keys | Forward secrecy violation |
| Guardian rotation | Key shares distributed atomically | Partial rotation = unusable keys |
| Recovery execution | Account state replacement | Partial recovery = corruption |
| OTA hard fork | Breaking protocol change | Partial activation = network split |
| Device revocation | Security-critical removal | Attacker acts before propagation |

### Implementation Pattern

Category C operations use existing ceremony infrastructure:

```rust
// Category C operations block until ceremony completes
async fn add_contact(&mut self, invitation: Invitation) -> Result<ContactId> {
    // 1. Initiate ceremony
    let ceremony_id = self.ceremony_executor
        .initiate_invitation_ceremony(invitation)
        .await?;

    // 2. Block until completion (user sees progress UI)
    loop {
        match self.ceremony_executor.get_status(&ceremony_id)? {
            CeremonyStatus::Committed => {
                // Context established, contact usable
                return Ok(ContactId::from_ceremony(&ceremony_id));
            }
            CeremonyStatus::Aborted { reason } => {
                return Err(AuraError::ceremony_failed(reason));
            }
            _ => {
                // Still in progress, show status to user
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}
```

### Ceremony Properties

All Category C ceremonies implement:

1. **Prestate Binding**: `CeremonyId = H(prestate_hash, operation_hash, nonce)`
   - Prevents concurrent ceremonies on same state
   - Ensures exactly-once semantics

2. **Atomic Commit/Abort**:
   - Either fully committed or no effect
   - No partial state possible

3. **Epoch Isolation**:
   - Uncommitted key packages are inert
   - No explicit rollback needed on abort

4. **Session Types**:
   - Protocol compliance enforced at compile time
   - Communication errors impossible

## Decision Tree

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

## UI Feedback Patterns

### Category A: Instant Result with Sync Indicator

```
┌─────────────────────────────────┐
│ You: Hello everyone!         ✓✓ │  ← Delivered
│ You: Check this out          ✓  │  ← Sent
│ You: New idea                ◐  │  ← Sending
└─────────────────────────────────┘
```

Effect already applied. Indicator shows sync/delivery status.

### Category B: Pending Indicator

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

### Category C: Blocking Wait

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

## Status Tracking

### SyncStatus (Category A)

```rust
pub enum SyncStatus {
    /// Fact committed locally, not yet synced
    LocalOnly,
    /// Fact synced to some peers
    Syncing { peers_synced: usize, peers_total: usize },
    /// Fact synced to all known peers
    Synced,
    /// Sync failed, will retry
    SyncFailed { retry_at: u64 },
}
```

### DeliveryStatus (Messages)

```rust
pub enum DeliveryStatus {
    /// Message queued locally
    Sending,
    /// Message reached at least one recipient
    Sent,
    /// Message reached all online recipients
    Delivered,
    /// Recipient viewed message
    Read,
    /// Delivery failed
    Failed { reason: String },
}
```

### ConfirmationStatus (Category B)

```rust
pub enum ConfirmationStatus {
    /// Proposal created, awaiting approvals
    Pending { approvals: usize, required: usize },
    /// Threshold approvals received, applying effect
    Confirming,
    /// Effect applied successfully
    Confirmed,
    /// Some parties approved, others declined
    PartiallyConfirmed { confirmed: Vec<AuthorityId>, declined: Vec<AuthorityId> },
    /// Proposal rejected or timed out
    Rejected { reason: String },
}
```

## Effect Policy Configuration

Operations use configurable policies that reference the capability system:

```rust
pub struct EffectPolicy {
    pub operation: OperationType,
    pub scope: ResourceScope,
    pub timing: EffectTiming,
}

pub enum EffectTiming {
    /// Category A: Immediate effect
    Immediate,

    /// Category B: Deferred until approval
    Deferred {
        requires_approval_from: Vec<CapabilityRequirement>,
        timeout_ms: u64,
        threshold: ApprovalThreshold,
    },

    /// Category C: Blocked until ceremony
    Blocking {
        ceremony: CeremonyType,
    },
}
```

### Context-Specific Overrides

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

## Full Operation Matrix

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
| Guardian rotation | C | Blocking | Critical | Key shares |
| Recovery execution | C | Blocking | Critical | Account state |
| Device revocation | C | Blocking | Critical | Security response |

## Common Mistakes to Avoid

### Mistake 1: Making Everything Category C

**Wrong**: "Adding a channel member requires ceremony"

**Right**: If the member is already in the relational context, it's Category A - just emit a fact. Only if they need to join the context first is it Category C.

### Mistake 2: Forgetting Context Existence

**Wrong**: Trying to create a channel before establishing relationship

**Right**: Contact invitation (Category C) must complete before any channel operations (Category A) are possible.

### Mistake 3: Optimistic Key Operations

**Wrong**: "User can start using new guardians while ceremony runs"

**Right**: Guardian changes affect key shares. Partial state means unusable keys. Must be Category C.

### Mistake 4: Blocking on Low-Risk Operations

**Wrong**: "Wait for all members to confirm before showing channel"

**Right**: Channel creation is optimistic. Show immediately, sync status later.

## Related Documentation

- [Consensus](104_consensus.md) - When Aura Consensus is required
- [AMP Protocol](112_amp.md) - Channel encryption and key derivation
- [Relational Contexts](103_relational_contexts.md) - Context vs channel distinction
- [Choreography Guide](107_mpst_and_choreography.md) - Session types for Category C
- [Transport](108_transport_and_information_flow.md) - Sync status tracking
- [Effect System](106_effect_system_and_runtime.md) - Effect policies
- [Design Document](../work/optimistic.md) - Full design rationale
