# Capability Patterns

Quick reference for capability-based access control patterns used throughout Aura. Capability patterns provide reusable authorization solutions for common scenarios. All patterns follow meet-semilattice restriction laws ensuring capabilities can only be attenuated, never expanded.

Capability patterns combine authorization primitives into higher-level access control strategies. Pattern composition enables building complex authorization policies from proven building blocks.

See [Capability System](202_capability_system.md) for detailed architecture. See [Web of Trust](200_web_of_trust.md) for trust-based delegation.

---

## Basic Patterns

### Simple Authorization

**Purpose**: Direct capability check for straightforward operations.

```rust
pub fn check_simple_authorization(
    device_capabilities: &CapabilitySet,
    required_capability: Capability,
) -> Result<(), AuthorizationError> {
    if device_capabilities.contains(&required_capability) {
        Ok(())
    } else {
        Err(AuthorizationError::InsufficientCapability {
            required: required_capability,
            available: device_capabilities.clone(),
        })
    }
}

// Usage example
pub async fn read_storage<S: StorageEffects>(
    device_id: DeviceId,
    device_capabilities: &CapabilitySet,
    storage: &S,
    key: &str,
) -> Result<Vec<u8>, OperationError> {
    check_simple_authorization(device_capabilities, Capability::StorageRead)?;
    storage.load(key).await.map_err(Into::into)
}
```

### Multi-Capability Check

**Purpose**: Verify multiple capabilities required for complex operations.

```rust
pub fn check_multi_capability(
    device_capabilities: &CapabilitySet,
    required_capabilities: &[Capability],
) -> Result<(), AuthorizationError> {
    let required_set = CapabilitySet::from_capabilities(required_capabilities);
    
    if device_capabilities.contains_all(&required_set) {
        Ok(())
    } else {
        let missing = required_set.difference(device_capabilities);
        Err(AuthorizationError::MissingCapabilities {
            missing: missing.to_vec(),
        })
    }
}

// Usage example
pub async fn admin_operation<J: JournalEffects>(
    device_id: DeviceId,
    device_capabilities: &CapabilitySet,
    journal: &J,
    operation: AdminOperation,
) -> Result<(), OperationError> {
    check_multi_capability(
        device_capabilities, 
        &[Capability::TreeAdmin, Capability::PolicyModification]
    )?;
    
    journal.execute_admin_operation(operation).await.map_err(Into::into)
}
```

### Contextual Authorization

**Purpose**: Authorization decisions based on operation context and environment.

```rust
pub struct AuthorizationContext {
    pub device_id: DeviceId,
    pub operation_type: OperationType,
    pub target_resource: ResourceId,
    pub timestamp: u64,
    pub network_location: NetworkLocation,
}

pub fn check_contextual_authorization(
    capabilities: &CapabilitySet,
    context: &AuthorizationContext,
    policies: &PolicySet,
) -> Result<(), AuthorizationError> {
    let applicable_policies = policies.for_context(context);
    
    for policy in applicable_policies {
        // Check base capabilities
        if !capabilities.contains_all(&policy.required_capabilities) {
            return Err(AuthorizationError::PolicyViolation {
                policy_id: policy.id.clone(),
                violation_type: ViolationType::InsufficientCapabilities,
            });
        }
        
        // Check contextual constraints
        if !policy.evaluate_context(context) {
            return Err(AuthorizationError::PolicyViolation {
                policy_id: policy.id.clone(),
                violation_type: ViolationType::ContextConstraint,
            });
        }
    }
    
    Ok(())
}
```

## Delegation Patterns

### Basic Delegation

**Purpose**: Transfer limited authority from one device to another.

```rust
pub fn create_basic_delegation(
    delegator_capabilities: &CapabilitySet,
    delegated_capabilities: &CapabilitySet,
    delegatee: DeviceId,
    expiration: Option<SystemTime>,
) -> Result<DelegationToken, DelegationError> {
    // Verify delegator has authority to delegate
    if !delegator_capabilities.contains_all(delegated_capabilities) {
        return Err(DelegationError::InsufficientAuthority);
    }
    
    // Create attenuation (meet operation)
    let effective_capabilities = delegator_capabilities.meet(delegated_capabilities);
    
    DelegationToken::new(
        effective_capabilities,
        delegatee,
        expiration,
        DelegationConditions::none(),
    )
}

// Usage example
pub fn delegate_storage_access(
    admin_capabilities: &CapabilitySet,
    target_device: DeviceId,
) -> Result<DelegationToken, DelegationError> {
    let storage_capabilities = CapabilitySet::from_capabilities(&[
        Capability::StorageRead,
        Capability::StorageWrite,
    ]);
    
    create_basic_delegation(
        admin_capabilities,
        &storage_capabilities,
        target_device,
        Some(SystemTime::now() + Duration::from_hours(24)),
    )
}
```

### Conditional Delegation

**Purpose**: Delegation with environmental or behavioral conditions.

```rust
pub struct DelegationCondition {
    pub condition_type: ConditionType,
    pub predicate: ConditionPredicate,
    pub enforcement_policy: EnforcementPolicy,
}

pub enum ConditionType {
    TimeWindow { start: SystemTime, end: SystemTime },
    NetworkLocation { allowed_locations: Vec<NetworkLocation> },
    OperationLimit { max_operations: u32, window: Duration },
    RequiredApprovers { approvers: Vec<DeviceId>, threshold: usize },
}

pub fn create_conditional_delegation(
    delegator_capabilities: &CapabilitySet,
    delegated_capabilities: &CapabilitySet,
    delegatee: DeviceId,
    conditions: Vec<DelegationCondition>,
) -> Result<ConditionalDelegation, DelegationError> {
    let base_delegation = create_basic_delegation(
        delegator_capabilities,
        delegated_capabilities,
        delegatee,
        None,
    )?;
    
    ConditionalDelegation::new(base_delegation, conditions)
}

// Usage example
pub fn delegate_emergency_access(
    admin_capabilities: &CapabilitySet,
    emergency_responder: DeviceId,
    emergency_location: NetworkLocation,
) -> Result<ConditionalDelegation, DelegationError> {
    let emergency_capabilities = CapabilitySet::from_capabilities(&[
        Capability::EmergencyOverride,
        Capability::StorageRead,
        Capability::MessageSend,
    ]);
    
    let conditions = vec![
        DelegationCondition {
            condition_type: ConditionType::TimeWindow {
                start: SystemTime::now(),
                end: SystemTime::now() + Duration::from_hours(4),
            },
            predicate: ConditionPredicate::Always,
            enforcement_policy: EnforcementPolicy::Strict,
        },
        DelegationCondition {
            condition_type: ConditionType::NetworkLocation {
                allowed_locations: vec![emergency_location],
            },
            predicate: ConditionPredicate::Always,
            enforcement_policy: EnforcementPolicy::BestEffort,
        },
    ];
    
    create_conditional_delegation(
        admin_capabilities,
        &emergency_capabilities,
        emergency_responder,
        conditions,
    )
}
```

### Delegation Chain

**Purpose**: Multi-hop delegation with capability attenuation at each step.

```rust
pub struct DelegationChain {
    pub links: Vec<DelegationLink>,
    pub effective_capabilities: CapabilitySet,
}

pub struct DelegationLink {
    pub delegator: DeviceId,
    pub delegatee: DeviceId,
    pub delegated_capabilities: CapabilitySet,
    pub conditions: Vec<DelegationCondition>,
    pub signature: Signature,
}

impl DelegationChain {
    pub fn new(initial_capabilities: CapabilitySet, root_delegator: DeviceId) -> Self {
        Self {
            links: vec![],
            effective_capabilities: initial_capabilities,
        }
    }
    
    pub fn add_delegation(
        &mut self,
        delegator: DeviceId,
        delegatee: DeviceId,
        requested_capabilities: &CapabilitySet,
        conditions: Vec<DelegationCondition>,
        signature: Signature,
    ) -> Result<(), DelegationError> {
        // Verify delegator is last delegatee in chain or root
        if !self.links.is_empty() {
            let last_delegatee = self.links.last().unwrap().delegatee;
            if delegator != last_delegatee {
                return Err(DelegationError::InvalidChain);
            }
        }
        
        // Apply meet operation to restrict capabilities
        let new_effective = self.effective_capabilities.meet(requested_capabilities);
        
        let link = DelegationLink {
            delegator,
            delegatee,
            delegated_capabilities: new_effective.clone(),
            conditions,
            signature,
        };
        
        self.links.push(link);
        self.effective_capabilities = new_effective;
        
        Ok(())
    }
    
    pub fn validate(&self) -> Result<(), DelegationError> {
        // Verify all signatures in chain
        for link in &self.links {
            if !self.verify_link_signature(link)? {
                return Err(DelegationError::InvalidSignature);
            }
        }
        
        // Verify capability monotonicity
        let mut current_capabilities = self.initial_capabilities();
        for link in &self.links {
            if !current_capabilities.contains_all(&link.delegated_capabilities) {
                return Err(DelegationError::CapabilityEscalation);
            }
            current_capabilities = current_capabilities.meet(&link.delegated_capabilities);
        }
        
        Ok(())
    }
    
    fn verify_link_signature(&self, link: &DelegationLink) -> Result<bool, DelegationError> {
        // Implementation would verify cryptographic signature
        // This is a placeholder for the actual verification logic
        Ok(true)
    }
    
    fn initial_capabilities(&self) -> CapabilitySet {
        // Return capabilities at start of chain
        if self.links.is_empty() {
            self.effective_capabilities.clone()
        } else {
            // Reconstruct by reversing all meet operations
            self.effective_capabilities.clone() // Simplified
        }
    }
}
```

## Threshold Patterns

### Basic Threshold Authorization

**Purpose**: Require multiple devices to authorize sensitive operations.

```rust
pub struct ThresholdAuthorization {
    pub required_threshold: usize,
    pub participant_capabilities: BTreeMap<DeviceId, CapabilitySet>,
    pub collected_approvals: BTreeMap<DeviceId, ApprovalSignature>,
}

impl ThresholdAuthorization {
    pub fn new(
        threshold: usize,
        participants: BTreeMap<DeviceId, CapabilitySet>,
    ) -> Self {
        Self {
            required_threshold: threshold,
            participant_capabilities: participants,
            collected_approvals: BTreeMap::new(),
        }
    }
    
    pub fn add_approval(
        &mut self,
        device_id: DeviceId,
        signature: ApprovalSignature,
    ) -> Result<bool, ThresholdError> {
        // Verify device is authorized participant
        if !self.participant_capabilities.contains_key(&device_id) {
            return Err(ThresholdError::UnauthorizedParticipant);
        }
        
        // Verify signature
        if !self.verify_approval_signature(&device_id, &signature)? {
            return Err(ThresholdError::InvalidSignature);
        }
        
        self.collected_approvals.insert(device_id, signature);
        
        Ok(self.collected_approvals.len() >= self.required_threshold)
    }
    
    pub fn compute_effective_capabilities(&self) -> Result<CapabilitySet, ThresholdError> {
        if self.collected_approvals.len() < self.required_threshold {
            return Err(ThresholdError::InsufficientApprovals);
        }
        
        // Compute intersection of all approving devices' capabilities
        let mut effective = CapabilitySet::universal();
        
        for device_id in self.collected_approvals.keys() {
            let device_caps = self.participant_capabilities.get(device_id)
                .ok_or(ThresholdError::UnauthorizedParticipant)?;
            
            effective = effective.meet(device_caps);
        }
        
        Ok(effective)
    }
    
    fn verify_approval_signature(
        &self,
        device_id: &DeviceId,
        signature: &ApprovalSignature,
    ) -> Result<bool, ThresholdError> {
        // Implementation would verify cryptographic signature
        Ok(true)
    }
}

// Usage example
pub async fn threshold_admin_operation<J: JournalEffects>(
    operation: AdminOperation,
    authorized_admins: BTreeMap<DeviceId, CapabilitySet>,
    journal: &J,
) -> Result<(), OperationError> {
    let mut threshold_auth = ThresholdAuthorization::new(2, authorized_admins);
    
    // Collect approvals (simplified - would be async process)
    // threshold_auth.add_approval(admin_1, signature_1)?;
    // threshold_auth.add_approval(admin_2, signature_2)?;
    
    let effective_capabilities = threshold_auth.compute_effective_capabilities()?;
    
    // Verify effective capabilities can perform operation
    check_multi_capability(&effective_capabilities, &operation.required_capabilities())?;
    
    journal.execute_admin_operation(operation).await.map_err(Into::into)
}
```

### Weighted Threshold Authorization

**Purpose**: Threshold authorization with weighted voting based on trust levels.

```rust
pub struct WeightedThreshold {
    pub required_weight: f64,
    pub participant_weights: BTreeMap<DeviceId, f64>,
    pub participant_capabilities: BTreeMap<DeviceId, CapabilitySet>,
    pub weighted_approvals: BTreeMap<DeviceId, (f64, ApprovalSignature)>,
}

impl WeightedThreshold {
    pub fn new(
        required_weight: f64,
        participants: BTreeMap<DeviceId, (f64, CapabilitySet)>,
    ) -> Self {
        let mut weights = BTreeMap::new();
        let mut capabilities = BTreeMap::new();
        
        for (device_id, (weight, caps)) in participants {
            weights.insert(device_id, weight);
            capabilities.insert(device_id, caps);
        }
        
        Self {
            required_weight,
            participant_weights: weights,
            participant_capabilities: capabilities,
            weighted_approvals: BTreeMap::new(),
        }
    }
    
    pub fn add_weighted_approval(
        &mut self,
        device_id: DeviceId,
        signature: ApprovalSignature,
    ) -> Result<bool, ThresholdError> {
        let weight = self.participant_weights.get(&device_id)
            .ok_or(ThresholdError::UnauthorizedParticipant)?;
        
        if !self.verify_approval_signature(&device_id, &signature)? {
            return Err(ThresholdError::InvalidSignature);
        }
        
        self.weighted_approvals.insert(device_id, (*weight, signature));
        
        let total_weight: f64 = self.weighted_approvals.values()
            .map(|(weight, _)| weight)
            .sum();
        
        Ok(total_weight >= self.required_weight)
    }
    
    pub fn compute_weighted_capabilities(&self) -> Result<CapabilitySet, ThresholdError> {
        let total_weight: f64 = self.weighted_approvals.values()
            .map(|(weight, _)| weight)
            .sum();
        
        if total_weight < self.required_weight {
            return Err(ThresholdError::InsufficientWeight { 
                required: self.required_weight, 
                actual: total_weight 
            });
        }
        
        // Weight-adjusted capability intersection
        let mut effective = CapabilitySet::universal();
        
        for (device_id, (weight, _)) in &self.weighted_approvals {
            let device_caps = self.participant_capabilities.get(device_id)
                .ok_or(ThresholdError::UnauthorizedParticipant)?;
            
            // Apply weight to capability strength
            let weighted_caps = device_caps.apply_weight(*weight);
            effective = effective.meet(&weighted_caps);
        }
        
        Ok(effective)
    }
    
    fn verify_approval_signature(
        &self,
        device_id: &DeviceId,
        signature: &ApprovalSignature,
    ) -> Result<bool, ThresholdError> {
        Ok(true) // Placeholder
    }
}
```

## Emergency Patterns

### Emergency Override

**Purpose**: Bypass normal authorization during critical situations.

```rust
pub struct EmergencyOverride {
    pub emergency_type: EmergencyType,
    pub authorizing_device: DeviceId,
    pub override_capabilities: CapabilitySet,
    pub justification: String,
    pub auto_expiry: SystemTime,
    pub audit_trail: Vec<EmergencyAction>,
}

pub enum EmergencyType {
    SecurityBreach,
    SystemFailure,
    DataRecovery,
    NetworkPartition,
    PhysicalCompromise,
}

impl EmergencyOverride {
    pub fn create(
        emergency_type: EmergencyType,
        authorizing_device: DeviceId,
        authorizing_capabilities: &CapabilitySet,
        requested_override: &CapabilitySet,
        justification: String,
        duration: Duration,
    ) -> Result<Self, EmergencyError> {
        // Verify authorizing device has emergency capability
        if !authorizing_capabilities.contains(&Capability::EmergencyOverride) {
            return Err(EmergencyError::InsufficientAuthority);
        }
        
        // Apply emergency policy constraints
        let max_override = EmergencyPolicy::max_override_for_type(&emergency_type);
        let effective_override = requested_override.meet(&max_override);
        
        let auto_expiry = SystemTime::now() + duration.min(Duration::from_hours(24));
        
        Ok(Self {
            emergency_type,
            authorizing_device,
            override_capabilities: effective_override,
            justification,
            auto_expiry,
            audit_trail: vec![EmergencyAction::OverrideCreated {
                timestamp: SystemTime::now(),
                authorizer: authorizing_device,
            }],
        })
    }
    
    pub fn is_valid(&self) -> bool {
        SystemTime::now() < self.auto_expiry
    }
    
    pub fn record_action(&mut self, action: EmergencyAction) {
        self.audit_trail.push(action);
    }
    
    pub fn revoke(&mut self, revoking_device: DeviceId) -> Result<(), EmergencyError> {
        if revoking_device != self.authorizing_device &&
           !self.can_device_revoke(revoking_device) {
            return Err(EmergencyError::CannotRevoke);
        }
        
        self.record_action(EmergencyAction::OverrideRevoked {
            timestamp: SystemTime::now(),
            revoker: revoking_device,
        });
        
        // Set expiry to now to invalidate override
        self.auto_expiry = SystemTime::now();
        
        Ok(())
    }
    
    fn can_device_revoke(&self, device_id: DeviceId) -> bool {
        // Implementation would check if device has authority to revoke emergency overrides
        false
    }
}

pub enum EmergencyAction {
    OverrideCreated { timestamp: SystemTime, authorizer: DeviceId },
    OverrideUsed { timestamp: SystemTime, operation: String },
    OverrideRevoked { timestamp: SystemTime, revoker: DeviceId },
}

// Usage example
pub async fn emergency_data_access<S: StorageEffects>(
    emergency_override: &EmergencyOverride,
    storage: &S,
    critical_data_key: &str,
) -> Result<Vec<u8>, EmergencyError> {
    if !emergency_override.is_valid() {
        return Err(EmergencyError::ExpiredOverride);
    }
    
    if !emergency_override.override_capabilities.contains(&Capability::StorageRead) {
        return Err(EmergencyError::InsufficientOverrideCapability);
    }
    
    match emergency_override.emergency_type {
        EmergencyType::DataRecovery | EmergencyType::SystemFailure => {
            // Allowed for these emergency types
        }
        _ => return Err(EmergencyError::WrongEmergencyType),
    }
    
    // Record usage in audit trail
    let mut override_copy = emergency_override.clone();
    override_copy.record_action(EmergencyAction::OverrideUsed {
        timestamp: SystemTime::now(),
        operation: format!("data_access:{}", critical_data_key),
    });
    
    storage.load(critical_data_key).await.map_err(|e| EmergencyError::StorageError(e))
}
```

### Emergency Recovery

**Purpose**: Coordinate emergency recovery with multiple stakeholders.

```rust
pub struct EmergencyRecovery {
    pub recovery_id: Uuid,
    pub emergency_type: EmergencyType,
    pub affected_resources: Vec<ResourceId>,
    pub recovery_plan: RecoveryPlan,
    pub stakeholder_approvals: BTreeMap<DeviceId, RecoveryApproval>,
    pub required_consensus: f64, // Fraction of stakeholders needed
    pub timeline: RecoveryTimeline,
}

pub struct RecoveryPlan {
    pub phases: Vec<RecoveryPhase>,
    pub rollback_procedures: Vec<RollbackStep>,
    pub success_criteria: Vec<SuccessCriterion>,
}

pub struct RecoveryPhase {
    pub phase_id: String,
    pub description: String,
    pub required_capabilities: CapabilitySet,
    pub estimated_duration: Duration,
    pub dependencies: Vec<String>, // Other phase IDs
    pub actions: Vec<RecoveryAction>,
}

impl EmergencyRecovery {
    pub fn initiate(
        emergency_type: EmergencyType,
        affected_resources: Vec<ResourceId>,
        recovery_plan: RecoveryPlan,
        stakeholders: Vec<DeviceId>,
        required_consensus: f64,
    ) -> Self {
        let timeline = RecoveryTimeline::new();
        
        Self {
            recovery_id: Uuid::new_v4(),
            emergency_type,
            affected_resources,
            recovery_plan,
            stakeholder_approvals: BTreeMap::new(),
            required_consensus,
            timeline,
        }
    }
    
    pub fn add_stakeholder_approval(
        &mut self,
        stakeholder: DeviceId,
        approval: RecoveryApproval,
    ) -> Result<RecoveryStatus, RecoveryError> {
        // Verify stakeholder authority
        if !self.verify_stakeholder_authority(stakeholder) {
            return Err(RecoveryError::UnauthorizedStakeholder);
        }
        
        self.stakeholder_approvals.insert(stakeholder, approval);
        
        let approval_ratio = self.stakeholder_approvals.len() as f64 / 
                           self.total_stakeholders() as f64;
        
        if approval_ratio >= self.required_consensus {
            Ok(RecoveryStatus::ApprovedForExecution)
        } else {
            Ok(RecoveryStatus::AwaitingApprovals {
                current_ratio: approval_ratio,
                required_ratio: self.required_consensus,
            })
        }
    }
    
    pub async fn execute_recovery_phase(
        &mut self,
        phase_id: &str,
        effect_registry: &EffectRegistry,
    ) -> Result<PhaseResult, RecoveryError> {
        let phase = self.recovery_plan.phases.iter()
            .find(|p| p.phase_id == phase_id)
            .ok_or(RecoveryError::PhaseNotFound)?;
        
        // Verify all dependencies completed
        for dependency in &phase.dependencies {
            if !self.timeline.is_phase_completed(dependency) {
                return Err(RecoveryError::DependencyNotMet { 
                    dependency: dependency.clone() 
                });
            }
        }
        
        // Execute phase actions
        let mut results = Vec::new();
        
        for action in &phase.actions {
            let result = self.execute_recovery_action(action, effect_registry).await?;
            results.push(result);
            
            self.timeline.record_action_completion(&action.action_id);
        }
        
        self.timeline.mark_phase_completed(phase_id);
        
        Ok(PhaseResult {
            phase_id: phase_id.to_string(),
            action_results: results,
            completion_time: SystemTime::now(),
        })
    }
    
    async fn execute_recovery_action(
        &self,
        action: &RecoveryAction,
        effect_registry: &EffectRegistry,
    ) -> Result<ActionResult, RecoveryError> {
        match action {
            RecoveryAction::RestoreData { backup_id, target_location } => {
                effect_registry.storage_handler
                    .restore_from_backup(backup_id, target_location).await
                    .map(|_| ActionResult::DataRestored)
                    .map_err(RecoveryError::ActionFailed)
            }
            RecoveryAction::ReestablishConnections { peer_list } => {
                for peer in peer_list {
                    effect_registry.network_handler
                        .establish_connection(*peer).await
                        .map_err(RecoveryError::ActionFailed)?;
                }
                Ok(ActionResult::ConnectionsReestablished)
            }
            RecoveryAction::ValidateIntegrity { resource_ids } => {
                // Verify data integrity for specified resources
                let mut integrity_results = BTreeMap::new();
                
                for resource_id in resource_ids {
                    let is_valid = effect_registry.journal_handler
                        .verify_resource_integrity(resource_id).await
                        .map_err(RecoveryError::ActionFailed)?;
                    
                    integrity_results.insert(resource_id.clone(), is_valid);
                }
                
                Ok(ActionResult::IntegrityValidated { results: integrity_results })
            }
        }
    }
    
    fn verify_stakeholder_authority(&self, stakeholder: DeviceId) -> bool {
        // Implementation would verify stakeholder has authority for emergency decisions
        true
    }
    
    fn total_stakeholders(&self) -> usize {
        // Implementation would return total number of eligible stakeholders
        5 // Placeholder
    }
}

pub enum RecoveryAction {
    RestoreData { backup_id: String, target_location: String },
    ReestablishConnections { peer_list: Vec<DeviceId> },
    ValidateIntegrity { resource_ids: Vec<ResourceId> },
}

pub enum ActionResult {
    DataRestored,
    ConnectionsReestablished,
    IntegrityValidated { results: BTreeMap<ResourceId, bool> },
}

pub struct PhaseResult {
    pub phase_id: String,
    pub action_results: Vec<ActionResult>,
    pub completion_time: SystemTime,
}

pub enum RecoveryStatus {
    AwaitingApprovals { current_ratio: f64, required_ratio: f64 },
    ApprovedForExecution,
    InProgress { current_phase: String },
    Completed { success: bool },
    Failed { reason: String },
}
```

## Testing Patterns

### Capability Mocking

**Purpose**: Test authorization logic with controlled capability sets.

```rust
pub struct MockCapabilityProvider {
    device_capabilities: BTreeMap<DeviceId, CapabilitySet>,
    capability_responses: BTreeMap<String, Result<CapabilitySet, AuthorizationError>>,
}

impl MockCapabilityProvider {
    pub fn new() -> Self {
        Self {
            device_capabilities: BTreeMap::new(),
            capability_responses: BTreeMap::new(),
        }
    }
    
    pub fn set_device_capabilities(&mut self, device_id: DeviceId, capabilities: CapabilitySet) {
        self.device_capabilities.insert(device_id, capabilities);
    }
    
    pub fn set_authorization_response(&mut self, operation: &str, response: Result<CapabilitySet, AuthorizationError>) {
        self.capability_responses.insert(operation.to_string(), response);
    }
}

#[async_trait]
impl CapabilityEffects for MockCapabilityProvider {
    async fn evaluate_capabilities(
        &self,
        device_id: DeviceId,
        operation: &Operation,
        _context: &CapabilityContext,
    ) -> Result<CapabilitySet, CapabilityError> {
        // Check for specific operation override first
        if let Some(response) = self.capability_responses.get(&operation.operation_id()) {
            return response.clone().map_err(|_| CapabilityError::AuthorizationDenied);
        }
        
        // Return device capabilities
        self.device_capabilities.get(&device_id)
            .cloned()
            .ok_or(CapabilityError::DeviceNotFound)
    }
    
    async fn delegate_capabilities(
        &self,
        _delegator: DeviceId,
        _delegatee: DeviceId,
        capabilities: &CapabilitySet,
        _conditions: Vec<DelegationCondition>,
    ) -> Result<DelegationId, DelegationError> {
        // Always succeed in tests
        Ok(DelegationId::new())
    }
    
    async fn revoke_delegation(
        &self,
        _delegation_id: DelegationId,
        _revoking_device: DeviceId,
        _reason: RevocationReason,
    ) -> Result<(), RevocationError> {
        Ok(())
    }
}

// Usage in tests
#[tokio::test]
async fn test_storage_authorization() {
    let mut mock_capabilities = MockCapabilityProvider::new();
    
    let device_id = DeviceId::new();
    let storage_caps = CapabilitySet::from_capabilities(&[
        Capability::StorageRead,
        Capability::StorageWrite,
    ]);
    
    mock_capabilities.set_device_capabilities(device_id, storage_caps);
    
    // Test authorized access
    let context = CapabilityContext::new(device_id);
    let operation = Operation::storage_read("test_key");
    
    let result = mock_capabilities.evaluate_capabilities(device_id, &operation, &context).await;
    assert!(result.is_ok());
}
```

### Authorization Test Scenarios

**Purpose**: Comprehensive test coverage for authorization patterns.

```rust
pub struct AuthorizationTestScenario {
    pub scenario_name: String,
    pub devices: BTreeMap<DeviceId, CapabilitySet>,
    pub operations: Vec<(DeviceId, Operation, ExpectedResult)>,
    pub delegations: Vec<TestDelegation>,
    pub policies: PolicySet,
}

pub struct TestDelegation {
    pub delegator: DeviceId,
    pub delegatee: DeviceId,
    pub capabilities: CapabilitySet,
    pub conditions: Vec<DelegationCondition>,
    pub should_succeed: bool,
}

pub enum ExpectedResult {
    Success,
    Failure(AuthorizationError),
}

impl AuthorizationTestScenario {
    pub fn basic_authorization_scenario() -> Self {
        let device_admin = DeviceId::from_seed(1);
        let device_user = DeviceId::from_seed(2);
        
        let mut devices = BTreeMap::new();
        devices.insert(device_admin, CapabilitySet::admin());
        devices.insert(device_user, CapabilitySet::user());
        
        let operations = vec![
            (device_admin, Operation::admin_config(), ExpectedResult::Success),
            (device_user, Operation::admin_config(), ExpectedResult::Failure(
                AuthorizationError::InsufficientCapability {
                    required: Capability::PolicyModification,
                    available: CapabilitySet::user(),
                }
            )),
            (device_user, Operation::read_storage(), ExpectedResult::Success),
        ];
        
        Self {
            scenario_name: "Basic Authorization".to_string(),
            devices,
            operations,
            delegations: vec![],
            policies: PolicySet::default(),
        }
    }
    
    pub fn delegation_scenario() -> Self {
        let device_admin = DeviceId::from_seed(1);
        let device_delegate = DeviceId::from_seed(2);
        
        let mut devices = BTreeMap::new();
        devices.insert(device_admin, CapabilitySet::admin());
        devices.insert(device_delegate, CapabilitySet::empty());
        
        let delegations = vec![
            TestDelegation {
                delegator: device_admin,
                delegatee: device_delegate,
                capabilities: CapabilitySet::from_capabilities(&[Capability::StorageRead]),
                conditions: vec![],
                should_succeed: true,
            }
        ];
        
        let operations = vec![
            // After delegation, delegate should be able to read storage
            (device_delegate, Operation::read_storage(), ExpectedResult::Success),
            // But not write storage (wasn't delegated)
            (device_delegate, Operation::write_storage(), ExpectedResult::Failure(
                AuthorizationError::InsufficientCapability {
                    required: Capability::StorageWrite,
                    available: CapabilitySet::from_capabilities(&[Capability::StorageRead]),
                }
            )),
        ];
        
        Self {
            scenario_name: "Delegation".to_string(),
            devices,
            operations,
            delegations,
            policies: PolicySet::default(),
        }
    }
    
    pub async fn execute(&self, capability_provider: &dyn CapabilityEffects) -> TestResult {
        let mut results = Vec::new();
        
        // Execute delegations first
        for delegation in &self.delegations {
            let delegation_result = capability_provider.delegate_capabilities(
                delegation.delegator,
                delegation.delegatee,
                &delegation.capabilities,
                delegation.conditions.clone(),
            ).await;
            
            let success = delegation_result.is_ok();
            if success != delegation.should_succeed {
                results.push(TestStepResult::DelegationFailed {
                    delegator: delegation.delegator,
                    delegatee: delegation.delegatee,
                    expected_success: delegation.should_succeed,
                    actual_success: success,
                });
            }
        }
        
        // Execute operations
        for (device_id, operation, expected_result) in &self.operations {
            let context = CapabilityContext::new(*device_id);
            let actual_result = capability_provider.evaluate_capabilities(
                *device_id,
                operation,
                &context,
            ).await;
            
            let matches_expectation = match (expected_result, &actual_result) {
                (ExpectedResult::Success, Ok(_)) => true,
                (ExpectedResult::Failure(_), Err(_)) => true,
                _ => false,
            };
            
            if !matches_expectation {
                results.push(TestStepResult::OperationMismatch {
                    device_id: *device_id,
                    operation: operation.clone(),
                    expected: expected_result.clone(),
                    actual: actual_result,
                });
            }
        }
        
        TestResult {
            scenario_name: self.scenario_name.clone(),
            passed: results.is_empty(),
            failures: results,
        }
    }
}

pub struct TestResult {
    pub scenario_name: String,
    pub passed: bool,
    pub failures: Vec<TestStepResult>,
}

pub enum TestStepResult {
    DelegationFailed {
        delegator: DeviceId,
        delegatee: DeviceId,
        expected_success: bool,
        actual_success: bool,
    },
    OperationMismatch {
        device_id: DeviceId,
        operation: Operation,
        expected: ExpectedResult,
        actual: Result<CapabilitySet, CapabilityError>,
    },
}
```

This capability patterns reference provides reusable authorization solutions for common scenarios in distributed systems. The patterns can be composed and extended to build sophisticated access control policies while maintaining the mathematical properties that ensure security.