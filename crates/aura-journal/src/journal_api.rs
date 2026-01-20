//! Clean Journal API
//!
//! This module provides an API for journal operations
//! that hides CRDT implementation details.

use crate::algebra::{AccountState, OpLog};
use crate::fact::{
    Fact, FactContent, FactEncoding, FactEnvelope, FactTypeId, Journal as FactJournal,
    JournalNamespace,
};

use aura_core::effects::time::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::semilattice::JoinSemilattice;
use aura_core::time::{OrderTime, TimeDomain, TimeStamp};
use aura_core::{
    effects::{storage::StorageEffects, CryptoEffects, RandomEffects},
    AccountId, AuraError, Ed25519VerifyingKey,
};
use serde::{Deserialize, Serialize};

/// Simplified Journal interface using fact-based architecture
///
/// # Stability: STABLE
/// This is the main journal API with semver guarantees.
#[derive(Debug, Clone)]
pub struct Journal {
    /// Account state for epoch and guardian management
    account_state: AccountState,
    /// Operation log for tracking applied operations
    op_log: OpLog,
    /// Fact-based journal for new architecture
    fact_journal: FactJournal,
}

fn derive_context_for_fact(fact: &JournalFact) -> ContextId {
    let mut input = Vec::new();
    input.extend_from_slice(&fact.source_authority.to_bytes());
    input.extend_from_slice(fact.content.as_bytes());
    ContextId::new_from_entropy(hash(&input))
}

impl Journal {
    /// Create a new journal for an account
    pub async fn new(account_id: AccountId, crypto: &dyn CryptoEffects) -> Result<Self, AuraError> {
        // Generate keypair through effects system
        let (_, public_key_bytes) = crypto
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to generate keypair: {e}")))?;

        let group_key = Ed25519VerifyingKey::try_from_slice(&public_key_bytes)
            .map_err(|e| AuraError::internal(format!("Invalid group public key bytes: {e}")))?;

        // Create authority ID from account ID for namespace
        let authority_id = AuthorityId::from_uuid(account_id.0);
        let namespace = JournalNamespace::Authority(authority_id);

        Ok(Self {
            account_state: AccountState::new(account_id, group_key),
            op_log: OpLog::default(),
            fact_journal: FactJournal::new(namespace),
        })
    }

    /// Create a new journal for an account with specific group key bytes
    pub fn new_with_group_key_bytes(
        account_id: AccountId,
        group_key_bytes: [u8; 32],
    ) -> Result<Self, AuraError> {
        // Create authority ID from account ID for namespace
        let authority_id = AuthorityId::from_uuid(account_id.0);
        let namespace = JournalNamespace::Authority(authority_id);

        let group_key = Ed25519VerifyingKey::from_bytes(group_key_bytes)
            .map_err(|e| AuraError::internal(format!("Invalid group public key bytes: {e}")))?;

        Ok(Self {
            account_state: AccountState::new(account_id, group_key),
            op_log: OpLog::default(),
            fact_journal: FactJournal::new(namespace),
        })
    }

    /// Merge with another journal, consuming it
    ///
    /// Takes ownership of `other` to avoid cloning facts during merge.
    pub fn merge(&mut self, other: Journal) -> Result<(), AuraError> {
        // Merge semilattice components (these clone internally as needed)
        self.account_state = self.account_state.join(&other.account_state);
        self.op_log = self.op_log.join(&other.op_log);

        // Merge fact journals - takes ownership to avoid cloning
        self.fact_journal.join_assign(other.fact_journal);

        Ok(())
    }

    /// Add a fact to the journal using the default order-clock domain
    pub async fn add_fact(
        &mut self,
        journal_fact: JournalFact,
        random: &dyn RandomEffects,
    ) -> Result<(), AuraError> {
        // Bridge RandomEffects into an order-clock generator for the default path.
        struct RandomOrder<'a> {
            rand: &'a dyn RandomEffects,
        }
        #[async_trait::async_trait]
        impl<'a> OrderClockEffects for RandomOrder<'a> {
            async fn order_time(&self) -> Result<OrderTime, aura_core::effects::time::TimeError> {
                Ok(OrderTime(self.rand.random_bytes_32().await))
            }
        }

        self.add_fact_with_domain(
            journal_fact,
            TimeDomain::OrderClock,
            &RandomOrder { rand: random },
            None,
            None,
        )
        .await
    }

    /// Add a fact to the journal using a specified time domain
    pub async fn add_fact_with_domain(
        &mut self,
        journal_fact: JournalFact,
        domain: TimeDomain,
        order_clock: &dyn OrderClockEffects,
        physical_clock: Option<&dyn PhysicalTimeEffects>,
        logical_clock: Option<&dyn LogicalClockEffects>,
    ) -> Result<(), AuraError> {
        // Thread through effect context using the fact's source authority
        let ts = match domain {
            TimeDomain::OrderClock => {
                let id = order_clock
                    .order_time()
                    .await
                    .map_err(|e| AuraError::internal(e.to_string()))?;
                TimeStamp::OrderClock(id)
            }
            TimeDomain::PhysicalClock => {
                let clock = physical_clock.ok_or_else(|| {
                    AuraError::invalid("Physical clock requested but no provider supplied")
                })?;
                TimeStamp::PhysicalClock(
                    clock
                        .physical_time()
                        .await
                        .map_err(|e| AuraError::internal(e.to_string()))?,
                )
            }
            TimeDomain::LogicalClock => {
                let clock = logical_clock.ok_or_else(|| {
                    AuraError::invalid("Logical clock requested but no provider supplied")
                })?;
                TimeStamp::LogicalClock(
                    clock
                        .logical_now()
                        .await
                        .map_err(|e| AuraError::internal(e.to_string()))?,
                )
            }
            TimeDomain::Range => {
                return Err(AuraError::invalid(
                    "Range domain must accompany a base domain",
                ))
            }
        };
        let order = match &ts {
            TimeStamp::OrderClock(id) => id.clone(),
            // If not order clock, synthesize an order token for deterministic insertion
            _ => OrderTime(aura_core::hash::hash(format!("{:?}", &ts).as_bytes())),
        };
        let envelope = FactEnvelope {
            type_id: FactTypeId::from("journal_fact"),
            schema_version: 1,
            encoding: FactEncoding::DagCbor,
            payload: journal_fact.content.clone().into_bytes(),
        };
        let fact = Fact::new(
            order,
            ts,
            FactContent::Relational(crate::fact::RelationalFact::Generic {
                context_id: derive_context_for_fact(&journal_fact),
                envelope,
            }),
        );

        self.fact_journal.add_fact(fact)?;
        Ok(())
    }

    /// Add a relational fact directly to the journal
    ///
    /// This method allows adding pre-constructed relational facts (such as
    /// `RelationalFact::Generic`, `RelationalFact::Protocol(...)`, etc.)
    /// directly to the journal without going through the `JournalFact` wrapper.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use aura_journal::fact::{RelationalFact, FactEnvelope, FactTypeId, FactEncoding};
    ///
    /// let envelope = FactEnvelope {
    ///     type_id: FactTypeId::from("dkd_derivation"),
    ///     schema_version: 1,
    ///     encoding: FactEncoding::DagCbor,
    ///     payload: serde_json::to_vec(&metadata).unwrap(),
    /// };
    /// let fact = RelationalFact::Generic {
    ///     context_id: ContextId::from(uuid::Uuid::from_bytes([0u8; 16])),
    ///     envelope,
    /// };
    ///
    /// journal.add_relational_fact(fact, &random).await?;
    /// ```
    pub async fn add_relational_fact(
        &mut self,
        relational_fact: crate::fact::RelationalFact,
        random: &dyn RandomEffects,
    ) -> Result<Fact, AuraError> {
        // Generate order token using random bytes
        let order = OrderTime(random.random_bytes_32().await);

        // Create timestamp using order clock domain
        let timestamp = TimeStamp::OrderClock(order.clone());

        // Construct the fact with the relational content
        let fact =
            crate::fact::Fact::new(order, timestamp, FactContent::Relational(relational_fact));

        self.fact_journal.add_fact(fact.clone())?;
        Ok(fact)
    }

    /// Get account state summary
    pub fn account_summary(&self) -> AccountSummary {
        // Derive device count from authority facts in TreeState
        let device_count = self.get_device_count_from_tree_state();

        AccountSummary {
            account_id: self.account_state.account_id,
            device_count: device_count as u32,
            guardian_count: self.account_state.guardian_registry.guardians.len() as u32,
            last_epoch: self.account_state.epoch_counter.value,
        }
    }

    /// Derive device count from authority facts in TreeState
    fn get_device_count_from_tree_state(&self) -> usize {
        // Use the reduction function to derive tree state from facts
        use crate::reduction::reduce_authority;

        // Reduce the authority facts to get current tree state
        // AccountJournal always uses Authority namespace, so Ok is expected
        match reduce_authority(&self.fact_journal) {
            Ok(authority_state) => authority_state.tree_state.device_count() as usize,
            Err(_) => {
                // This should not happen for an AccountJournal
                tracing::warn!("AccountJournal has unexpected namespace, returning 0 devices");
                0
            }
        }
    }

    /// Get account ID
    pub fn account_id(&self) -> AccountId {
        self.account_state.account_id
    }

    /// Get fact journal for advanced usage
    pub fn fact_journal(&self) -> &FactJournal {
        &self.fact_journal
    }

    /// Get the number of committed facts in the journal
    pub fn committed_fact_count(&self) -> usize {
        self.fact_journal.size()
    }

    /// Get committed facts as serializable CommittedFact records
    ///
    /// This method extracts all committed facts from the journal in a
    /// format suitable for display or persistence.
    pub fn committed_facts(&self) -> Vec<CommittedFact> {
        self.fact_journal
            .iter_facts()
            .map(|fact| CommittedFact {
                timestamp: fact.timestamp.clone(),
                order: fact.order.clone(),
                content_type: match &fact.content {
                    FactContent::AttestedOp(_) => "AttestedOp".to_string(),
                    FactContent::Relational(rel) => match rel {
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::GuardianBinding { .. },
                        ) => "GuardianBinding".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::RecoveryGrant { .. },
                        ) => "RecoveryGrant".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::Consensus { .. },
                        ) => "Consensus".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::AmpChannelCheckpoint(..),
                        ) => "AmpChannelCheckpoint".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::AmpProposedChannelEpochBump(..),
                        ) => "AmpProposedChannelEpochBump".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::AmpCommittedChannelEpochBump(..),
                        ) => "AmpCommittedChannelEpochBump".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::AmpChannelPolicy(..),
                        ) => "AmpChannelPolicy".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::LeakageEvent(..),
                        ) => "LeakageEvent".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::DkgTranscriptCommit(..),
                        ) => "DkgTranscriptCommit".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::ConvergenceCert(..),
                        ) => "ConvergenceCert".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::ReversionFact(..),
                        ) => "ReversionFact".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::RotateFact(..),
                        ) => "RotateFact".to_string(),
                        crate::fact::RelationalFact::Protocol(
                            crate::fact::ProtocolRelationalFact::AmpChannelBootstrap(..),
                        ) => "AmpChannelBootstrap".to_string(),
                        crate::fact::RelationalFact::Generic { envelope, .. } => {
                            format!("Generic:{}", envelope.type_id.as_str())
                        }
                    },
                    FactContent::Snapshot(_) => "Snapshot".to_string(),
                    FactContent::RendezvousReceipt { .. } => "RendezvousReceipt".to_string(),
                },
                content_summary: match &fact.content {
                    FactContent::AttestedOp(op) => {
                        format!("{:?} -> {:?}", op.tree_op, op.new_commitment)
                    }
                    FactContent::Relational(rel) => match rel {
                        crate::fact::RelationalFact::Generic { envelope, .. } => {
                            // Try to decode payload as UTF-8 for readability
                            String::from_utf8(envelope.payload.clone())
                                .unwrap_or_else(|_| format!("{} bytes", envelope.payload.len()))
                        }
                        _ => format!("{rel:?}"),
                    },
                    FactContent::Snapshot(snap) => {
                        format!(
                            "seq={}, superseded={}",
                            snap.sequence,
                            snap.superseded_facts.len()
                        )
                    }
                    FactContent::RendezvousReceipt { envelope_id, .. } => {
                        format!("envelope={}", hex::encode(&envelope_id[..8]))
                    }
                },
            })
            .collect()
    }

    /// Get committed facts as JournalFact records for persistence
    ///
    /// This method extracts all committed facts from the journal and
    /// converts them back to `JournalFact` format for storage and replay.
    /// The source authority is derived from the journal's namespace.
    pub fn journal_facts(&self) -> Vec<JournalFact> {
        // Extract the authority from the journal namespace
        let source_authority = match &self.fact_journal.namespace {
            crate::fact::JournalNamespace::Authority(auth_id) => *auth_id,
            crate::fact::JournalNamespace::Context(ctx_id) => {
                // For context-scoped journals, derive an authority from the context
                // Pad the 16-byte context ID to 32 bytes
                let mut padded = [0u8; 32];
                padded[..16].copy_from_slice(&ctx_id.to_bytes());
                AuthorityId::new_from_entropy(padded)
            }
        };

        self.fact_journal
            .iter_facts()
            .filter_map(|fact| {
                // Only extract Generic relational facts that were added via add_fact
                match &fact.content {
                    FactContent::Relational(crate::fact::RelationalFact::Generic {
                        envelope,
                        ..
                    }) => {
                        // Try payload as UTF-8, fall back to type_id
                        let content = String::from_utf8(envelope.payload.clone())
                            .unwrap_or_else(|_| envelope.type_id.as_str().to_string());

                        Some(JournalFact {
                            content,
                            timestamp: fact.timestamp.clone(),
                            source_authority,
                        })
                    }
                    _ => None, // Skip non-Generic facts
                }
            })
            .collect()
    }

    /// Sync the journal to persistent storage
    ///
    /// This method serializes the journal state and persists it using the
    /// provided storage effects. The journal is stored under a key derived
    /// from the account ID.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // After adding facts to the journal
    /// journal.add_relational_fact(fact, &random).await?;
    ///
    /// // Sync to persistent storage
    /// journal.sync(&storage).await?;
    /// ```
    pub async fn sync(&self, storage: &dyn StorageEffects) -> Result<(), AuraError> {
        // Create storage key from account ID
        let storage_key = format!("journal/{}", self.account_state.account_id.0);

        // Serialize the journal state
        // We serialize the fact journal, account state, and op_log together
        let journal_state = JournalPersistState {
            account_state: self.account_state.clone(),
            op_log: self.op_log.clone(),
            fact_journal: self.fact_journal.clone(),
        };

        let serialized = aura_core::util::serialization::to_vec(&journal_state)
            .map_err(|e| AuraError::internal(format!("Failed to serialize journal: {e}")))?;

        // Persist to storage
        storage
            .store(&storage_key, serialized)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to persist journal: {e}")))?;

        tracing::debug!(
            account_id = ?self.account_state.account_id,
            storage_key = %storage_key,
            "Journal synced to storage"
        );

        Ok(())
    }

    /// Load a journal from persistent storage
    ///
    /// This method retrieves and deserializes a previously synced journal
    /// from storage.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let journal = Journal::load(account_id, &storage).await?;
    /// ```
    pub async fn load(
        account_id: AccountId,
        storage: &dyn StorageEffects,
    ) -> Result<Option<Self>, AuraError> {
        let storage_key = format!("journal/{}", account_id.0);

        match storage.retrieve(&storage_key).await {
            Ok(Some(data)) => {
                let state: JournalPersistState = aura_core::util::serialization::from_slice(&data)
                    .map_err(|e| {
                        AuraError::internal(format!("Failed to deserialize journal: {e}"))
                    })?;

                Ok(Some(Self {
                    account_state: state.account_state,
                    op_log: state.op_log,
                    fact_journal: state.fact_journal,
                }))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::storage(format!("Failed to load journal: {e}"))),
        }
    }
}

/// Internal state for journal persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalPersistState {
    account_state: AccountState,
    op_log: OpLog,
    fact_journal: FactJournal,
}

/// A committed fact in the journal with serializable metadata
///
/// This provides a view of facts that have been committed to the journal
/// in a format suitable for display and persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommittedFact {
    /// Timestamp of the fact (unified time system)
    pub timestamp: TimeStamp,
    /// Order token for deterministic sorting
    pub order: OrderTime,
    /// Type of the fact content (e.g., "AttestedOp", "Generic:chat")
    pub content_type: String,
    /// Human-readable summary of the content
    pub content_summary: String,
}

/// Fact to be added to the journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFact {
    /// Content of the fact being recorded
    pub content: String,
    /// Time when the fact was created (using unified time system)
    pub timestamp: TimeStamp,
    /// Authority that originated this fact
    pub source_authority: AuthorityId,
}

// Use ContextId from aura-core instead of defining our own

/// Simplified account summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    /// The account identifier
    pub account_id: AccountId,
    /// Number of devices in the account
    pub device_count: u32,
    /// Number of guardians configured for the account
    pub guardian_count: u32,
    /// Latest epoch number for this account
    pub last_epoch: u64,
}

impl AccountSummary {
    /// Create a new account summary
    pub fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            device_count: 0,
            guardian_count: 0,
            last_epoch: 0,
        }
    }
}
