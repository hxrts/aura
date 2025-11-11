# Journal System

The Journal System implements distributed state management using CRDT semilattice operations for threshold identity and account data. Journal and Ledger form complementary layers with distinct responsibilities. The Journal handles high-level CRDT semantics while the Ledger provides primitive operations and storage.

This system enables conflict-free replication across devices without coordination protocols. All state changes follow mathematical laws that guarantee convergence. The architecture supports offline operation with eventual consistency.

See [CRDT Programming Guide](802_crdt_programming_guide.md) for implementation patterns. See [Ratchet Tree](300_ratchet_tree.md) for tree operation details.

---

## Journal Layer Architecture

**Core Structure** implements the formal `Journal { facts: Fact, caps: Cap }` semilattice model for distributed state management. Facts accumulate knowledge through join operations. Capabilities refine authority through meet operations.

```rust
pub struct Journal {
    pub facts: Fact,    // Join-semilattice: knowledge accumulation
    pub caps: Cap,      // Meet-semilattice: capability refinement
}

impl Journal {
    pub fn merge(&self, other: &Journal) -> Journal {
        Journal {
            facts: self.facts.join(&other.facts),
            caps: self.caps.meet(&other.caps),
        }
    }
}
```

The dual-semilattice structure ensures safe distributed operation. Knowledge can only grow through fact accumulation. Authority can only shrink through capability intersection.

**Fact Management** stores immutable records of system events and state changes. Facts form a join-semilattice where merge operations combine information from different sources without conflicts.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fact {
    pub fact_id: FactId,
    pub timestamp: u64,
    pub device_id: DeviceId,
    pub fact_type: FactType,
    pub content: FactContent,
    pub signature: Signature,
}

pub enum FactType {
    TreeOperation(TreeOp),
    DeviceRegistration(DeviceInfo),
    CapabilityGrant(CapabilitySet),
    TrustRelationship(TrustLevel),
    PolicyUpdate(PolicyChange),
}
```

Facts include cryptographic signatures for authenticity verification. Each fact references its creator device and includes temporal ordering information.

**Intent Pool** manages pending operations before commitment to the permanent fact log. The intent pool uses OR-set semantics for high availability during coordination protocols.

```rust
pub struct IntentPool {
    pending_operations: BTreeMap<IntentId, Intent>,
    commitment_threshold: usize,
    timeout_duration: Duration,
}

pub struct Intent {
    pub operation: TreeOp,
    pub proposer: DeviceId,
    pub supporters: BTreeSet<DeviceId>,
    pub created_at: u64,
    pub expires_at: u64,
}

impl IntentPool {
    pub fn add_intent(&mut self, intent: Intent) -> IntentId {
        let intent_id = IntentId::new();
        self.pending_operations.insert(intent_id, intent);
        intent_id
    }

    pub fn support_intent(&mut self, intent_id: IntentId, supporter: DeviceId) -> bool {
        if let Some(intent) = self.pending_operations.get_mut(&intent_id) {
            intent.supporters.insert(supporter);
            intent.supporters.len() >= self.commitment_threshold
        } else {
            false
        }
    }
}
```

Intent pools enable non-blocking operation proposals. Devices can propose changes and collect support asynchronously before commitment.

**Operation Processing** handles the lifecycle from intent creation through fact commitment. Processing includes validation, threshold collection, and permanent storage.

```rust
pub async fn process_tree_operation(
    operation: TreeOp,
    journal: &mut Journal,
    effects: &JournalEffects,
) -> Result<FactId, JournalError> {
    // Phase 1: Validate operation
    let validation_result = validate_tree_operation(&operation, &journal.facts)?;
    
    // Phase 2: Create intent
    let intent = Intent::new(operation.clone(), effects.current_device_id());
    let intent_id = journal.intent_pool.add_intent(intent);
    
    // Phase 3: Collect threshold support
    let supporters = collect_threshold_support(intent_id, &operation, effects).await?;
    
    // Phase 4: Create attested operation
    let attested_op = AttestedOp {
        operation,
        attestations: supporters,
        committed_at: effects.current_timestamp(),
    };
    
    // Phase 5: Append to fact log
    let fact = Fact::from_attested_operation(attested_op);
    let fact_id = effects.append_fact(fact.clone()).await?;
    
    // Phase 6: Update journal state
    journal.facts = journal.facts.join(&FactSet::from(fact));
    
    Ok(fact_id)
}
```

Operation processing ensures cryptographic validity and threshold approval before commitment. Each phase includes proper error handling and rollback mechanisms.

## Ledger Layer

**Primitive Operations** provide the foundation for Journal functionality through cryptographic and storage primitives. The Ledger handles low-level operations while Journal manages high-level semantics.

```rust
#[async_trait]
pub trait LedgerEffects: Send + Sync {
    async fn append_event(&self, event: Event) -> Result<EventId, LedgerError>;
    async fn current_epoch(&self) -> Result<u64, LedgerError>;
    async fn hash_data(&self, data: &[u8]) -> Result<Hash, LedgerError>;
    async fn verify_signature(&self, signature: &Signature, data: &[u8], public_key: &PublicKey) -> Result<bool, LedgerError>;
    async fn get_device_info(&self, device_id: DeviceId) -> Result<DeviceInfo, LedgerError>;
}
```

Ledger effects provide stable primitives that support multiple high-level components. The interface abstracts storage and cryptographic details from Journal logic.

**Event Sourcing** maintains an append-only log of system events with epoch management. Events provide the foundation for fact derivation and state reconstruction.

```rust
pub struct Event {
    pub event_id: EventId,
    pub epoch: u64,
    pub timestamp: u64,
    pub device_id: DeviceId,
    pub event_type: EventType,
    pub payload: Vec<u8>,
}

pub enum EventType {
    FactAppended,
    EpochRotated,
    DeviceRegistered,
    PolicyChanged,
}

pub fn derive_facts_from_events(events: &[Event]) -> Result<FactSet, LedgerError> {
    let mut facts = FactSet::new();
    
    for event in events {
        match event.event_type {
            EventType::FactAppended => {
                let fact: Fact = bincode::deserialize(&event.payload)?;
                facts.insert(fact);
            }
            EventType::DeviceRegistered => {
                let device_fact = create_device_registration_fact(event)?;
                facts.insert(device_fact);
            }
            _ => {} // Other event types handled elsewhere
        }
    }
    
    Ok(facts)
}
```

Event sourcing enables complete state reconstruction and provides audit trails for all system changes. Events include cryptographic signatures and temporal ordering.

**Device Management** tracks device authorization, metadata, and activity within the system. Device information supports authorization decisions and relationship formation.

```rust
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub public_key: PublicKey,
    pub capabilities: CapabilitySet,
    pub registered_at: u64,
    pub last_active: u64,
    pub metadata: DeviceMetadata,
}

pub async fn authorize_device_operation(
    device_id: DeviceId,
    operation: &Operation,
    ledger: &dyn LedgerEffects,
) -> Result<bool, LedgerError> {
    let device_info = ledger.get_device_info(device_id).await?;
    
    // Check device capabilities
    if !device_info.capabilities.contains_all(&operation.required_capabilities()) {
        return Ok(false);
    }
    
    // Check device activity
    let current_time = ledger.current_timestamp().await?;
    if current_time - device_info.last_active > DEVICE_TIMEOUT_SECONDS {
        return Ok(false);
    }
    
    Ok(true)
}
```

Device management integrates with capability-based authorization to ensure only authorized devices can perform operations.

## CRDT Integration Patterns

**Merge Strategies** handle concurrent updates from multiple devices using semilattice operations. Different merge strategies apply based on data types and consistency requirements.

```rust
pub trait CRDTMerge {
    fn merge(&self, other: &Self) -> Self;
    fn is_concurrent_with(&self, other: &Self) -> bool;
}

impl CRDTMerge for Journal {
    fn merge(&self, other: &Journal) -> Journal {
        Journal {
            facts: self.facts.join(&other.facts),
            caps: self.caps.meet(&other.caps),
        }
    }
    
    fn is_concurrent_with(&self, other: &Journal) -> bool {
        !self.facts.dominates(&other.facts) && !other.facts.dominates(&self.facts)
    }
}

pub fn merge_journal_updates(
    local: Journal,
    remote_updates: Vec<Journal>,
) -> Journal {
    let mut merged = local;
    
    for update in remote_updates {
        merged = merged.merge(&update);
    }
    
    merged
}
```

CRDT merge operations ensure consistent state across all devices without coordination protocols. Merge commutativity and associativity guarantee convergence.

**Conflict Resolution** handles cases where automatic CRDT merging cannot determine correct outcomes. Resolution strategies depend on operation semantics and policy requirements.

```rust
pub enum ConflictResolution {
    LastWriterWins { timestamp_field: String },
    DevicePriority { device_ordering: Vec<DeviceId> },
    PolicyBased { resolution_policy: ResolutionPolicy },
    ManualReview { review_queue: ReviewQueue },
}

pub fn resolve_conflicting_facts(
    conflicting_facts: Vec<Fact>,
    resolution_strategy: ConflictResolution,
) -> Result<Fact, ConflictError> {
    match resolution_strategy {
        ConflictResolution::LastWriterWins { .. } => {
            Ok(conflicting_facts.into_iter()
                .max_by_key(|fact| fact.timestamp)
                .unwrap())
        }
        ConflictResolution::DevicePriority { device_ordering } => {
            for device_id in device_ordering {
                if let Some(fact) = conflicting_facts.iter()
                    .find(|fact| fact.device_id == device_id) {
                    return Ok(fact.clone());
                }
            }
            Err(ConflictError::NoAuthorizedDevice)
        }
        _ => Err(ConflictError::UnsupportedStrategy),
    }
}
```

Conflict resolution provides deterministic outcomes when CRDT operations cannot automatically merge. Resolution strategies preserve system invariants and policy requirements.

## Performance Optimization

**State Compaction** reduces memory usage by removing redundant information while preserving CRDT properties. Compaction strategies balance memory usage with synchronization efficiency.

```rust
pub struct JournalCompactionStrategy {
    pub fact_retention_days: u32,
    pub intent_timeout_hours: u32,
    pub snapshot_interval_hours: u32,
}

pub fn compact_journal_state(
    journal: &mut Journal,
    strategy: &JournalCompactionStrategy,
) -> Result<CompactionStats, CompactionError> {
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let retention_cutoff = current_time - (strategy.fact_retention_days as u64 * 24 * 3600);
    
    // Remove expired intents
    let intent_cutoff = current_time - (strategy.intent_timeout_hours as u64 * 3600);
    let removed_intents = journal.intent_pool.remove_expired(intent_cutoff);
    
    // Compact fact storage
    let removed_facts = journal.facts.compact_before_timestamp(retention_cutoff);
    
    Ok(CompactionStats {
        removed_intents,
        removed_facts,
        memory_freed: removed_intents.len() + removed_facts.len(),
    })
}
```

Compaction preserves essential state while reducing storage requirements. Snapshot mechanisms enable state reconstruction after aggressive compaction.

**Query Optimization** improves performance for common Journal operations through indexing and caching strategies.

```rust
pub struct JournalIndexes {
    pub device_facts: BTreeMap<DeviceId, BTreeSet<FactId>>,
    pub operation_facts: BTreeMap<TreeOpType, BTreeSet<FactId>>,
    pub timestamp_facts: BTreeMap<u64, BTreeSet<FactId>>,
}

impl JournalIndexes {
    pub fn query_device_operations(&self, device_id: DeviceId, operation_type: TreeOpType) -> Vec<FactId> {
        let device_facts = self.device_facts.get(&device_id).cloned().unwrap_or_default();
        let operation_facts = self.operation_facts.get(&operation_type).cloned().unwrap_or_default();
        
        device_facts.intersection(&operation_facts).cloned().collect()
    }
    
    pub fn update_indexes(&mut self, new_fact: &Fact) {
        // Update device index
        self.device_facts
            .entry(new_fact.device_id)
            .or_default()
            .insert(new_fact.fact_id);
            
        // Update operation type index
        if let FactType::TreeOperation(ref op) = new_fact.fact_type {
            self.operation_facts
                .entry(op.operation_type())
                .or_default()
                .insert(new_fact.fact_id);
        }
        
        // Update timestamp index
        self.timestamp_facts
            .entry(new_fact.timestamp)
            .or_default()
            .insert(new_fact.fact_id);
    }
}
```

Index structures enable efficient queries without scanning complete fact sets. Indexes update incrementally as new facts are added to the journal.

## Troubleshooting

**Common Issues** and their resolutions help maintain system reliability and diagnose problems quickly.

**Intent Timeout Problems**: Intents fail to achieve threshold support within timeout periods. Check network connectivity between devices. Verify threshold configuration matches available devices. Increase timeout duration for high-latency networks.

**Fact Ordering Conflicts**: Facts arrive in different orders on different devices causing inconsistent state. Verify timestamp synchronization across devices. Check fact dependency chains for circular references. Use deterministic conflict resolution based on fact hashes.

**Memory Growth**: Journal state grows without bound consuming available memory. Configure fact retention policies for historical data. Enable periodic compaction with appropriate intervals. Implement snapshot mechanisms for state reconstruction.

**Synchronization Delays**: Journal updates take excessive time to propagate between devices. Optimize network communication protocols. Reduce fact payload sizes through compression. Implement delta synchronization for large journals.

**Debugging Tools** assist in diagnosing Journal system issues and validating correct operation.

```rust
pub fn validate_journal_invariants(journal: &Journal) -> Vec<InvariantViolation> {
    let mut violations = Vec::new();
    
    // Check semilattice properties
    if !journal.facts.satisfies_join_laws() {
        violations.push(InvariantViolation::JoinLawViolation);
    }
    
    if !journal.caps.satisfies_meet_laws() {
        violations.push(InvariantViolation::MeetLawViolation);
    }
    
    // Check fact consistency
    for fact in &journal.facts.all_facts() {
        if !verify_fact_signature(fact) {
            violations.push(InvariantViolation::InvalidFactSignature(fact.fact_id));
        }
    }
    
    violations
}
```

Invariant checking validates that Journal state maintains mathematical properties. Regular validation during development catches implementation bugs early.
