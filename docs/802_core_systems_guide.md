# Core Systems Guide

This guide covers the foundation systems that enable distributed applications in Aura. You will learn effect system composition, authentication and authorization patterns, capability-based access control, and journal fundamentals.

## Effect System Architecture

Aura's effect system separates application logic from infrastructure implementation. Effects define what your application needs. Handlers provide concrete implementations for different environments.

The effect system follows a clean layered architecture. Interface traits in `aura-core` define capabilities. Standard implementations in `aura-effects` provide basic handlers. Coordination primitives in `aura-protocol` handle multi-party operations.

### Stateless Effect System

The effect system uses a unified stateless architecture that eliminates shared mutable state:

```rust
use aura_protocol::{AuraEffectSystem, effects::EffectSystemConfig};
use aura_core::DeviceId;

// Create a stateless effect system for production
let device_id = DeviceId::new();
let config = EffectSystemConfig::for_production(device_id)
    .expect("Failed to create production configuration");
let effect_system = AuraEffectSystem::new(config)
    .expect("Failed to initialize effect system");

// All effect operations go through the unified system
let data = b"hello world";
let hash = effect_system.hash(data).await?;
effect_system.store("key", data.to_vec()).await?;
effect_system.send_to_peer(peer_id, message).await?;
```

The stateless architecture provides deadlock freedom through isolated state services. All handlers are context-free and operate without device-specific shared state. Configuration determines whether to use production or testing implementations.

Use sealed supertraits to define protocol-specific effect interfaces:

```rust
/// Protocol effects for data synchronization
pub trait DataSyncEffects: NetworkEffects + StorageEffects + CryptoEffects {}
impl<T> DataSyncEffects for T where T: NetworkEffects + StorageEffects + CryptoEffects {}

pub async fn execute_data_sync<E: DataSyncEffects>(
    effects: &E,
    data: SyncData,
    peers: Vec<aura_core::DeviceId>,
) -> Result<SyncResult, SyncError> {
    let hash = effects.hash(&data.content).await?;
    
    for peer in peers {
        let sync_message = SyncMessage { data: data.clone(), hash };
        let serialized = bincode::serialize(&sync_message)?;
        effects.send_to_peer(peer.into(), serialized).await?;
    }
    
    Ok(SyncResult::Success)
}
```

Sealed supertraits provide clean type signatures and better error messages. They enable protocol-specific extensions while maintaining flexibility.

### Testing Configuration

Use testing configuration for deterministic test execution:

```rust
use aura_protocol::{AuraEffectSystem, effects::EffectSystemConfig};

// Create a stateless effect system for testing
let device_id = DeviceId::new();
let config = EffectSystemConfig::for_testing(device_id);
let effect_system = AuraEffectSystem::new(config)
    .expect("Failed to initialize test effect system");

// Testing operations are deterministic and isolated
let test_data = b"test data";
let hash1 = effect_system.hash(test_data).await?;
let hash2 = effect_system.hash(test_data).await?;
assert_eq!(hash1, hash2); // Deterministic in testing mode

// Test error injection through configuration
let failing_config = EffectSystemConfig::for_testing(device_id)
    .with_crypto_failures(vec!["ed25519_sign"]);
let failing_system = AuraEffectSystem::new(failing_config)?;
```

Testing configuration provides mock implementations that eliminate external dependencies and enable deterministic test execution. Error injection capabilities support comprehensive testing of failure scenarios.


## Identity System

Aura implements threshold identity where accounts are relational identities anchored in the journal. Every device and guardian occupies a leaf in a ratchet tree with threshold policies stored at branches.

### Account Structure

Account state is managed through attested tree operations:

```rust
use aura_core::{DeviceId, AccountId};
use aura_journal::ratchet_tree::{TreeSnapshot, DeviceLeaf, GuardianLeaf};

pub struct AccountState {
    tree_snapshot: TreeSnapshot,
    devices: BTreeMap<DeviceId, DeviceLeaf>,
    guardians: BTreeMap<DeviceId, GuardianLeaf>,
    epoch: u64,
}

impl AccountState {
    pub fn verify_tree_commitment(&self) -> Result<bool, VerificationError> {
        let computed_root = self.compute_tree_root()?;
        Ok(computed_root == self.tree_snapshot.commitment)
    }
    
    pub fn get_signing_threshold(&self, policy_id: &PolicyId) -> Option<usize> {
        self.tree_snapshot.get_policy(policy_id).map(|p| p.threshold)
    }
}
```

Tree operations create attested facts in the journal. No device edits account state directly. Reduction of the operation log yields canonical state that all devices verify independently.

### Deterministic Key Derivation

Context identities derive from account keys using deterministic key derivation:

```rust
use aura_crypto::{DKDCapsule, ContextId};

pub async fn derive_context_identity<E: CryptoEffects>(
    effects: &E,
    account_key_share: &[u8],
    app_id: &str,
    context: &str,
    device_id: DeviceId,
) -> Result<ContextId, DKDError> {
    let capsule = DKDCapsule {
        app_id: app_id.to_string(),
        context: context.to_string(),
        device_id,
        timestamp: effects.current_timestamp().await,
    };
    
    let capsule_hash = effects.hash(&capsule.to_cbor()?).await?;
    let derived_key = effects.derive_key(account_key_share, &capsule_hash).await?;
    
    Ok(ContextId::from_key_material(derived_key))
}
```

DKD binds relationship contexts to account roots. Every relationship identifier inherits the same graph state. Context identities scope to account epochs for forward secrecy.

### Threshold Signatures

Account operations require threshold signatures from eligible devices:

```rust
use aura_frost::{FrostShare, ThresholdSignResult};

pub async fn execute_threshold_signing<E: CryptoEffects>(
    effects: &E,
    message: &[u8],
    shares: Vec<FrostShare>,
    threshold: usize,
) -> Result<ThresholdSignResult, SigningError> {
    if shares.len() < threshold {
        return Err(SigningError::InsufficientShares);
    }
    
    let nonces = generate_signing_nonces(&shares)?;
    let partial_signatures = compute_partial_signatures(message, &shares, &nonces)?;
    let aggregate_signature = aggregate_signatures(&partial_signatures)?;
    
    Ok(ThresholdSignResult {
        signature: aggregate_signature,
        participants: shares.iter().map(|s| s.device_id).collect(),
        tree_commitment: compute_tree_commitment(&shares)?,
    })
}
```

FROST threshold signatures bind to tree commitments and policy identifiers. Honest devices refuse to sign mismatched tree states. This prevents forked policies from producing valid signatures.

## Access Control System

Aura's access control system maintains strict separation between authentication (WHO) and authorization (WHAT). Authentication verifies identity through cryptography. Authorization evaluates capabilities based on policy.

### Authentication Layer

The authentication layer handles identity verification:

```rust
use aura_verify::{verify_identity_proof, IdentityProof};

pub fn verify_device_signature(
    device_id: aura_core::DeviceId,
    message: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> Result<bool, VerificationError> {
    let proof = IdentityProof::Device {
        device_id,
        signature: signature.to_vec(),
    };
    
    let verified_identity = verify_identity_proof(&proof, message, public_key)?;
    
    Ok(verified_identity.device_id == Some(device_id))
}
```

Authentication provides cryptographic identity verification without policy knowledge. The verification process validates signatures and returns verified identity information.

### Authorization and Capabilities

The authorization subsystem evaluates capability-based permissions:

```rust
use aura_wot::{CapabilitySet, evaluate_tree_operation_capabilities};

pub fn check_operation_permission(
    operation: &TreeOperation,
    device_capabilities: &CapabilitySet,
    required_capabilities: &CapabilitySet,
) -> Result<bool, AuthorizationError> {
    let permission_grant = evaluate_tree_operation_capabilities(
        operation,
        device_capabilities,
        required_capabilities,
    )?;
    
    Ok(permission_grant.is_authorized())
}
```

The authorization subsystem uses meet-semilattice operations for capability evaluation. Capabilities can only shrink through intersection operations, providing conservative security decisions.

### Access Control Integration

The access control bridge combines authentication and authorization:

```rust
use aura_protocol::authorization_bridge::authenticate_and_authorize;

pub async fn authorize_device_operation(
    identity_proof: IdentityProof,
    message: &[u8],
    operation: TreeOperation,
    context: AuthorizationContext,
) -> Result<PermissionGrant, AuthorizationError> {
    let permission = authenticate_and_authorize(
        identity_proof,
        message,
        &context.key_material,
        context.authorization_context,
        operation,
        context.additional_signers,
        context.guardian_signers,
    )?;
    
    Ok(permission)
}
```

The bridge orchestrates both layers without coupling them. Identity verification occurs first. Capability evaluation uses the verified identity for authorization decisions.

Capabilities are permission tokens that can be delegated and composed. They support intersection (meet) operations:

```rust
use aura_wot::CapabilitySet;

pub fn compute_effective_capabilities(
    user_capabilities: &CapabilitySet,
    context_capabilities: &CapabilitySet,
    operation_requirements: &CapabilitySet,
) -> Result<CapabilitySet, CapabilityError> {
    let effective_caps = user_capabilities
        .meet(context_capabilities)
        .meet(operation_requirements);
    
    if effective_caps.is_empty() {
        return Err(CapabilityError::InsufficientCapabilities);
    }
    
    Ok(effective_caps)
}
```

The meet operation ensures capabilities can only be restricted. This provides conservative security where operations require explicit authorization. Empty capability sets deny all operations.

#### Delegation Chains

Capabilities can be delegated through trust relationships:

```rust
use aura_wot::{DelegationChain, TrustLevel};

pub fn evaluate_delegated_capability(
    capability: &Capability,
    delegation_chain: &DelegationChain,
    max_delegation_depth: usize,
) -> Result<Option<Capability>, DelegationError> {
    if delegation_chain.length() > max_delegation_depth {
        return Ok(None);
    }
    
    let mut attenuated_capability = capability.clone();
    
    for delegation in delegation_chain.delegations() {
        let trust_factor = delegation.trust_level.to_factor();
        attenuated_capability = attenuated_capability.attenuate(trust_factor);
        
        if attenuated_capability.strength() < MINIMUM_CAPABILITY_STRENGTH {
            return Ok(None);
        }
    }
    
    Ok(Some(attenuated_capability))
}
```

Delegation chains attenuate capabilities based on trust levels. Longer chains produce weaker capabilities. Minimum strength thresholds prevent excessive delegation.

#### Guard Integration

Capabilities integrate with guard chains for runtime enforcement:

```rust
use aura_protocol::guards::{CapGuard, GuardResult};

pub struct CapabilityGuard {
    required_capabilities: CapabilitySet,
}

impl CapGuard for CapabilityGuard {
    async fn check_capability<E: Effects>(
        &self,
        context: &GuardContext,
        effects: &E,
    ) -> Result<GuardResult, GuardError> {
        let device_capabilities = context.device_capabilities();
        let operation_capabilities = &self.required_capabilities;
        
        let effective_capabilities = device_capabilities.meet(operation_capabilities);
        
        if effective_capabilities.contains_all(operation_capabilities) {
            Ok(GuardResult::Allow)
        } else {
            Ok(GuardResult::Deny("Insufficient capabilities".to_string()))
        }
    }
}
```

Guards enforce capability requirements as part of the authorization subsystem. Failed capability checks prevent unauthorized actions. Guard results provide audit information for security monitoring.

## Journal System Fundamentals

The journal system manages distributed state using Conflict-free Replicated Data Types (CRDTs). The journal ensures eventual consistency across devices without coordination protocols.

### Journal Structure

The journal contains facts and capabilities:

```rust
use aura_core::{Journal, FactSet, CapabilitySet};

pub struct DeviceJournal {
    facts: FactSet,
    capabilities: CapabilitySet,
    version: u64,
    device_id: aura_core::DeviceId,
}

impl DeviceJournal {
    pub fn merge_journal(&mut self, remote_journal: &DeviceJournal) -> Result<(), JournalError> {
        self.facts = self.facts.join(&remote_journal.facts);
        self.capabilities = self.capabilities.meet(&remote_journal.capabilities);
        self.version = self.version.max(remote_journal.version);
        
        Ok(())
    }
}
```

Facts accumulate through join operations. Capabilities refine through meet operations. Version vectors track causal ordering for conflict resolution.


## Integration Patterns

Core systems integrate through well-defined interfaces. Effect handlers compose for different environments. Authentication and authorization bridge cleanly. Journal operations provide distributed consistency.

Create production applications by combining these systems:

```rust
use aura_agent::AuraAgent;

pub async fn create_production_application(
    device_id: aura_core::DeviceId,
    config: ApplicationConfig,
) -> Result<AuraApplication, ApplicationError> {
    let agent = AuraAgent::for_production(device_id)?;
    agent.initialize().await?;
    
    let application = AuraApplication::new(agent, config);
    
    Ok(application)
}
```

The agent provides complete runtime composition. Applications build on top of integrated core systems. This pattern provides security and consistency guarantees.

Continue with [Coordination Systems Guide](803_coordination_systems_guide.md) for distributed coordination patterns. Learn advanced techniques in [Advanced Choreography Guide](804_advanced_choreography_guide.md).