# Trust Relationships

Trust relationships form the foundation for capability delegation and secure communication in Aura. Relationships establish bidirectional trust through cryptographic ceremonies and enable capability evaluation based on social connections. The relationship system provides input signals for both authorization decisions and spam prevention mechanisms.

Trust relationships persist as CRDT facts in the journal using join-semilattice semantics. Relationship establishment requires mutual consent through choreographic protocols. Relationship maintenance adapts to changing trust evidence over time.

See [Web of Trust](200_web_of_trust.md) for capability evaluation. See [Authentication vs Authorization Architecture](101_auth_authz.md) for trust integration. See [Information Flow Budget](103_info_flow_budget.md) for budget calculations.

---

## Relationship Formation

### Invitation-Based Formation

Trust relationships begin with invitation protocols that establish mutual authentication and initial trust levels. The invitation system creates secure channels for relationship negotiation before full trust establishment.

Invitation acceptance triggers the relationship formation ceremony that derives shared cryptographic material. The ceremony establishes bidirectional relationship keys and creates trust records in both device journals.

```rust
pub struct RelationshipFormationConfig {
    pub initiator_id: DeviceId,
    pub responder_id: DeviceId,
    pub account_context: Option<AccountId>,
    pub timeout_secs: u64,
}

pub struct RelationshipFormationResult {
    pub context_id: ContextId,
    pub relationship_keys: RelationshipKeys,
    pub trust_record_hash: Hash32,
    pub success: bool,
}
```

The formation ceremony follows a four-phase choreographic protocol. Phase one establishes context and mutual authentication. Phase two exchanges public keys and derives shared material. Phase three validates key derivation through proof exchange. Phase four creates trust records in local journals.

Each phase includes timeout protection and failure recovery. Failed ceremonies can be retried with new cryptographic material. Successful ceremonies produce durable trust relationships persisted in journal state.

### Guardian Introduction

Guardian relationships use elevated trust levels and specialized formation protocols. Guardian trust enables account recovery operations and requires higher authentication standards than normal relationships.

Guardian formation includes additional verification steps for identity validation and capability attestation. Multiple guardians coordinate through threshold protocols to establish collective trust authority. Guardian relationships receive High trust level by default.

```rust
pub struct GuardianRelationship {
    pub guardian_id: GuardianId,
    pub account_id: AccountId,
    pub trust_level: TrustLevel,
    pub recovery_capabilities: CapabilitySet,
    pub attestation_threshold: u32,
}
```

Guardian introduction ceremonies verify guardian credentials and establish recovery capabilities. Guardian trust records include attestation thresholds for recovery operations. Multiple guardian relationships enable distributed account recovery without single points of failure.

Guardian relationships support revocation through threshold voting among existing guardians. Revocation requires sufficient guardian consensus and creates negative trust facts in the journal. Revoked guardians lose recovery capabilities but may retain standard relationship trust.

### Transitive Relationship Discovery

Transitive relationships form through mutual connections in the social graph. When Alice trusts Bob and Bob trusts Charlie, Alice may develop transitive trust toward Charlie based on Bob's attestation.

Transitive trust discovery uses automated protocols that propagate trust information through existing relationships. Trust levels decay through delegation distance to prevent excessive transitive authority. Maximum delegation depth limits prevent unbounded trust chains.

```rust
pub fn calculate_transitive_trust(
    direct_trust: TrustLevel,
    intermediary_trust: TrustLevel,
    delegation_depth: u32,
    max_depth: u32,
) -> Option<TrustLevel> {
    if delegation_depth >= max_depth {
        return None;
    }
    
    let base_level = direct_trust.to_level().min(intermediary_trust.to_level());
    let decay_factor = 0.8_f64.powi(delegation_depth as i32);
    let final_level = (base_level as f64 * decay_factor) as u8;
    
    Some(TrustLevel::from_level(final_level))
}
```

Transitive trust calculation considers direct relationship strength, intermediary relationship strength, and delegation path length. Longer delegation paths produce lower trust levels. Multiple delegation paths enable trust level reinforcement through independent attestations.

Transitive relationship formation requires consent from all parties in the delegation chain. Intermediaries can decline to facilitate trust delegation. End parties can reject transitive relationships even with valid delegation chains.

## Trust Evidence and Attestations

### Interaction-Based Evidence

Trust evidence accumulates through successful interactions and positive outcomes. Communication success, resource sharing, and collaboration create positive trust evidence. Failed interactions and abuse reports create negative trust evidence.

```rust
pub struct TrustEvidence {
    pub evidence_type: EvidenceType,
    pub quality_score: f64,
    pub timestamp: u64,
    pub attestor: DeviceId,
    pub interaction_context: ContextId,
}

pub enum EvidenceType {
    SuccessfulCommunication,
    ResourceSharing,
    CollaborativeWork,
    AbuseReport,
    RecoveryAssistance,
    ReferralProvided,
}
```

Evidence collection happens automatically during normal system operation. Successful message delivery creates positive communication evidence. Resource sharing through storage protocols creates sharing evidence. Guardian assistance during recovery creates assistance evidence.

Evidence quality scores weight different interaction types based on trustworthiness indicators. Direct interactions receive higher scores than reported interactions. Recent evidence receives higher weight than historical evidence. Multiple independent sources strengthen evidence quality.

Evidence aggregation uses weighted averaging to compute overall trust scores. Evidence decay reduces the weight of old evidence over time. Negative evidence has stronger impact than positive evidence to maintain conservative trust decisions.

### Third-Party Attestations

Third-party attestations provide external validation for trust relationships. Attestations come from devices with their own trust relationships to both the attestor and subject. Multiple attestations strengthen trust evaluation beyond direct experience.

```rust
pub struct TrustAttestation {
    pub attestor: DeviceId,
    pub subject: DeviceId,
    pub relationship_id: RelationshipId,
    pub attestation_type: AttestationType,
    pub confidence_score: f64,
    pub timestamp: u64,
    pub signature: AttestationSignature,
}
```

Attestation types include reputation endorsement, capability confirmation, and abuse reporting. Reputation endorsements strengthen trust levels through social validation. Capability confirmations verify specific authorization claims. Abuse reports trigger trust degradation processes.

Attestation credibility depends on attestor trust level and relationship distance to both attestor and subject. Close relationships provide stronger attestation weight. High trust attestors provide more credible attestations. Recent attestations receive higher weight.

Attestation spam protection limits the frequency and volume of attestations per device pair. Repeated attestations from the same source have diminishing returns. Conflicting attestations trigger manual review processes.

## Trust Evolution and Maintenance

### Trust Level Progression

Trust levels evolve based on accumulated evidence and attestation patterns. Positive interactions and attestations enable trust level upgrades. Negative evidence and abuse reports trigger trust level downgrades.

Trust level changes use threshold logic to prevent spurious changes from isolated events. Multiple positive indicators must align for trust upgrades. Multiple negative indicators trigger trust downgrades. Severe negative evidence enables immediate trust termination.

```rust
pub fn evaluate_trust_level_change(
    current_level: TrustLevel,
    evidence_history: &[TrustEvidence],
    attestations: &[TrustAttestation],
    time_period: Duration,
) -> Option<TrustLevel> {
    let positive_score = calculate_positive_evidence_score(evidence_history, time_period);
    let negative_score = calculate_negative_evidence_score(evidence_history, time_period);
    let attestation_score = calculate_attestation_score(attestations, time_period);
    
    let total_score = positive_score + attestation_score - negative_score;
    
    match current_level {
        TrustLevel::None if total_score > 0.3 => Some(TrustLevel::Low),
        TrustLevel::Low if total_score > 0.6 => Some(TrustLevel::Medium),
        TrustLevel::Medium if total_score > 0.8 => Some(TrustLevel::High),
        _ if negative_score > 0.7 => Some(current_level.downgrade()),
        _ => None,
    }
}
```

Trust level progression includes cooldown periods to prevent rapid oscillation. Recent trust changes delay additional changes until sufficient time passes. Emergency procedures can override cooldown periods with sufficient evidence and attestation support.

Trust recovery enables relationship rehabilitation after negative events. Recovery requires time delays, evidence rebuilding, and attestation renewal. Successful recovery can restore previous trust levels with appropriate safeguards.

### Dispute and Revocation

Trust disputes arise when parties disagree about relationship status or evidence interpretation. Dispute resolution uses mediation through mutual trusted parties or community consensus mechanisms.

Dispute initiation creates dispute records in the journal with evidence submissions from all parties. Mediators review evidence and attestations to recommend resolution approaches. Resolution outcomes update trust relationships based on findings.

```rust
pub struct TrustDispute {
    pub dispute_id: Hash32,
    pub relationship_id: RelationshipId,
    pub initiator: DeviceId,
    pub respondent: DeviceId,
    pub dispute_type: DisputeType,
    pub evidence_submissions: Vec<EvidenceSubmission>,
    pub mediators: Vec<DeviceId>,
    pub resolution: Option<DisputeResolution>,
}
```

Relationship revocation provides permanent termination for severe trust violations. Revocation requires threshold approval from trusted third parties or clear evidence of malicious behavior. Revoked relationships create permanent negative trust facts.

Revocation propagates through transitive relationships to prevent circumvention through intermediaries. Devices with revoked relationships lose transitive trust derived from those relationships. Recovery from revocation requires extensive rehabilitation and community consensus.

## Implementation Integration

### Journal Storage

Trust relationships persist in the journal as CRDT facts using join-semilattice semantics for eventual consistency. Trust facts include relationship formation, evidence accumulation, and attestation records.

```rust
pub struct TrustRelationshipFact {
    pub relationship_id: RelationshipId,
    pub fact_type: TrustFactType,
    pub data: TrustFactData,
    pub timestamp: u64,
    pub device_signature: Signature,
}

pub enum TrustFactType {
    RelationshipFormed,
    TrustLevelChanged,
    EvidenceAdded,
    AttestationReceived,
    DisputeCreated,
    RelationshipRevoked,
}
```

Trust fact merging uses join operations that accumulate evidence over time. Conflicting facts trigger conflict resolution using timestamp ordering and evidence strength. Negative facts override positive facts for security reasons.

Journal queries provide efficient trust relationship lookups for authorization decisions. Query indexes support lookups by relationship type, trust level, and temporal validity. Cached query results improve performance for frequent authorization checks.

### Effect System Integration

Trust relationship management integrates with the effect system through `AgentEffects` trait for unified relationship operations. Effect handlers provide relationship formation, trust evaluation, and evidence collection through common interfaces.

```rust
#[async_trait]
pub trait AgentEffects: Send + Sync {
    async fn form_relationship(&self, 
        config: RelationshipFormationConfig) -> Result<RelationshipFormationResult, InvitationError>;
    async fn evaluate_trust_level(&self, 
        relationship_id: RelationshipId) -> Result<TrustLevel, WotError>;
    async fn add_trust_evidence(&self, 
        evidence: TrustEvidence) -> Result<(), WotError>;
}
```

Production handlers use real relationship formation protocols and journal storage. Testing handlers use deterministic mock relationships for reproducible tests. Simulation handlers inject relationship failures for robustness testing.

Effect composition enables relationship integration with other system components. Authorization systems use trust effects for capability evaluation. Communication systems use trust effects for budget allocation.

### Choreographic Integration

Trust relationship ceremonies use choreographic programming for deadlock-free coordination between devices. Choreographies ensure both parties complete formation steps or both parties abort consistently.

```rust
/// Sealed supertrait for relationship formation effects
pub trait RelationshipFormationEffects: NetworkEffects + CryptoEffects + StorageEffects {}
impl<T> RelationshipFormationEffects for T where T: NetworkEffects + CryptoEffects + StorageEffects {}

choreography! {
    #[namespace = "relationship_formation"]
    protocol RelationshipFormation {
        roles: Initiator, Responder;
        
        Initiator[guard_capability = "initiate_relationship", flow_cost = 30] 
        -> Responder: RelationshipInitRequest(device_id: DeviceId, trust_level: TrustLevel);
        
        Responder[guard_capability = "offer_key", flow_cost = 25, journal_facts = "key_offered"] 
        -> Initiator: RelationshipKeyOffer(public_key: Vec<u8>, capabilities: CapabilitySet);
        
        Initiator[guard_capability = "exchange_key", flow_cost = 25, journal_facts = "key_exchanged"] 
        -> Responder: RelationshipKeyExchange(public_key: Vec<u8>, signature: Vec<u8>);
        
        // Mutual validation phase
        Initiator[guard_capability = "validate_relationship", flow_cost = 20] 
        -> Responder: RelationshipValidation(proof: ValidationProof);
        
        Responder[guard_capability = "validate_relationship", flow_cost = 20] 
        -> Initiator: RelationshipValidation(proof: ValidationProof);
        
        // Final confirmation
        Initiator[guard_capability = "confirm_relationship", flow_cost = 15, journal_facts = "relationship_confirmed"] 
        -> Responder: RelationshipConfirmation(relationship_id: RelationshipId);
        
        Responder[guard_capability = "confirm_relationship", flow_cost = 15, journal_facts = "relationship_confirmed"] 
        -> Initiator: RelationshipConfirmation(relationship_id: RelationshipId);
    }
}
```

Choreographic projection generates local implementations for each role with session type safety. Protocol failures trigger consistent cleanup on both sides. Timeout handling prevents indefinite blocking during ceremony execution.

Guardian relationship ceremonies extend basic formation with threshold coordination and elevated security checks. Multiple guardians may participate in witness roles for enhanced security validation.
