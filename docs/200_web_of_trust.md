# Web of Trust

Aura implements a capability-based Web of Trust system for authorization and spam prevention. The system uses meet-semilattice operations to ensure capabilities can only shrink through delegation. Trust relationships establish the foundation for both capability evaluation and information flow controls.

The Web of Trust integrates with threshold identity, guardian recovery, and peer discovery systems. All trust decisions flow through capability evaluation using formal mathematical properties. Trust relationships provide input signals for budget allocation and spam prevention.

See [Authentication vs Authorization Architecture](101_auth_authz.md) for integration details. See [Information Flow Budget](103_info_flow_budget.md) for budget calculations. See [Theoretical Foundations](001_theoretical_foundations.md) for semilattice semantics.

---

## Trust Relationship Model

### Trust Level Semantics

Trust levels follow a four-level hierarchy from None to High. Each level represents both capability authority and communication budget allocation. Higher trust enables more capabilities and larger communication budgets.

```rust
pub enum TrustLevel {
    None = 0,    // No trust (bottom element)
    Low = 1,     // Limited capabilities
    Medium = 2,  // Standard capabilities
    High = 3,    // Full capabilities (top element)
}
```

Trust levels determine capability evaluation through meet operations. When two devices interact, their effective capabilities use the minimum trust level. This ensures conservative authorization even under partial information.

Trust levels also influence information flow budgets. Higher trust relationships receive larger communication allowances per epoch. The budget allocation formula weights trust level alongside reciprocity and abuse metrics.

### Relationship Formation

Trust relationships form through explicit invitation or guardian introduction protocols. Direct relationships require mutual acceptance through cryptographic ceremonies. Transitive relationships inherit reduced trust levels through delegation chains.

The invitation system creates initial trust relationships between devices. Invitation acceptance establishes bidirectional relationship keys and sets initial trust levels. Default trust starts at Low level with manual upgrade to Medium or High.

Guardian relationships use High trust by default since they control account recovery. Guardian trust provides elevated capabilities for tree operations and emergency procedures. Multiple guardians enable threshold-based recovery ceremonies.

### Trust Weight Calculation

Web of Trust edge weights range from 0.0 to 1.0 based on relationship factors. Direct relationships start with weight derived from trust level. Transitive relationships decay through delegation depth and attestation quality.

```rust
pub fn calculate_trust_weight(
    relationship: &TrustRelationship,
    delegation_depth: u32,
    attestations: &[Attestation],
) -> f64 {
    let base_weight = relationship.trust_level.to_level() as f64 / 3.0;
    let depth_decay = 0.8_f64.powi(delegation_depth as i32);
    let attestation_boost = attestations.len().min(5) as f64 * 0.05;

    (base_weight * depth_decay + attestation_boost).min(1.0)
}
```

Trust weight influences both capability evaluation and budget allocation. Higher weights enable more permissive capability intersections. Higher weights also provide larger information flow budgets per communication epoch.

## Capability Operations

### Meet-Semilattice Laws

Capability operations follow meet-semilattice laws ensuring monotonic restriction. Capabilities can only shrink through intersection operations. This prevents privilege escalation through delegation or composition.

The meet operation computes intersection of two capability sets. Only permissions present in both sets remain in the result. This mathematical property ensures safety under concurrent capability updates or network partitions.

```rust
impl CapabilitySet {
    pub fn meet(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            permissions: self.permissions
                .intersection(&other.permissions)
                .cloned()
                .collect()
        }
    }
}
```

Meet operation properties include associativity, commutativity, and idempotence. Associativity means grouping does not matter. Commutativity means order does not matter. Idempotence means self-intersection has no effect.

Property-based tests verify these laws automatically during development. Any violation indicates implementation bugs that could enable privilege escalation. The test suite generates random capability sets and validates mathematical properties.

### Delegation Chains

Capability delegation enables temporary authority transfer with proper attenuation. Each delegation step can only grant capabilities that the delegator currently possesses. Delegation depth limits prevent unbounded chain length.

```rust
pub struct DelegationLink {
    pub delegator: DeviceId,
    pub delegatee: DeviceId,
    pub capabilities: CapabilitySet,
    pub max_delegation_depth: u32,
    pub expires_at: Option<SystemTime>,
}
```

Delegation chains compose through sequential meet operations. The effective capabilities equal the intersection of all links in the chain. This ensures delegated authority never exceeds original authority.

Delegation expiration provides automatic cleanup for temporary grants. Expired delegations contribute empty capability sets to meet operations. This prevents stale delegations from accumulating over time.

### Guardian Capabilities

Guardian capabilities enable account recovery and emergency operations. Guardians receive elevated privileges for tree operations, epoch rotation, and dispute resolution. Guardian capabilities require threshold coordination for safety.

Guardian capability evaluation uses threshold logic for authorization. Operations require approval from a minimum number of guardians based on account policy. Individual guardians cannot perform sensitive operations alone.

```rust
pub fn evaluate_guardian_operation(
    operation: &TreeOp,
    guardian_signatures: &BTreeSet<GuardianId>,
    policy: &ThresholdConfig,
) -> Result<PermissionGrant, WotError> {
    if guardian_signatures.len() >= policy.threshold as usize {
        Ok(PermissionGrant::Approved)
    } else {
        Err(WotError::InsufficientGuardians)
    }
}
```

Guardian threshold policies specify required approvals for different operation types. Account recovery requires higher thresholds than routine tree maintenance. Emergency operations may have different threshold requirements.

## Trust Evaluation Context

### Journal Integration

Trust relationships persist in the journal as CRDT facts using join-semilattice semantics. Relationship facts accumulate trust evidence over time. Trust downgrades require explicit negative facts rather than removal.

```rust
pub struct TrustRelationshipFact {
    pub relationship_id: RelationshipId,
    pub trust_level: TrustLevel,
    pub evidence: TrustEvidence,
    pub timestamp: u64,
    pub attestations: Vec<DeviceId>,
}
```

Trust facts merge using maximum trust level and union of evidence sets. This ensures trust relationships strengthen over time through additional attestations. Trust degradation requires explicit negative evidence facts.

Journal queries provide trust relationship lookups for capability evaluation. Queries filter by relationship type, trust level, and temporal validity. Cached results improve performance for frequent authorization decisions.

### Policy Enforcement

Policy evaluation combines trust relationships with capability requirements for authorization decisions. Policies specify minimum trust levels for different operations. Complex policies use boolean logic over trust predicates.

```rust
pub struct TrustPolicy {
    pub min_trust_level: TrustLevel,
    pub required_attestations: usize,
    pub max_delegation_depth: u32,
    pub temporal_constraints: Option<TimeRange>,
}
```

Policy enforcement integrates with guard chains at send sites. CapGuard evaluates trust-based policies before operation execution. Policy violations prevent operation execution and preserve system invariants.

Custom policies enable application-specific trust requirements. Storage operations might require Medium trust while relay operations accept Low trust. Emergency operations require High trust from multiple attestors.

### Attestation System

Trust attestations provide additional evidence for capability evaluation. Attestations come from other trusted devices observing positive interactions. Multiple attestations strengthen trust relationships over time.

Attestation weight depends on the attesting device's own trust level and relationship distance. Direct relationships provide stronger attestations than transitive relationships. Recent attestations receive higher weight than historical attestations.

```rust
pub struct TrustAttestation {
    pub attestor: DeviceId,
    pub subject: DeviceId,
    pub interaction_type: InteractionType,
    pub quality_score: f64,
    pub timestamp: u64,
}
```

Attestation aggregation uses weighted averaging based on attestor credibility. Conflicting attestations trigger manual review processes. Attestation spam protection limits frequency per attestor and subject pair.

## Spam Prevention

### Budget Allocation

Web of Trust relationships provide input signals for information flow budget calculation. Higher trust relationships receive larger communication allowances. Trust weight multiplies base budget allocations per communication epoch.

Budget allocation formula combines trust weight with reciprocity and abuse metrics. Trust component provides base allocation scaled by relationship strength. Additional components account for communication patterns and historical behavior.

```rust
pub fn calculate_flow_budget(
    base_limit: u64,
    trust_weight: f64,
    reciprocity_factor: f64,
    abuse_penalty: u64,
) -> u64 {
    let trust_boost = (base_limit as f64 * trust_weight) as u64;
    let reciprocity_boost = (base_limit as f64 * reciprocity_factor * 0.5) as u64;

    (trust_boost + reciprocity_boost).saturating_sub(abuse_penalty)
}
```

Trust-based budget allocation creates natural spam resistance. Untrusted relationships receive minimal communication budgets. Trusted relationships enable normal communication patterns. This aligns spam prevention with social trust patterns.

### Abuse Response

Trust system integrates with abuse reporting for adaptive response. Abuse reports trigger trust level downgrades and budget reductions. Repeated abuse results in relationship termination and capability revocation.

Abuse response uses threshold logic to prevent false positives. Multiple independent reports trigger automatic trust downgrades. Severe abuse triggers immediate relationship suspension pending manual review.

```rust
pub fn process_abuse_report(
    report: &AbuseReport,
    relationship: &mut TrustRelationship,
    abuse_threshold: usize,
) -> TrustAction {
    relationship.abuse_reports.push(report.clone());

    if relationship.abuse_reports.len() >= abuse_threshold {
        match report.severity {
            AbuseSeverity::Minor => TrustAction::DowngradeLevel,
            AbuseSeverity::Major => TrustAction::SuspendRelationship,
            AbuseSeverity::Severe => TrustAction::TerminateRelationship,
        }
    } else {
        TrustAction::RecordReport
    }
}
```

Trust recovery enables relationship rehabilitation after abuse resolution. Recovery requires time delays and attestation rebuilding. Successful recovery restores communication capabilities and trust levels.

## Implementation Architecture

### Core Components

The `aura-wot` crate implements Web of Trust capability evaluation using meet-semilattice operations. Core types include `CapabilitySet` for permission management and `TrustLevel` for relationship strength encoding.

Capability evaluation integrates with tree authorization for account operations. Tree policies specify required trust levels for different operation types. Guardian coordination uses elevated trust capabilities for recovery procedures.

```rust
pub fn evaluate_tree_operation_capabilities(
    operation: &TreeOp,
    context: &TreeAuthzContext,
    requester_capabilities: &CapabilitySet,
    guardian_signatures: &BTreeSet<GuardianId>,
) -> Result<PermissionGrant, WotError>
```

Trust relationship storage uses journal CRDT facts for eventual consistency. Trust evaluation queries combine current relationships with delegation chains. Capability intersection produces effective authorization decisions.

### Effect System Integration

Web of Trust integrates with effect system through `AgentEffects` trait for unified authorization operations. Effect handlers provide trust evaluation, capability checking, and policy enforcement through common interfaces.

```rust
#[async_trait]
pub trait AgentEffects: Send + Sync {
    async fn evaluate_trust_relationship(&self,
        relationship_id: RelationshipId) -> Result<TrustLevel, WotError>;
    async fn check_capabilities(&self,
        operation: &TreeOp,
        context: &AuthorizationContext) -> Result<bool, WotError>;
}
```

Production handlers use real trust relationship queries against journal state. Testing handlers use deterministic mock relationships for reproducible tests. Simulation handlers inject trust failures for robustness testing.

Effect composition enables authorization integration with other system components. Guard chains use trust effects for send-site authorization. Journal operations use trust effects for write authorization.

### Property Verification

Property-based testing verifies Web of Trust mathematical properties automatically. Tests generate random capability sets and trust relationships then verify semilattice laws. Any violation indicates implementation errors requiring fixes.

```rust
proptest! {
    #[test]
    fn trust_relationship_meet_is_monotonic(
        trust_a: TrustLevel,
        trust_b: TrustLevel
    ) {
        let relationship_a = TrustRelationship::new(trust_a);
        let relationship_b = TrustRelationship::new(trust_b);
        let meet_result = relationship_a.meet(&relationship_b);

        prop_assert!(meet_result.trust_level <= trust_a);
        prop_assert!(meet_result.trust_level <= trust_b);
    }
}
```

Integration tests verify Web of Trust behavior under concurrent operations and network partitions. Tests simulate multiple devices with different trust relationships executing operations simultaneously. Convergence verification ensures eventual consistency.

Formal verification using Quint specifications validates critical Web of Trust properties. Specifications model trust relationship evolution and capability delegation under adversarial conditions. Model checking verifies safety and liveness properties.
