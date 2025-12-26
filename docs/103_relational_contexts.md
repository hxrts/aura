# Relational Contexts

This document describes the architecture of relational contexts in Aura. It explains how cross-authority relationships are represented using dedicated context namespaces. It defines the structure of relational facts and the role of [Aura Consensus](104_consensus.md) in producing agreed relational state. It also describes privacy boundaries and the interpretation of relational data by participating authorities.

## 1. RelationalContext Abstraction

A relational context is shared state linking two or more authorities. A relational context has its own [journal](102_journal.md) namespace. A relational context does not expose internal authority structure. A relational context contains only the facts that the participating authorities choose to share.

A relational context is identified by a `ContextId`. Authorities publish relational facts inside the context journal. The context journal is a join semilattice under set union. Reduction produces a deterministic relational state.

```rust
pub struct ContextId(Uuid);
```

This identifier selects the journal namespace for a relational context. It does not encode participant information. It does not reveal the type of relationship. Only the participants know how the context is used.

## 2. Participants and Fact Store

A relational context has a defined set of participating authorities. This set is not encoded in the `ContextId`. Participation is expressed by writing relational facts to the context journal. Each fact references the commitments of the participating authorities.

```rust
/// Canonical relational facts in `aura-journal/src/fact.rs`
pub enum RelationalFact {
    // Protocol-level receipts (handled directly by `reduce_context`)
    GuardianBinding { account_id: AuthorityId, guardian_id: AuthorityId, binding_hash: Hash32 },
    RecoveryGrant { account_id: AuthorityId, guardian_id: AuthorityId, grant_hash: Hash32 },
    Consensus { consensus_id: Hash32, operation_hash: Hash32, threshold_met: bool, participant_count: u16 },

    // Domain-level extensibility (reduced via `FactRegistry`)
    Generic { context_id: ContextId, binding_type: String, binding_data: Vec<u8> },
}
```

The `Generic` variant is the extensibility mechanism for application/domain facts. Domain crates implement the `DomainFact` trait (`aura-journal/src/extensibility.rs`) and store facts as `RelationalFact::Generic` via `DomainFact::to_generic()`. The runtime registers reducers in `crates/aura-agent/src/fact_registry.rs` so `reduce_context()` can turn Generic facts into `RelationalBinding` values.

## 3. Prestate Model

Relational context operations use a prestate model. Aura Consensus verifies that all witnesses see the same authority states. The prestate hash binds the relational fact to current authority states.

```rust
let prestate_hash = H(C_auth1, C_auth2, C_context);
```

This hash represents the commitments of the authorities and the current context. Aura Consensus witnesses check this hash before producing shares. The final commit fact includes a threshold signature over the relational operation.

## 4. Types of Relational Contexts

Several categories of relational contexts appear in Aura. Each fact type carries a well-defined schema to ensure deterministic reduction.

### GuardianConfigContext

Stores `GuardianBinding` facts:

```json
{
  "type": "GuardianBinding",
  "account_commitment": "Hash32",
  "guardian_commitment": "Hash32",
  "parameters": {
    "recovery_delay_secs": 86400,
    "notification_required": true
  },
  "consensus_commitment": "Hash32",
  "consensus_proof": "ThresholdSignature"
}
```

- `account_commitment`: reduced commitment of the protected authority.
- `guardian_commitment`: reduced commitment of the guardian authority.
- `parameters`: serialized GuardianParameters (delay, notification policy, etc.).
- `consensus_commitment`: commitment hash of the Aura Consensus instance that approved the binding.
- `consensus_proof`: aggregated signature from the witness set.

### GuardianRecoveryContext

Stores `RecoveryGrant` facts:

```json
{
  "type": "RecoveryGrant",
  "account_commitment_old": "Hash32",
  "account_commitment_new": "Hash32",
  "guardian_commitment": "Hash32",
  "operation": {
    "kind": "ReplaceTree",
    "payload": "Base64(TreeOp)"
  },
  "consensus_commitment": "Hash32",
  "consensus_proof": "ThresholdSignature"
}
```

- `account_commitment_old/new`: before/after commitments for the account authority.
- `guardian_commitment`: guardian authority that approved the grant.
- `operation`: serialized recovery operation (matches TreeOp schema).
- `consensus_*`: Aura Consensus identifiers tying the grant to witness approvals.

### Generic / Application Contexts

Shared group or project contexts can store application-defined facts:

```json
{
  "type": "Generic",
  "payload": "Base64(opaque application data)",
  "bindings": ["Hash32 commitment of participant A", "Hash32 commitment of participant B"],
  "labels": ["project:alpha", "role:reviewer"]
}
```

Generic facts should include enough metadata (`bindings`, optional labels) for interpreters to apply context-specific rules.

## 5. Relational Facts

Relational facts express specific cross-authority operations. A `GuardianBinding` fact defines the guardian authority for an account. A `RecoveryGrant` fact defines an allowed update to the account state. A `Generic` fact covers application defined interactions. Consensus-backed facts include the `consensus_commitment` and aggregated signature so reducers can verify provenance even after witnesses rotate.

Reduction applies all relational facts to produce relational state. Reduction verifies that authority commitments in each fact match the current reduced state of each authority.

```rust
/// Reduced relational state from aura-journal/src/reduction.rs
pub struct RelationalState {
    /// Active relational bindings
    pub bindings: Vec<RelationalBinding>,
    /// Flow budget state by context
    pub flow_budgets: BTreeMap<(AuthorityId, AuthorityId, u64), u64>,
    /// AMP channel epoch state keyed by channel id
    pub channel_epochs: BTreeMap<ChannelId, ChannelEpochState>,
}

pub struct RelationalBinding {
    pub binding_type: RelationalBindingType,
    pub context_id: ContextId,
    pub data: Vec<u8>,
}
```

This structure represents the reduced relational state. It contains relational bindings, flow budget tracking between authorities, and AMP channel epoch state for message ratcheting. Reduction processes all facts in the context journal to derive this state deterministically.

## 6. Aura Consensus in Relational Contexts

Some relational operations require strong agreement. [Aura Consensus](104_consensus.md) produces these operations. Aura Consensus uses a witness set drawn from participating authorities. Witnesses compute shares after verifying the prestate hash.

Commit facts contain threshold signatures. Each commit fact is inserted into the relational context journal. Reduction interprets the commit fact as a confirmed relational operation.

Aura Consensus binds relational operations to authority state. After consensus completes, the initiator inserts a `CommitFact` into the relational context journal that includes:

```rust
pub struct RelationalCommitFact {
    pub context_id: ContextId,
    pub consensus_commitment: Hash32, // H(Op, prestate)
    pub fact: RelationalFact,         // GuardianBinding, RecoveryGrant, etc.
    pub aggregated_signature: ThresholdSignature,
    pub attesters: BTreeSet<AuthorityId>,
}
```

Reducers validate `aggregated_signature` before accepting the embedded `RelationalFact`. This mirrors the account-level process but scoped to the context namespace.

## 7. Interpretation of Relational State

Authorities interpret relational state by reading the relational context journal. An account reads `GuardianBinding` facts to determine its guardian authority. A guardian authority reads the same facts to determine which accounts it protects.

Other relational contexts follow similar patterns. Each context defines its own interpretation rules. No context affects authority internal structure. Context rules remain confined to the specific relationship.

## 8. Privacy and Isolation

A relational context does not reveal its participants. The `ContextId` is opaque. Only participating authorities know the relationship. No external party can infer connections between authorities based on context identifiers.

Profile information shared inside a context stays local to that context. Display names and contact attributes do not leave the context journal. Each context forms a separate identity boundary. Authorities can maintain many unrelated relationships without cross linking.

## 9. Implementation Patterns

The implementation in `aura-relational` provides concrete patterns for working with relational contexts.

### Creating and Managing Contexts

```rust
use aura_core::{AuthorityId, ContextId};
use aura_relational::RelationalContext;

// Create a new guardian-account relational context
let account_authority = AuthorityId::new();
let guardian_authority = AuthorityId::new();

let context = RelationalContext::new(vec![account_authority, guardian_authority]);

// Or use a specific context ID
let context_id = ContextId::new();
let context = RelationalContext::with_id(
    context_id,
    vec![account_authority, guardian_authority],
);

// Check participation
assert!(context.is_participant(&account_authority));
assert!(context.is_participant(&guardian_authority));
```

### Guardian Binding Pattern

```rust
use aura_core::relational::{GuardianBinding, GuardianParameters};
use std::time::Duration;

let params = GuardianParameters {
    recovery_delay: Duration::from_secs(86400),
    notification_required: true,
    // Use Aura's unified time system (PhysicalTimeEffects/TimeStamp) for expiration.
    expiration: None,
};

let binding = GuardianBinding::new(
    Hash32::from_bytes(&account_authority.to_bytes()),
    Hash32::from_bytes(&guardian_authority.to_bytes()),
    params,
);

// Store a binding receipt + full payload via the context's journal-backed API
context.add_guardian_binding(account_authority, guardian_authority, binding)?;
```

### Recovery Grant Pattern

```rust
use aura_core::relational::{RecoveryGrant, RecoveryOp, ConsensusProof};

// Construct a recovery operation
let recovery_op = RecoveryOp::AddDevice {
    device_public_key: new_device_pubkey.to_bytes(),
};

// Create recovery grant (requires consensus proof)
let grant = RecoveryGrant {
    account_old: old_tree_commitment,
    account_new: new_tree_commitment,
    guardian: guardian_commitment,
    operation: recovery_op,
    consensus_proof: consensus_result.proof, // From Aura Consensus
};

// Add to recovery context
recovery_context.add_fact(RelationalFact::RecoveryGrant(grant))?;

// Check operation type
if grant.operation.is_emergency() {
    // Handle emergency operations immediately
    execute_emergency_recovery(&grant)?;
}
```

### Query Patterns

```rust
// Query guardian bindings
let bindings = context.guardian_bindings();
for binding in bindings {
    println!("Guardian: {:?}", binding.guardian_commitment);
    println!("Recovery delay: {:?}", binding.parameters.recovery_delay);

    if let Some(expiration) = binding.parameters.expiration {
        if expiration < Utc::now() {
            // Binding has expired
        }
    }
}

// Find specific guardian binding
if let Some(binding) = context.get_guardian_binding(account_authority) {
    // Found guardian for this account
    let guardian_id = binding.guardian_commitment;
}

// Query recovery grants
let grants = context.recovery_grants();
for grant in grants {
    println!("Operation: {}", grant.operation.description());
    println!("From: {:?} -> To: {:?}", grant.account_old, grant.account_new);
}
```

### Generic Binding Pattern

```rust
use aura_core::relational::{GenericBinding, RelationalFact};

// Application-specific binding (e.g., project collaboration)
let binding_data = serde_json::to_vec(&ProjectMetadata {
    name: "Alpha Project",
    role: "Reviewer",
    permissions: vec!["read", "comment"],
})?;

let generic = GenericBinding {
    binding_type: "project_collaboration".to_string(),
    binding_data,
    consensus_proof: None,
};

context.add_fact(RelationalFact::Generic(generic))?;
```

### Prestate Computation Pattern

```rust
use aura_core::Prestate;

// Collect current authority commitments
let authority_commitments = vec![
    (account_authority, account_tree_commitment),
    (guardian_authority, guardian_tree_commitment),
];

// Compute prestate for consensus
let prestate = context.compute_prestate(authority_commitments);

// Use prestate in consensus protocol
let consensus_result = run_consensus(
    prestate,
    operation_data,
    witness_set,
).await?;
```

### Journal Commitment Pattern

```rust
// Get deterministic commitment of current relational state
let commitment = context.journal.compute_commitment();

// Commitment is deterministic - all replicas compute same value
// Used for:
// - Prestate computation in consensus
// - Context verification
// - Anti-entropy sync checkpoints
```

### Integration with Aura Consensus

```rust
use aura_core::relational::ConsensusProof;
use aura_relational::run_consensus;

// 1. Prepare operation requiring consensus
let binding = GuardianBindingBuilder::new()
    .account(account_commitment)
    .guardian(guardian_commitment)
    .build()?;

// 2. Compute prestate
let prestate = context.compute_prestate(authority_commitments);

// 3. Run consensus
let consensus_proof = run_consensus(
    prestate,
    serde_json::to_vec(&binding)?,
    witness_set,
).await?;

// 4. Attach proof to fact
let binding_with_proof = GuardianBinding {
    consensus_proof: Some(consensus_proof),
    ..binding
};

// 5. Add to context
context.add_fact(RelationalFact::GuardianBinding(binding_with_proof))?;
```

### Recovery Operation Selection

```rust
use aura_core::relational::RecoveryOp;

// Select appropriate recovery operation
let recovery_op = match recovery_scenario {
    RecoveryScenario::LostAllDevices => RecoveryOp::ReplaceTree {
        new_tree_root: new_tree_commitment,
    },
    RecoveryScenario::AddNewDevice => RecoveryOp::AddDevice {
        device_public_key: device_key.to_bytes(),
    },
    RecoveryScenario::RemoveCompromised => RecoveryOp::RemoveDevice {
        leaf_index: compromised_device_index,
    },
    RecoveryScenario::ChangeThreshold => RecoveryOp::UpdatePolicy {
        new_threshold: new_m_of_n.0,
    },
    RecoveryScenario::EmergencyCompromise => RecoveryOp::EmergencyRotation {
        new_epoch: current_epoch + 1,
    },
};

// Emergency operations bypass delay
if recovery_op.is_emergency() {
    // No recovery_delay applied
} else {
    // Wait for binding.parameters.recovery_delay
}
```

### Best Practices

**Guardian Configuration:**
- Use 24-hour minimum recovery delay for security
- Always require notification unless emergency scenario
- Set expiration for temporary guardian relationships
- Rotate guardian bindings periodically

**Recovery Grants:**
- Always require consensus proof for recovery operations
- Validate prestate commitments before accepting grants
- Log all recovery operations for audit trail
- Emergency operations should be rare and logged prominently

**Generic Bindings:**
- Document binding_type schema externally
- Version your binding_data format
- Include consensus_proof for critical application bindings
- Keep binding_data size reasonable for sync performance

**Context Management:**
- Use opaque ContextId - never encode participant info
- Limit participants to 2-10 authorities for efficiency
- Separate contexts for different relationship types
- Garbage collect expired bindings periodically

## 10. Contexts vs Channels: Operation Categories

Understanding the distinction between relational contexts and channels is essential for operation categorization.

### 10.1 Relational Contexts (Category C - Consensus Required)

Creating a relational context establishes a cryptographic relationship between authorities. This is a **Category C (consensus-gated)** operation because:

- It creates the shared secret foundation for all future communication
- Both parties must agree to establish the relationship
- Partial state (one party thinks relationship exists, other doesn't) is dangerous

**Examples:**
- Adding a contact (bilateral context between two authorities)
- Creating a group (multi-party context with all members)
- Adding a member to an existing group (extends the cryptographic context)

### 10.2 Channels Within Contexts (Category A - Optimistic)

Once a relational context exists, channels are **Category A (optimistic)** operations:

- Channels are just organizational substreams within the context
- No new cryptographic agreement needed - keys derive from context
- Channel facts sync via anti-entropy, eventual consistency is sufficient

**Examples within existing context:**
- Create channel → emit `ChannelCheckpoint` fact
- Send message → derive key from context, encrypt, send
- Update topic → emit fact to context journal

### 10.3 The Cost Structure

```
                    BILATERAL                      MULTI-PARTY
                    (2 members)                    (3+ members)
                    ───────────                    ────────────
Context Creation    Invitation ceremony            Group ceremony
                    (Category C - expensive)       (Category C - expensive)

Member Addition     N/A (already 2)               Per-member ceremony
                                                  (Category C - expensive)

Channel Creation    Optimistic                    Optimistic
                    (Category A - cheap)          (Category A - cheap)

Messages            Optimistic                    Optimistic
                    (Category A - cheap)          (Category A - cheap)
```

The expensive part is establishing WHO is in the group. Once that's established, operations WITHIN the group are cheap.

### 10.4 Multi-Party Context Keys

Groups with >2 members derive keys from all member tree roots:

```
GroupContext {
    context_id: ContextId,
    members: [Alice, Bob, Carol],
    group_secret: DerivedFromMemberTreeRoots,
    epoch: u64,
}

Key Derivation:
1. Each member contributes their tree root commitment
2. Group secret = KDF(sorted_member_roots, context_id)
3. Channel key = KDF(group_secret, channel_id, epoch)

All members derive the SAME group secret from the SAME inputs
```

### 10.5 Why Membership Changes Require Ceremony

Group membership changes are Category C because they affect encryption:

1. **Forward Secrecy**: New members shouldn't read old messages
   - Solution: Epoch rotation, new keys for new messages

2. **Post-Compromise Security**: Removed members shouldn't read new messages
   - Solution: Epoch rotation, re-derive group secret without removed member

3. **Consistency**: All members must agree on who's in the group
   - Solution: Ceremony ensures atomic membership view

See [Consensus - Operation Categories](104_consensus.md#17-operation-categories) for the full decision tree.

## 11. Summary

Relational contexts represent cross-authority relationships in Aura. They provide shared state without revealing authority structure. They support guardian configuration, recovery, and application specific collaboration. Aura Consensus ensures strong agreement where needed. Deterministic reduction ensures consistent relational state. Privacy boundaries isolate each relationship from all others.

The implementation provides concrete types (`RelationalContext`, `GuardianBinding`, `RecoveryGrant`) with builder patterns, query methods, and consensus integration. All relational facts are stored in a CRDT journal with deterministic commitment computation.

## 12. Implementation References

- **Core Types**: `aura-core/src/relational/` - RelationalFact, GuardianBinding, RecoveryGrant, ConsensusProof domain types
- **Journal Facts**: `aura-journal/src/fact.rs` - Protocol-level RelationalFact with AMP variants
- **Reduction**: `aura-journal/src/reduction.rs` - RelationalState, reduce_context()
- **Context Management**: `aura-relational/src/lib.rs` - RelationalContext (context-scoped fact journal mirror + helpers)
- **Consensus Integration**: `crates/aura-consensus/src/consensus/relational.rs` - consensus implementation
- **Consensus Adapter**: `aura-relational/src/consensus_adapter.rs` - thin consensus delegation layer
- **Prestate Computation**: `aura-core/src/domain/consensus.rs` - Prestate struct and methods
- **Protocol Usage**: `aura-authentication/src/guardian_auth_relational.rs` - Guardian authentication
- **Recovery Flows**: `aura-recovery/src/` - Guardian recovery choreographies
