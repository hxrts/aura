# Access Control API Reference

Quick reference for Aura's access control system including authentication, authorization, and capability management. The access control system maintains strict separation between identity verification and permission evaluation.

See [Core Systems Guide](802_core_systems_guide.md) for system architecture. See [Identity System](100_identity_system.md) for threshold identity details.

---

## Authentication API

### Identity Verification

```rust
use aura_verify::{verify_identity_proof, IdentityProof, VerificationContext};

// Verify device signature
pub async fn verify_device_identity<E: CryptoEffects>(
    effects: &E,
    device_id: DeviceId,
    message: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> Result<VerifiedIdentity, VerificationError> {
    let proof = IdentityProof::Device {
        device_id,
        signature: signature.to_vec(),
    };
    
    let context = VerificationContext {
        message: message.to_vec(),
        timestamp: effects.current_timestamp().await,
        verification_type: VerificationType::DeviceSignature,
    };
    
    verify_identity_proof(&proof, &context, public_key)
}

// Verify threshold signature
pub async fn verify_threshold_identity<E: CryptoEffects>(
    effects: &E,
    message: &[u8],
    threshold_signature: &ThresholdSignature,
    tree_commitment: &TreeCommitment,
) -> Result<VerifiedIdentity, VerificationError> {
    let proof = IdentityProof::ThresholdSignature {
        signature: threshold_signature.clone(),
        tree_commitment: tree_commitment.clone(),
        participants: threshold_signature.participants.clone(),
    };
    
    let context = VerificationContext {
        message: message.to_vec(),
        timestamp: effects.current_timestamp().await,
        verification_type: VerificationType::ThresholdSignature,
    };
    
    verify_identity_proof(&proof, &context, &tree_commitment.root_key())
}
```

Identity verification returns cryptographic proofs without policy evaluation. Verification processes validate signatures and return verified identity information.

### Context Identity Derivation

```rust
use aura_core::{DKDCapsule, derive_context_identity};

// Derive context-specific identity
pub async fn derive_device_context<E: CryptoEffects>(
    effects: &E,
    account_key_share: &[u8],
    app_id: &str,
    context: &str,
    device_id: DeviceId,
) -> Result<ContextIdentity, DKDError> {
    let capsule = DKDCapsule {
        app_id: app_id.to_string(),
        context: context.to_string(),
        device_id,
        timestamp: effects.current_timestamp().await,
        ttl: None,
    };
    
    derive_context_identity(effects, account_key_share, &capsule).await
}

// Verify context identity
pub async fn verify_context_identity<E: CryptoEffects>(
    effects: &E,
    context_id: &ContextId,
    capsule: &DKDCapsule,
    mac: &[u8],
) -> Result<bool, VerificationError> {
    let expected_mac = effects.compute_dkd_mac(capsule).await?;
    Ok(effects.constant_time_eq(mac, &expected_mac).await)
}
```

Context identity derivation binds relationship contexts to account roots. Every relationship identifier inherits the same graph state. Context identities scope to account epochs for forward secrecy.

## Authorization API

### Capability Evaluation

```rust
use aura_wot::{CapabilitySet, evaluate_capabilities, AuthorizationRequest};

// Basic capability check
pub fn check_device_capabilities(
    device_capabilities: &CapabilitySet,
    required_capabilities: &CapabilitySet,
) -> Result<PermissionGrant, AuthorizationError> {
    let effective_capabilities = device_capabilities.meet(required_capabilities);
    
    if effective_capabilities.contains_all(required_capabilities) {
        Ok(PermissionGrant {
            granted_capabilities: effective_capabilities,
            expires_at: None,
            conditions: Vec::new(),
        })
    } else {
        let missing = required_capabilities.difference(device_capabilities);
        Err(AuthorizationError::InsufficientCapabilities { missing })
    }
}

// Multi-device capability evaluation
pub fn evaluate_multi_device_capabilities(
    device_capabilities: &BTreeMap<DeviceId, CapabilitySet>,
    required_capabilities: &CapabilitySet,
    threshold: usize,
) -> Result<PermissionGrant, AuthorizationError> {
    if device_capabilities.len() < threshold {
        return Err(AuthorizationError::InsufficientDevices {
            required: threshold,
            available: device_capabilities.len(),
        });
    }
    
    let mut effective_capabilities = CapabilitySet::universal();
    let mut participating_devices = Vec::new();
    
    for (device_id, capabilities) in device_capabilities {
        if capabilities.contains_all(required_capabilities) {
            effective_capabilities = effective_capabilities.meet(capabilities);
            participating_devices.push(*device_id);
            
            if participating_devices.len() >= threshold {
                break;
            }
        }
    }
    
    if participating_devices.len() >= threshold {
        Ok(PermissionGrant {
            granted_capabilities: effective_capabilities,
            expires_at: None,
            conditions: vec![Condition::RequiredParticipants(participating_devices)],
        })
    } else {
        Err(AuthorizationError::InsufficientAuthorizedDevices)
    }
}
```

The authorization system uses meet-semilattice operations for capability evaluation. Capabilities can only shrink through intersection operations, providing conservative security decisions.

### Operation Authorization

```rust
use aura_wot::{Operation, AuthorizationContext, PolicySet};

// Authorize specific operation
pub async fn authorize_operation<E: EffectSystem>(
    effects: &E,
    device_id: DeviceId,
    operation: &Operation,
    context: &AuthorizationContext,
) -> Result<PermissionGrant, AuthorizationError> {
    // Get device capabilities from journal
    let device_capabilities = effects.get_device_capabilities(device_id).await?;
    
    // Check base capability requirements
    let required_capabilities = operation.required_capabilities();
    check_device_capabilities(&device_capabilities, &required_capabilities)?;
    
    // Apply contextual policies
    let policies = effects.get_applicable_policies(operation, context).await?;
    for policy in policies {
        policy.evaluate(device_id, operation, context)?;
    }
    
    Ok(PermissionGrant {
        granted_capabilities: device_capabilities.meet(&required_capabilities),
        expires_at: Some(context.timestamp + policy.max_duration()),
        conditions: policy.conditions(),
    })
}

// Authorize with delegation chain
pub async fn authorize_with_delegation<E: EffectSystem>(
    effects: &E,
    delegation_chain: &DelegationChain,
    operation: &Operation,
    context: &AuthorizationContext,
) -> Result<PermissionGrant, AuthorizationError> {
    // Validate delegation chain
    delegation_chain.validate(effects).await?;
    
    // Get effective capabilities from chain
    let effective_capabilities = delegation_chain.compute_effective_capabilities();
    
    // Check operation requirements
    let required_capabilities = operation.required_capabilities();
    check_device_capabilities(&effective_capabilities, &required_capabilities)?;
    
    Ok(PermissionGrant {
        granted_capabilities: effective_capabilities.meet(&required_capabilities),
        expires_at: delegation_chain.earliest_expiry(),
        conditions: delegation_chain.combined_conditions(),
    })
}
```

Operation authorization combines capability checking with policy evaluation. Contextual policies can impose additional constraints beyond basic capability requirements.

## Capability Management

### Capability Operations

```rust
use aura_wot::{Capability, CapabilityConstraint, CapabilityType};

// Create capability set
pub fn create_capability_set(capabilities: &[Capability]) -> CapabilitySet {
    CapabilitySet::from_capabilities(capabilities)
}

// Capability intersection (meet operation)
pub fn restrict_capabilities(
    base_capabilities: &CapabilitySet,
    allowed_capabilities: &CapabilitySet,
) -> CapabilitySet {
    base_capabilities.meet(allowed_capabilities)
}

// Capability difference
pub fn missing_capabilities(
    required: &CapabilitySet,
    available: &CapabilitySet,
) -> CapabilitySet {
    required.difference(available)
}

// Capability constraints
pub fn apply_constraints(
    capabilities: &CapabilitySet,
    constraints: &[CapabilityConstraint],
) -> Result<CapabilitySet, ConstraintError> {
    let mut constrained = capabilities.clone();
    
    for constraint in constraints {
        constrained = constraint.apply(&constrained)?;
    }
    
    Ok(constrained)
}
```

Capability operations use meet-semilattice properties for secure composition. All operations preserve the monotonic restriction property.

### Delegation Management

```rust
use aura_wot::{DelegationToken, DelegationCondition, TrustLevel};

// Create basic delegation
pub fn create_delegation(
    delegator_capabilities: &CapabilitySet,
    delegated_capabilities: &CapabilitySet,
    delegatee: DeviceId,
    conditions: Vec<DelegationCondition>,
    expiry: Option<SystemTime>,
) -> Result<DelegationToken, DelegationError> {
    // Verify delegator authority
    if !delegator_capabilities.contains_all(delegated_capabilities) {
        return Err(DelegationError::InsufficientAuthority);
    }
    
    // Apply capability restriction
    let effective_capabilities = delegator_capabilities.meet(delegated_capabilities);
    
    DelegationToken::new(
        effective_capabilities,
        delegatee,
        conditions,
        expiry,
    )
}

// Trust-based delegation with attenuation
pub fn create_trust_delegation(
    delegator_capabilities: &CapabilitySet,
    delegated_capabilities: &CapabilitySet,
    delegatee: DeviceId,
    trust_level: TrustLevel,
    conditions: Vec<DelegationCondition>,
) -> Result<DelegationToken, DelegationError> {
    let base_delegation = create_delegation(
        delegator_capabilities,
        delegated_capabilities,
        delegatee,
        conditions,
        None,
    )?;
    
    // Apply trust-based attenuation
    let attenuated_capabilities = base_delegation.capabilities().attenuate(trust_level);
    
    Ok(base_delegation.with_capabilities(attenuated_capabilities))
}

// Revoke delegation
pub async fn revoke_delegation<E: JournalEffects>(
    effects: &E,
    delegation_id: &DelegationId,
    revoking_device: DeviceId,
    reason: RevocationReason,
) -> Result<(), RevocationError> {
    let delegation = effects.get_delegation(delegation_id).await?;
    
    // Verify revocation authority
    if !delegation.can_be_revoked_by(revoking_device) {
        return Err(RevocationError::InsufficientAuthority);
    }
    
    // Record revocation in journal
    let revocation_fact = RevocationFact {
        delegation_id: *delegation_id,
        revoking_device,
        reason,
        timestamp: effects.current_timestamp().await,
    };
    
    effects.add_fact(revocation_fact.into()).await?;
    
    Ok(())
}
```

Delegation management enables transferring limited authority between devices. Trust-based attenuation reduces capabilities based on relationship strength.

## Guard Integration

### Guard Chains

```rust
use aura_protocol::guards::{GuardChain, GuardResult, CapabilityGuard, FlowGuard};

// Create capability guard
pub fn create_capability_guard(required_capabilities: CapabilitySet) -> CapabilityGuard {
    CapabilityGuard::new(required_capabilities)
}

// Create flow budget guard
pub fn create_flow_guard(
    context_id: ContextId,
    cost: u32,
    budget_type: FlowBudgetType,
) -> FlowGuard {
    FlowGuard::new(context_id, cost, budget_type)
}

// Execute guard chain
pub async fn execute_guard_chain<E: EffectSystem>(
    effects: &E,
    guards: &[Box<dyn Guard>],
    context: &GuardContext,
) -> Result<GuardResult, GuardError> {
    for guard in guards {
        let result = guard.evaluate(context, effects).await?;
        
        match result {
            GuardResult::Allow => continue,
            GuardResult::Deny(reason) => return Ok(GuardResult::Deny(reason)),
            GuardResult::Defer => return Ok(GuardResult::Defer),
        }
    }
    
    Ok(GuardResult::Allow)
}

// Common guard combinations
pub fn create_operation_guards(
    required_capabilities: CapabilitySet,
    flow_cost: u32,
    context_id: ContextId,
) -> Vec<Box<dyn Guard>> {
    vec![
        Box::new(create_capability_guard(required_capabilities)),
        Box::new(create_flow_guard(context_id, flow_cost, FlowBudgetType::Communication)),
    ]
}
```

Guards enforce access control requirements as part of the authorization system. Guard chains enable composing multiple authorization checks.

## Access Control Integration

### Complete Authorization Flow

```rust
use aura_protocol::access_control::AccessControlBridge;

// Complete access control evaluation
pub async fn evaluate_access_control<E: EffectSystem>(
    effects: &E,
    identity_proof: IdentityProof,
    operation: &Operation,
    context: &AuthorizationContext,
) -> Result<AccessGrant, AccessControlError> {
    // Step 1: Verify identity
    let verified_identity = verify_identity_proof(
        &identity_proof,
        &context.verification_context(),
        &context.public_key_material(),
    )?;
    
    // Step 2: Get device capabilities
    let device_capabilities = effects.get_device_capabilities(verified_identity.device_id).await?;
    
    // Step 3: Check operation authorization
    let permission_grant = authorize_operation(
        effects,
        verified_identity.device_id,
        operation,
        context,
    ).await?;
    
    // Step 4: Execute guard chain
    let guard_context = GuardContext::new(
        verified_identity.device_id,
        device_capabilities,
        operation.clone(),
    );
    
    let guards = create_operation_guards(
        operation.required_capabilities(),
        operation.flow_cost(),
        context.context_id,
    );
    
    let guard_result = execute_guard_chain(effects, &guards, &guard_context).await?;
    
    match guard_result {
        GuardResult::Allow => Ok(AccessGrant {
            identity: verified_identity,
            permissions: permission_grant,
            valid_until: context.timestamp + Duration::from_secs(3600),
        }),
        GuardResult::Deny(reason) => Err(AccessControlError::Denied { reason }),
        GuardResult::Defer => Err(AccessControlError::Deferred),
    }
}

// Simplified access control for common operations
pub async fn check_operation_access<E: EffectSystem>(
    effects: &E,
    device_id: DeviceId,
    operation_type: OperationType,
    context_id: ContextId,
) -> Result<bool, AccessControlError> {
    let device_capabilities = effects.get_device_capabilities(device_id).await?;
    let required_capabilities = operation_type.required_capabilities();
    
    // Check capabilities
    if !device_capabilities.contains_all(&required_capabilities) {
        return Ok(false);
    }
    
    // Check flow budget
    let flow_cost = operation_type.flow_cost();
    let current_budget = effects.get_flow_budget(context_id, device_id).await?;
    
    if current_budget.remaining() < flow_cost {
        return Ok(false);
    }
    
    Ok(true)
}
```

The access control bridge combines authentication and authorization without coupling them. Identity verification occurs first, followed by capability evaluation and guard execution.

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum AccessControlError {
    #[error("Identity verification failed: {reason}")]
    VerificationFailed { reason: String },
    
    #[error("Insufficient capabilities: missing {missing:?}")]
    InsufficientCapabilities { missing: Vec<Capability> },
    
    #[error("Access denied: {reason}")]
    Denied { reason: String },
    
    #[error("Access check deferred")]
    Deferred,
    
    #[error("Delegation error: {0}")]
    Delegation(#[from] DelegationError),
    
    #[error("Guard error: {0}")]
    Guard(#[from] GuardError),
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Expired proof")]
    ExpiredProof,
    
    #[error("Unknown device: {device_id}")]
    UnknownDevice { device_id: DeviceId },
    
    #[error("Tree commitment mismatch")]
    TreeCommitmentMismatch,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("Insufficient capabilities")]
    InsufficientCapabilities { missing: Vec<Capability> },
    
    #[error("Policy violation: {policy_id}")]
    PolicyViolation { policy_id: String },
    
    #[error("Operation not allowed in context")]
    ContextDenied,
    
    #[error("Device not authorized")]
    DeviceNotAuthorized,
}
```

Standardized error handling across authentication and authorization components provides clear failure information and audit trails.