# Capability System

Aura implements capability-based access control using meet-semilattice operations to ensure authorization security. Capabilities can only be restricted through delegation, never expanded. The capability system integrates with Web of Trust evaluation and threshold signature authorization.

Capability operations follow mathematical laws that prevent privilege escalation. All capability decisions use meet-semilattice intersection to compute effective permissions. The system provides formal verification properties for security analysis.

See [Web of Trust](200_web_of_trust.md) for trust evaluation. See [Authentication vs Authorization Architecture](101_auth_authz_system.md) for integration patterns.

---

## Capability Model

**Capability Sets** represent collections of permissions that enable specific operations. Capability sets follow meet-semilattice laws with intersection as the meet operation. No operation can expand capability sets beyond their original scope.

```rust
pub struct CapabilitySet {
    capabilities: BTreeSet<Capability>,
}

impl CapabilitySet {
    pub fn meet(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self.capabilities
                .intersection(&other.capabilities)
                .cloned()
                .collect()
        }
    }

    pub fn contains(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    pub fn is_subset(&self, other: &CapabilitySet) -> bool {
        self.capabilities.is_subset(&other.capabilities)
    }
}
```

Capability sets implement meet operations that compute intersection of permissions. This mathematical property ensures capabilities can only shrink through delegation chains.

**Capability Types** define specific permissions for different operation categories. Each capability type corresponds to a specific class of operations that require authorization.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    // Tree operation capabilities
    TreeRead,
    TreeWrite,
    TreeAdmin,
    
    // Storage capabilities  
    StorageRead,
    StorageWrite,
    StorageDelete,
    
    // Communication capabilities
    MessageSend,
    MessageReceive,
    RelayTraffic,
    
    // Recovery capabilities
    GuardianApproval,
    EmergencyOverride,
    PolicyModification,
    
    // System capabilities
    DeviceRegistration,
    NetworkConfiguration,
    SecurityAudit,
}
```

Capability types provide fine-grained permission control for different system components. The hierarchy enables building complex authorization policies from basic capability primitives.

**Capability Context** provides environmental information for capability evaluation. Context includes the requesting device, operation target, temporal constraints, and network conditions.

```rust
pub struct CapabilityContext {
    pub requesting_device: DeviceId,
    pub target_resource: ResourceId,
    pub operation_type: OperationType,
    pub timestamp: u64,
    pub network_location: Option<NetworkLocation>,
    pub emergency_override: bool,
}
```

Context information enables dynamic capability evaluation based on environmental factors. Policies can adjust authorization decisions based on context conditions.

## Delegation Chains

**Capability Delegation** enables transferring limited authority from one device to another. Delegation creates chains where each link can only restrict capabilities further. No delegation can expand capabilities beyond the original grant.

```rust
pub struct DelegationChain {
    pub original_grant: CapabilitySet,
    pub chain_links: Vec<DelegationLink>,
}

pub struct DelegationLink {
    pub delegator: DeviceId,
    pub delegatee: DeviceId,
    pub restricted_capabilities: CapabilitySet,
    pub expiration: Option<SystemTime>,
    pub conditions: Vec<DelegationCondition>,
}

impl DelegationChain {
    pub fn effective_capabilities(&self) -> CapabilitySet {
        let mut effective = self.original_grant.clone();
        
        for link in &self.chain_links {
            effective = effective.meet(&link.restricted_capabilities);
        }
        
        effective
    }
}
```

Delegation chains compute effective capabilities through sequential meet operations. Each delegation step further restricts the available permissions.

**Delegation Validation** ensures all chain links are cryptographically valid and properly authorized. Validation checks signatures, expiration times, and delegation authority at each step.

```rust
pub fn validate_delegation_chain(
    chain: &DelegationChain,
    verify_effects: &dyn VerificationEffects,
) -> Result<(), DelegationError> {
    let mut current_capabilities = chain.original_grant.clone();
    
    for link in &chain.chain_links {
        // Verify delegator has authority to delegate
        if !current_capabilities.contains_all(&link.restricted_capabilities) {
            return Err(DelegationError::InsufficientAuthority);
        }
        
        // Verify cryptographic signature
        verify_effects.verify_delegation_signature(
            &link.delegator,
            &link.delegatee,
            &link.restricted_capabilities,
            &link.signature,
        )?;
        
        // Check expiration
        if link.is_expired() {
            return Err(DelegationError::ExpiredDelegation);
        }
        
        // Update current capabilities for next iteration
        current_capabilities = current_capabilities.meet(&link.restricted_capabilities);
    }
    
    Ok(())
}
```

Delegation validation ensures cryptographic integrity and proper authorization at each step. Invalid delegations are rejected to prevent unauthorized access.

**Revocation Mechanisms** enable canceling delegations when trust relationships change. Revocation provides immediate termination of delegated authority for security purposes.

```rust
pub struct DelegationRevocation {
    pub delegation_id: DelegationId,
    pub revoking_device: DeviceId,
    pub revocation_timestamp: u64,
    pub revocation_signature: Signature,
    pub reason: RevocationReason,
}

pub enum RevocationReason {
    TrustViolation,
    SecurityBreach,
    PolicyChange,
    ExplicitRequest,
}
```

Revocation records provide permanent evidence of delegation cancellation. Revoked delegations cannot be used for future authorization decisions.

## Policy Enforcement

**Policy Rules** define authorization requirements using capability and context predicates. Policies specify minimum capability requirements and environmental conditions for operation approval. Capability-based authorization works in conjunction with privacy and flow controls described in [Privacy and Information Flow Model](004_information_flow_model.md).

```rust
pub struct PolicyRule {
    pub rule_id: String,
    pub required_capabilities: CapabilitySet,
    pub context_constraints: Vec<ContextConstraint>,
    pub trust_requirements: TrustRequirement,
    pub temporal_constraints: Option<TemporalConstraint>,
}

pub struct ContextConstraint {
    pub constraint_type: ConstraintType,
    pub required_value: ContextValue,
    pub comparison: ComparisonOp,
}

pub enum ConstraintType {
    NetworkLocation,
    TimeOfDay,
    DeviceType,
    SecurityLevel,
}
```

Policy rules combine capability requirements with contextual constraints. This enables sophisticated authorization decisions based on multiple factors.

**Policy Evaluation** determines whether a request should be approved based on available capabilities and policy rules. Evaluation considers all applicable policies and environmental factors.

```rust
pub fn evaluate_authorization_policy(
    request: &AuthorizationRequest,
    available_capabilities: &CapabilitySet,
    context: &CapabilityContext,
    policies: &PolicySet,
) -> Result<AuthorizationDecision, PolicyError> {
    let applicable_policies = policies.find_applicable(
        &request.operation_type,
        &request.target_resource,
    );
    
    for policy in applicable_policies {
        // Check capability requirements
        if !available_capabilities.contains_all(&policy.required_capabilities) {
            return Ok(AuthorizationDecision::Denied {
                reason: DenialReason::InsufficientCapabilities,
                missing_capabilities: policy.required_capabilities
                    .difference(available_capabilities)
                    .collect(),
            });
        }
        
        // Check context constraints
        if !policy.evaluate_context_constraints(context)? {
            return Ok(AuthorizationDecision::Denied {
                reason: DenialReason::ContextViolation,
                violated_constraints: policy.find_violated_constraints(context),
            });
        }
        
        // Check trust requirements
        if !policy.trust_requirements.evaluate(context)? {
            return Ok(AuthorizationDecision::Denied {
                reason: DenialReason::InsufficientTrust,
                required_trust: policy.trust_requirements.clone(),
            });
        }
    }
    
    Ok(AuthorizationDecision::Approved)
}
```

Policy evaluation combines multiple authorization factors to make access control decisions. All applicable policies must be satisfied for approval.

**Emergency Overrides** enable bypassing normal policy restrictions during critical situations. Emergency overrides require elevated authority and create audit trails for security review.

```rust
pub struct EmergencyOverride {
    pub override_id: String,
    pub authorizing_device: DeviceId,
    pub emergency_type: EmergencyType,
    pub override_capabilities: CapabilitySet,
    pub justification: String,
    pub expiration: SystemTime,
    pub authorization_signature: Signature,
}

pub enum EmergencyType {
    SecurityBreach,
    SystemFailure,
    DataRecovery,
    NetworkPartition,
}

pub fn evaluate_emergency_override(
    override_request: &EmergencyOverride,
    authorizing_capabilities: &CapabilitySet,
    emergency_policies: &EmergencyPolicySet,
) -> Result<bool, EmergencyError> {
    // Verify authorizing device has emergency override capability
    if !authorizing_capabilities.contains(&Capability::EmergencyOverride) {
        return Err(EmergencyError::InsufficientAuthority);
    }
    
    // Check emergency type is valid for requested override
    let emergency_policy = emergency_policies
        .get_policy(&override_request.emergency_type)
        .ok_or(EmergencyError::InvalidEmergencyType)?;
    
    // Verify override capabilities are within allowed scope
    if !emergency_policy.allowed_overrides.contains_all(&override_request.override_capabilities) {
        return Err(EmergencyError::ExcessiveOverride);
    }
    
    Ok(true)
}
```

Emergency override evaluation ensures proper authority and justification for policy bypasses. Override decisions create permanent audit records.

## Integration Patterns

**Web of Trust Integration** combines capability-based authorization with trust relationship evaluation. Trust levels influence capability effectiveness and delegation authority.

```rust
pub fn evaluate_trust_weighted_capabilities(
    base_capabilities: &CapabilitySet,
    trust_level: TrustLevel,
    relationship_weight: f64,
) -> CapabilitySet {
    let trust_multiplier = match trust_level {
        TrustLevel::High => 1.0,
        TrustLevel::Medium => 0.8,
        TrustLevel::Low => 0.5,
        TrustLevel::None => 0.0,
    };
    
    let effective_weight = trust_multiplier * relationship_weight;
    
    if effective_weight >= 0.8 {
        base_capabilities.clone()
    } else if effective_weight >= 0.5 {
        base_capabilities.filter_basic_capabilities()
    } else {
        CapabilitySet::minimal()
    }
}
```

Trust-weighted capability evaluation adjusts authorization based on relationship strength. Lower trust reduces effective capabilities even with valid delegation.

**Threshold Signature Integration** enables multi-party authorization for sensitive operations. Threshold requirements ensure no single device can authorize critical operations alone.

```rust
pub struct ThresholdAuthorizationRequest {
    pub operation: Operation,
    pub required_threshold: usize,
    pub guardian_signatures: BTreeMap<GuardianId, GuardianSignature>,
    pub policy_requirements: ThresholdPolicy,
}

pub fn evaluate_threshold_authorization(
    request: &ThresholdAuthorizationRequest,
    guardian_capabilities: &BTreeMap<GuardianId, CapabilitySet>,
) -> Result<AuthorizationDecision, ThresholdError> {
    // Verify sufficient signatures
    if request.guardian_signatures.len() < request.required_threshold {
        return Ok(AuthorizationDecision::Denied {
            reason: DenialReason::InsufficientThreshold,
            required_count: request.required_threshold,
            provided_count: request.guardian_signatures.len(),
        });
    }
    
    // Compute intersection of guardian capabilities
    let mut effective_capabilities = CapabilitySet::universal();
    
    for guardian_id in request.guardian_signatures.keys() {
        let guardian_caps = guardian_capabilities
            .get(guardian_id)
            .ok_or(ThresholdError::UnknownGuardian(*guardian_id))?;
        
        effective_capabilities = effective_capabilities.meet(guardian_caps);
    }
    
    // Evaluate policy against effective capabilities
    if effective_capabilities.contains_all(&request.policy_requirements.required_capabilities) {
        Ok(AuthorizationDecision::Approved)
    } else {
        Ok(AuthorizationDecision::Denied {
            reason: DenialReason::InsufficientCapabilities,
            missing_capabilities: request.policy_requirements.required_capabilities
                .difference(&effective_capabilities)
                .collect(),
        })
    }
}
```

Threshold authorization combines multiple guardian capabilities through meet operations. This ensures all participating guardians have sufficient authority for the requested operation.

**Effect System Integration** provides capability evaluation through the effect system architecture. Capability effects enable testing with mock authorization while using real authorization in production.

```rust
#[async_trait]
pub trait CapabilityEffects: Send + Sync {
    async fn evaluate_capabilities(
        &self,
        device_id: DeviceId,
        operation: &Operation,
        context: &CapabilityContext,
    ) -> Result<CapabilitySet, CapabilityError>;
    
    async fn delegate_capabilities(
        &self,
        delegator: DeviceId,
        delegatee: DeviceId,
        capabilities: &CapabilitySet,
        conditions: Vec<DelegationCondition>,
    ) -> Result<DelegationId, DelegationError>;
    
    async fn revoke_delegation(
        &self,
        delegation_id: DelegationId,
        revoking_device: DeviceId,
        reason: RevocationReason,
    ) -> Result<(), RevocationError>;
}
```

Capability effects provide uniform interfaces for authorization operations across different environments. This enables testing with deterministic capability evaluation while using real policies in production.

For implementation details on how capabilities integrate with flow budget enforcement, see [Information Flow Budget](103_flow_budget_system.md).