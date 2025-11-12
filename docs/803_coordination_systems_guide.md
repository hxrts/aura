# Coordination Systems Guide

This guide covers distributed coordination and privacy systems in Aura. You will learn CRDT programming patterns, ratchet tree operations, web of trust relationships, flow budget management, session types, and basic choreographic programming.

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
use aura_journal::semilattice::{Join, GCounterState};

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
use aura_wot::CapabilitySet;

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
use aura_protocol::choreography::protocols::anti_entropy::execute_anti_entropy;

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

## Ratchet Tree Operations

Ratchet trees provide forward secrecy for group communication. The tree structure enables efficient key updates when group membership changes.

### Tree Structure

Ratchet trees organize group members in a binary tree:

```rust
use aura_journal::ratchet_tree::{RatchetTree, TreePosition};

pub struct GroupRatchetTree {
    tree: RatchetTree,
    device_id: aura_core::DeviceId,
    position: TreePosition,
}

impl GroupRatchetTree {
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
use aura_crypto::DerivedKeyContext;

impl GroupRatchetTree {
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
use aura_journal::ratchet_tree::TreeOperation;

pub async fn synchronize_ratchet_tree<E: NetworkEffects + CryptoEffects>(
    effects: &E,
    local_tree: &mut GroupRatchetTree,
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

Establish trust relationships through invitation protocols:

```rust
use aura_invitation::relationship_formation::{RelationshipFormationConfig, execute_relationship_formation};

pub async fn form_trust_relationship<E: RelationshipFormationEffects>(
    effects: &E,
    config: RelationshipFormationConfig,
) -> Result<TrustRelationship, RelationshipError> {
    let relationship = execute_relationship_formation(
        config.initiator_id,
        config.responder_id,
        config,
        effects,
    ).await?;

    Ok(relationship)
}
```

Relationship formation requires mutual consent from both devices. The protocol establishes shared cryptographic material and initial trust levels. Successful formation creates bidirectional trust records.

### Trust Computation

Compute trust levels using direct and transitive relationships:

```rust
use aura_wot::{TrustGraph, TrustLevel};

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
use aura_wot::{AuthorizationRequest, AuthorizationDecision};

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

Flow budgets control communication rates and privacy leakage. Budgets prevent spam attacks and limit metadata disclosure through traffic analysis.

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

## Session Types and Basic Choreography

Session types provide compile-time guarantees about protocol communication patterns. Choreographic programming specifies protocols from global perspectives with automatic projection to local implementations.

### Session Type Basics

Session types track protocol state transitions:

```rust
use aura_mpst::{SessionType, Send, Receive, End};

// Client session: Send request, receive response, end
type ClientSession = Send<RequestData, Receive<ResponseData, End>>;

// Server session: Receive request, send response, end
type ServerSession = Receive<RequestData, Send<ResponseData, End>>;
```

Session types encode communication protocols in the type system. Type checking prevents protocol violations like sending when expecting to receive. Session progression ensures protocol completion.

### Basic Choreography

Define simple choreographies using the macro system:

```rust
use aura_macros::aura_choreography;

/// Sealed supertrait for request-response effects
pub trait RequestResponseEffects: NetworkEffects + TimeEffects + JournalEffects {}
impl<T> RequestResponseEffects for T where T: NetworkEffects + TimeEffects + JournalEffects {}

aura_choreography! {
    #[namespace = "request_response"]
    protocol RequestResponse {
        roles: Client, Server;

        Client[guard_capability = "send_request", flow_cost = 50]
        -> Server: SendRequest(RequestData);

        Server[guard_capability = "send_response", flow_cost = 30, journal_facts = "response_sent"]
        -> Client: SendResponse(ResponseData);
    }
}
```

Choreographies specify global protocol behavior. Annotations control security (guard capabilities), performance (flow costs), and state management (journal facts). The macro generates type-safe implementations.

### Protocol Execution

Execute choreographic protocols using effect handlers:

```rust
pub async fn execute_request_protocol<E: RequestResponseEffects>(
    effects: &E,
    request: RequestData,
    server_device: aura_core::DeviceId,
) -> Result<ResponseData, ProtocolError> {
    let request_bytes = serde_json::to_vec(&request)?;
    effects.send_to_peer(server_device.into(), request_bytes).await?;

    let (peer_id, response_bytes) = effects.receive().await?;
    let response: ResponseData = serde_json::from_slice(&response_bytes)?;

    Ok(response)
}
```

Protocol execution uses effect handlers for infrastructure access. Serialization handles message encoding. Error handling manages communication failures and timeouts.

## Integration Patterns

Coordination systems integrate to provide comprehensive distributed functionality. CRDTs handle state consistency. Ratchet trees provide forward secrecy. Web of trust enables authorization. Flow budgets control privacy and performance.

Combine these systems for robust distributed applications:

```rust
use aura_agent::AuraAgent;

pub async fn create_coordinated_application(
    device_id: aura_core::DeviceId,
    config: CoordinationConfig,
) -> Result<CoordinatedApp, ApplicationError> {
    let agent = AuraAgent::for_production(device_id)?;
    agent.initialize().await?;

    let coordination_systems = CoordinationSystems::new(agent, config)?;
    let app = CoordinatedApp::new(coordination_systems);

    Ok(app)
}
```

Integrated coordination provides security, consistency, and privacy guarantees. Applications build on proven distributed systems patterns.

Continue with [Advanced Choreography Guide](804_advanced_choreography_guide.md) for sophisticated protocol development. Learn comprehensive testing in [Simulation and Testing Guide](805_simulation_and_testing_guide.md).
