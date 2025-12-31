# Coordination Systems Guide

This guide covers distributed coordination and privacy systems in Aura. You will learn CRDT programming patterns, commitment tree operations, web of trust relationships, flow budget management, session types, and basic choreographic programming.

## CRDT Programming

Conflict-free Replicated Data Types enable distributed applications to handle concurrent updates without coordination protocols. CRDTs use mathematical semilattice properties to ensure eventual consistency.

Aura provides builder patterns for easy CRDT setup:

```rust
use aura_protocol::effects::semilattice::CrdtCoordinator;

// Simple state-based CRDT
let coordinator = CrdtCoordinator::with_cv_state(device_id, initial_journal);

// Delta CRDT with compaction threshold
let coordinator = CrdtCoordinator::with_delta_threshold(device_id, 100);

// Meet-semilattice for constraints
let coordinator = CrdtCoordinator::with_mv_state(device_id, capability_set);
```

The builder pattern eliminates manual handler registration. Each method creates appropriate handler types with sensible defaults. Coordinators integrate directly with choreographic protocols.

### Join Semilattices

Join semilattices accumulate knowledge through union operations:

```rust
use aura_journal::algebra::{Join, GCounterState};

impl Join for GCounterState {
    fn join(&self, other: &Self) -> Self {
        let mut merged_counts = self.device_counts.clone();

        for (device_id, count) in &other.device_counts {
            let current_count = merged_counts.get(device_id).copied().unwrap_or(0);
            merged_counts.insert(*device_id, current_count.max(*count));
        }

        GCounterState { device_counts: merged_counts }
    }
}
```

Join operations merge device-specific contributions by taking maximum values. This ensures increments from all devices are preserved. The operation is associative, commutative, and idempotent.

### Meet Semilattices

Meet semilattices refine authority through intersection operations:

```rust
use aura_authorization::CapabilitySet;

impl Meet for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        let intersection = self.capabilities.intersection(&other.capabilities).cloned().collect();

        CapabilitySet { capabilities: intersection }
    }
}
```

Meet operations produce capability sets that are subsets of both inputs. This ensures conservative security where operations require explicit authorization from all sources.

### CRDT Integration

Integrate CRDTs with choreographic protocols:

```rust
use aura_sync::choreography::anti_entropy::execute_as_requester;

let coordinator = CrdtCoordinator::with_cv_state(device_id, journal_state);

let (result, updated_coordinator) = execute_anti_entropy(
    device_id,
    config,
    is_requester,
    &effect_system,
    coordinator,
).await?;

let synchronized_state = updated_coordinator.cv_handler().get_state();
```

Protocols consume and return coordinators with updated state. This enables immutable data flow patterns where protocol execution produces new state versions.

## Commitment Tree Operations

[Commitment trees](101_accounts_and_commitment_tree.md) provide forward secrecy for group communication. The tree structure enables efficient key updates when group membership changes.

### Tree Structure

Commitment trees organize group members in a binary tree:

```rust
use aura_journal::commitment_tree::{CommitmentTree, TreePosition};

pub struct GroupCommitmentTree {
    tree: CommitmentTree,
    device_id: aura_core::DeviceId,
    position: TreePosition,
}

impl GroupCommitmentTree {
    pub fn add_member(&mut self, new_member: aura_core::DeviceId) -> Result<TreeUpdate, RatchetError> {
        let new_position = self.tree.find_empty_leaf()?;
        let update = self.tree.insert_member(new_position, new_member)?;

        self.update_path_keys(&update)?;

        Ok(update)
    }

    pub fn remove_member(&mut self, member: aura_core::DeviceId) -> Result<TreeUpdate, RatchetError> {
        let position = self.tree.find_member_position(member)?;
        let update = self.tree.remove_member(position)?;

        self.update_path_keys(&update)?;

        Ok(update)
    }
}
```

Member addition inserts devices at empty leaf positions. Member removal clears the device position and updates affected key paths. Tree updates maintain forward secrecy properties.

### Group Key Derivation

Derive group encryption keys from tree state:

```rust
use aura_core::DerivedKeyContext;

impl GroupCommitmentTree {
    pub async fn derive_group_key<E: CryptoEffects>(
        &self,
        effects: &E,
        epoch: u64,
    ) -> Result<GroupKey, RatchetError> {
        let root_secret = self.tree.compute_root_secret()?;

        let context = DerivedKeyContext {
            app_id: "group_messaging".to_string(),
            context: format!("epoch_{}", epoch),
            derivation_path: vec![epoch],
        };

        let key_material = effects.derive_key(&root_secret, &context).await?;

        Ok(GroupKey::from_material(key_material))
    }
}
```

Key derivation uses tree root secrets and epoch information. Each epoch produces different keys even with the same tree structure. This provides forward secrecy across time periods.

### Tree Synchronization

Synchronize tree state across group members:

```rust
use aura_journal::commitment_tree::TreeOperation;

pub async fn synchronize_commitment_tree<E: NetworkEffects + CryptoEffects>(
    effects: &E,
    local_tree: &mut GroupCommitmentTree,
    peers: Vec<aura_core::DeviceId>,
) -> Result<(), SynchronizationError> {
    let tree_state = local_tree.export_state();
    let state_hash = effects.hash(&tree_state.serialize()?).await?;

    for peer in peers {
        let sync_request = TreeSyncRequest {
            state_hash: state_hash.clone(),
            requesting_device: local_tree.device_id,
        };

        let request_bytes = bincode::serialize(&sync_request)?;
        effects.send_to_peer(peer.into(), request_bytes).await?;
    }

    let responses = collect_sync_responses(effects, peers.len()).await?;
    let merged_operations = merge_tree_operations(responses)?;

    for operation in merged_operations {
        local_tree.apply_operation(operation)?;
    }

    Ok(())
}
```

Tree synchronization exchanges state hashes to detect differences. Devices with different hashes exchange tree operations. Operations apply in causal order to maintain consistency.

## Web of Trust

Web of trust systems model relationships between devices for authorization decisions. Trust relationships enable capability delegation and reputation-based access control.

### Relationship Formation

Use relational contexts plus `InvitationServiceApi` to model trust formation. Create a shared context for the participants, exchange capabilities via invitations (e.g., `InvitationType::Contact`), and record trust facts in the relational journal. The legacy `relationship_formation` module has been removed.

### Trust Computation

Compute trust levels using direct and transitive relationships:

```rust
use aura_authorization::{TrustGraph, TrustLevel};

pub struct DeviceTrustGraph {
    relationships: BTreeMap<(aura_core::DeviceId, aura_core::DeviceId), TrustLevel>,
}

impl DeviceTrustGraph {
    pub fn compute_trust_path(
        &self,
        source: aura_core::DeviceId,
        target: aura_core::DeviceId,
        max_hops: usize,
    ) -> Option<TrustLevel> {
        if source == target {
            return Some(TrustLevel::Complete);
        }

        if let Some(direct_trust) = self.relationships.get(&(source, target)) {
            return Some(*direct_trust);
        }

        self.find_transitive_path(source, target, max_hops, &mut BTreeSet::new())
    }

    fn find_transitive_path(
        &self,
        current: aura_core::DeviceId,
        target: aura_core::DeviceId,
        remaining_hops: usize,
        visited: &mut BTreeSet<aura_core::DeviceId>,
    ) -> Option<TrustLevel> {
        if remaining_hops == 0 || visited.contains(&current) {
            return None;
        }

        visited.insert(current);

        let mut best_trust = None;

        for ((source, intermediate), trust_level) in &self.relationships {
            if *source == current && !visited.contains(intermediate) {
                let onward_trust = self.find_transitive_path(*intermediate, target, remaining_hops - 1, visited);

                if let Some(onward) = onward_trust {
                    let path_trust = trust_level.attenuate(onward);

                    best_trust = match best_trust {
                        Some(current_best) => Some(current_best.max(path_trust)),
                        None => Some(path_trust),
                    };
                }
            }
        }

        visited.remove(&current);
        best_trust
    }
}
```

Trust computation considers both direct relationships and transitive paths. Transitive trust attenuates based on path length and intermediate trust levels. Maximum hop limits prevent infinite path exploration.

### Trust-Based Authorization

Use trust levels for authorization decisions:

```rust
use aura_authorization::{AuthorizationRequest, AuthorizationDecision};

pub fn evaluate_trust_authorization(
    request: &AuthorizationRequest,
    trust_graph: &DeviceTrustGraph,
    required_trust: TrustLevel,
) -> AuthorizationDecision {
    let computed_trust = trust_graph.compute_trust_path(
        request.requesting_device,
        request.resource_owner,
        MAX_TRUST_HOPS,
    );

    match computed_trust {
        Some(trust_level) if trust_level >= required_trust => {
            AuthorizationDecision::Allow {
                trust_level,
                delegation_path: trust_graph.get_path(request.requesting_device, request.resource_owner),
            }
        }
        Some(trust_level) => {
            AuthorizationDecision::Deny {
                reason: format!("Insufficient trust: {} < {}", trust_level, required_trust),
                computed_trust: trust_level,
            }
        }
        None => {
            AuthorizationDecision::Deny {
                reason: "No trust path found".to_string(),
                computed_trust: TrustLevel::None,
            }
        }
    }
}
```

Authorization evaluation computes trust paths between devices. Requests require minimum trust levels for approval. Authorization decisions include trust evidence for auditing.

## Flow Budget Management

Flow budgets control communication rates and privacy leakage. Budgets prevent spam attacks and limit metadata disclosure through traffic analysis. See [Transport and Information Flow](108_transport_and_information_flow.md) for detailed implementation.

### Budget Structure

Flow budgets track spending per context and peer:

```rust
use aura_core::{FlowBudget, ContextId};

pub struct DeviceFlowBudgets {
    budgets: BTreeMap<(ContextId, aura_core::DeviceId), FlowBudget>,
    epoch: u64,
}

impl DeviceFlowBudgets {
    pub fn charge_budget(
        &mut self,
        context: &ContextId,
        peer: &aura_core::DeviceId,
        cost: u32,
    ) -> Result<FlowBudget, BudgetError> {
        let key = (*context, *peer);
        let budget = self.budgets.get_mut(&key)
            .ok_or(BudgetError::BudgetNotFound)?;

        if budget.remaining() < cost {
            return Err(BudgetError::InsufficientBudget {
                required: cost,
                available: budget.remaining(),
            });
        }

        budget.spend(cost);
        Ok(*budget)
    }
}
```

Budget charging occurs before message transmission. Failed charges prevent unauthorized communication. Budget state persists across device restarts for consistency.

### Epoch Management

Epochs control budget replenishment periods:

```rust
impl DeviceFlowBudgets {
    pub fn advance_epoch(&mut self, new_epoch: u64) -> Result<(), BudgetError> {
        if new_epoch <= self.epoch {
            return Ok(()); // Epoch already advanced
        }

        for budget in self.budgets.values_mut() {
            budget.replenish_for_epoch(new_epoch);
        }

        self.epoch = new_epoch;
        Ok(())
    }

    pub fn merge_epoch_updates(&mut self, peer_epochs: Vec<(aura_core::DeviceId, u64)>) -> Result<(), BudgetError> {
        let max_peer_epoch = peer_epochs.iter().map(|(_, epoch)| *epoch).max().unwrap_or(self.epoch);

        if max_peer_epoch > self.epoch {
            self.advance_epoch(max_peer_epoch)?;
        }

        Ok(())
    }
}
```

Epoch advancement replenishes budget limits. Devices synchronize epochs through peer communication. The highest observed epoch becomes the new local epoch for convergence.

### Privacy Leakage Control

Flow budgets limit privacy leakage through communication patterns:

```rust
use aura_mpst::LeakageBudget;

pub fn evaluate_privacy_cost(
    message: &ProtocolMessage,
    context: &ContextId,
    recipient: &aura_core::DeviceId,
) -> LeakageBudget {
    let message_size = message.serialized_size();
    let timing_leakage = calculate_timing_leakage(&message.message_type);
    let metadata_leakage = calculate_metadata_leakage(context, recipient);

    LeakageBudget {
        external_leakage: metadata_leakage,
        neighbor_leakage: timing_leakage,
        group_leakage: message_size / 1024, // Size-based group leakage
    }
}
```

Privacy costs depend on message content, timing patterns, and recipient relationships. Different context types have different leakage sensitivities. Budget enforcement prevents excessive information disclosure.

## Session Types and Choreographic Programming

[Choreographic programming](107_mpst_and_choreography.md) enables writing distributed protocols from a global perspective. Automatic projection generates local session types for each participant. For theoretical foundations, see [Theoretical Model](002_theoretical_model.md). This ensures protocol compliance and prevents communication errors.

### Rumpsteak-Aura Integration

Aura uses rumpsteak-aura for choreographic protocol implementation. The framework provides DSL parsing, projection, and effect-based execution. Protocol definitions compile to type-safe Rust implementations through `aura-macros`.

```rust
use aura_macros::choreography;

choreography! {
    #[namespace = "simple_ping_pong"]
    protocol PingPong {
        roles: Alice, Bob;
        Alice -> Bob: Ping;
        Bob -> Alice: Pong;
    }
}
```

The `choreography!` macro extracts Aura-specific annotations, then uses rumpsteak-aura's `parse_choreography_str` for parsing and projection. Generated code creates extension effects for each annotation and integrates with rumpsteak-aura's session types. Protocol execution occurs through `AuraHandler` which implements `ChoreoHandler` and bridges to the Aura effect system.

### Session Types from Choreographies

Projection transforms global protocols into local session types automatically. The `aura-macros` crate generates role-specific session types using rumpsteak-aura's projection algorithms.

```rust
// Generated code for each role
type Alice_PingPong = Send<Bob, Ping, Receive<Bob, Pong, End>>;
type Bob_PingPong = Receive<Alice, Ping, Send<Alice, Pong, End>>;
```

Alice's projected type shows send then receive. Bob's projected type shows receive then send. Each role gets their specific protocol view with compile-time type safety.

### Effect-Based Execution

Protocols execute through rumpsteak-aura's interpreter using `AuraHandler`:

```rust
use aura_mpst::{AuraHandler, AuraEndpoint};
use rumpsteak_aura_choreography::effects::interpret_extensible;

let mut handler = AuraHandler::for_testing(device_id)?;
let mut endpoint = AuraEndpoint::new(context_id);

// Execute protocol through rumpsteak-aura interpreter
let result = interpret_extensible(&mut handler, &mut endpoint, program).await?;
```

The `AuraHandler` implements `ChoreoHandler` and `ExtensibleHandler` to bridge rumpsteak-aura session types with Aura's effect system. Extension effects execute automatically for annotated messages, including capability guards, flow budgets, and journal coupling.

### Aura Choreography Extensions

Aura extends rumpsteak-aura with domain-specific annotations for security and performance:

```rust
use aura_macros::choreography;

choreography! {
    #[namespace = "secure_request"]
    protocol SecureRequest {
        roles: Client, Server;

        Client[guard_capability = "send_request", flow_cost = 50]
        -> Server: SendRequest(RequestData);

        Server[guard_capability = "send_response", flow_cost = 30, journal_facts = "response_sent"]
        -> Client: SendResponse(ResponseData);
    }
}
```

Annotations compile to extension effects that execute during protocol operations. The `guard_capability` annotation creates `CapabilityGuardEffect` instances that verify authorization before sending messages. The `flow_cost` annotation creates `FlowCostEffect` instances that charge privacy budgets. The `journal_facts` annotation creates `JournalFactsEffect` instances that update distributed state. All effects integrate seamlessly with rumpsteak-aura's extension system and execute automatically during choreographic operations.

### Protocol Composition

Choreographies compose through effect programs:

```rust
use aura_sync::choreography::anti_entropy::execute_as_requester;
// TODO: Move threshold ceremony into core-aligned primitives; aura-frost is deprecated
// use aura_frost::threshold_ceremony::execute_threshold_ceremony;

let composed_protocol = Program::new()
    .ext(ValidateCapability {
        capability: "coordinate".into(),
        role: Coordinator
    })
    .then(anti_entropy_program)
    .then(threshold_ceremony_program)
    .ext(LogEvent {
        event: "protocols_complete".into()
    })
    .end();
```

Protocol composition enables building complex distributed operations. Extensions provide cross-cutting concerns like validation and logging.

## Integration Patterns

Coordination systems integrate to provide comprehensive distributed functionality. CRDTs handle state consistency. Commitment trees provide forward secrecy. Web of trust enables authorization. Flow budgets control privacy and performance.

Combine these systems for robust distributed applications:

```rust
use aura_agent::{create_production_agent, EffectContext, ExecutionMode};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

pub async fn create_coordinated_application(
    device_id: aura_core::DeviceId,
    config: CoordinationConfig,
) -> Result<CoordinatedApp, ApplicationError> {
    let authority_id = AuthorityId::from_uuid(device_id.0);
    let ctx = EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(hash(&authority_id.to_bytes())),
        ExecutionMode::Production,
    );
    let agent = create_production_agent(&ctx, authority_id).await?;
    agent.initialize().await?;

    let coordination_systems = CoordinationSystems::new(agent, config)?;
    let app = CoordinatedApp::new(coordination_systems);

    Ok(app)
}
```

Integrated coordination provides security, consistency, and privacy guarantees. Applications build on proven distributed systems patterns. See [System Architecture](001_system_architecture.md) for complete system integration.

## Operation Categories: When to Use Ceremonies vs Optimistic Execution

Not all distributed operations require full choreographic coordination. Aura classifies operations into three categories based on their security requirements:

### Category A: Optimistic (Immediate Effect)

Use optimistic execution when:
- Operation is within an **established relational context**
- Keys are already derived (no new key agreement needed)
- Eventual consistency is acceptable
- Worst case is temporary inconsistency, not security breach

```rust
// Category A: Just emit a CRDT fact
async fn send_message(&mut self, channel_id: ChannelId, content: String) {
    // Key already derived from context: KDF(ContextRoot, ChannelId, epoch)
    let encrypted = self.encrypt_for_channel(&channel_id, &content).await;

    // Emit fact - syncs via anti-entropy
    self.emit_fact(ChatFact::Message {
        channel_id,
        content: encrypted,
        timestamp: self.now(),
    }).await;

    // Message visible immediately, delivery status updates in background
}
```

**Examples**: Send message, create channel, react, update topic, block contact

### Category B: Deferred (Pending Until Confirmed)

Use deferred execution when:
- Operation affects other users' access or policies
- Rollback is possible and acceptable
- Some approval threshold is required
- User can see "pending" state while waiting

```rust
// Category B: Create proposal, apply effect on approval
async fn kick_member(&mut self, channel_id: ChannelId, target: AuthorityId) {
    // Don't apply effect yet - create proposal
    let proposal = Proposal {
        operation: Operation::RemoveMember { channel_id, target },
        requires_approval_from: vec![CapabilityRequirement::Role("admin")],
        threshold: ApprovalThreshold::Any,
        timeout_ms: 24 * 60 * 60 * 1000,
    };

    self.emit_proposal(proposal).await;
    // UI shows "Pending: Remove {target}" until approved/rejected
}
```

**Examples**: Change permissions, kick member, transfer ownership, archive channel

### Category C: Consensus-Gated (Blocking Ceremony)

Use ceremonies when:
- Operation establishes **new cryptographic context**
- Partial state would be dangerous or unusable
- Operation is irreversible or security-critical
- All parties must agree before any effect

```rust
// Category C: Must complete ceremony before proceeding
async fn add_contact(&mut self, invitation: Invitation) -> Result<ContactId> {
    // Start invitation ceremony - blocks until complete
    let ceremony_id = self.ceremony_executor
        .initiate_invitation_ceremony(invitation)
        .await?;

    // Cannot use contact until ceremony commits
    // Ceremony establishes shared ContextRoot for key derivation
    match self.await_ceremony(&ceremony_id).await? {
        CeremonyStatus::Committed => Ok(ContactId::from_ceremony(&ceremony_id)),
        CeremonyStatus::Aborted { reason } => Err(AuraError::ceremony_failed(reason)),
    }
}
```

**Examples**: Add contact, create group, add group member, guardian rotation, recovery

### Decision Quick Reference

```
Is this operation establishing new cryptographic relationships?
├─ YES → Category C (ceremony required)
└─ NO: Does it affect other users' access?
       ├─ YES: High-security or irreversible?
       │       ├─ YES → Category B (deferred)
       │       └─ NO → Category A (optimistic)
       └─ NO → Category A (optimistic)
```

### Common Mistakes

| Mistake | Why It's Wrong | Correct Approach |
|---------|----------------|------------------|
| Ceremony for channel creation | Context already exists | Category A - emit fact |
| Optimistic guardian rotation | Partial key shares unusable | Category C - ceremony |
| Blocking on message send | Eventual consistency is fine | Category A - optimistic |
| Immediate permission change | Affects others' access | Category B - deferred |

See [Operation Categories](117_operation_categories.md) for comprehensive documentation.

Continue with [Advanced Choreography Guide](804_advanced_coordination_guide.md) for sophisticated protocol development. Learn comprehensive testing in [Testing Guide](805_testing_guide.md) and [Simulation Guide](806_simulation_guide.md).
