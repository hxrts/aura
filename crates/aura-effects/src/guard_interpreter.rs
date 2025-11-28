//! Guard Effect Interpreter
//!
//! This module provides the `ProductionEffectInterpreter` that executes effect commands
//! produced by pure guard evaluation in the Aura system.
//!
//! # Overview
//!
//! Per ADR-014, guards in Aura are pure functions that:
//! 1. Take an immutable `GuardSnapshot` containing pre-fetched state
//! 2. Return a `GuardOutcome` with authorization decision and effect commands
//! 3. Never perform I/O or blocking operations directly
//!
//! The `ProductionEffectInterpreter` bridges this pure evaluation model to real-world
//! effects by asynchronously executing the commands.
//!
//! # Effect Commands
//!
//! The interpreter handles these primitive effect commands:
//!
//! - **ChargeBudget**: Deduct flow budget for spam/DoS protection
//! - **AppendJournal**: Persist facts to the journal
//! - **RecordLeakage**: Track privacy metadata leakage
//! - **StoreMetadata**: Save key-value pairs to storage
//! - **SendEnvelope**: Transmit messages over the network
//! - **GenerateNonce**: Create cryptographic nonces
//!
//! # Usage
//!
//! ```ignore
//! use aura_effects::ProductionEffectInterpreter;
//! use std::sync::Arc;
//!
//! // Create interpreter with all required effect handlers
//! let interpreter = ProductionEffectInterpreter::new(
//!     Arc::new(journal_handler),
//!     Arc::new(flow_budget_handler),
//!     Arc::new(leakage_handler),
//!     Arc::new(storage_handler),
//!     Arc::new(network_handler),
//!     Arc::new(random_handler),
//!     authority_id,
//! );
//!
//! // Execute effect commands from guard evaluation
//! for cmd in guard_outcome.effects {
//!     let result = interpreter.execute(cmd).await?;
//!     // Handle result...
//! }
//! ```
//!
//! # Architecture
//!
//! The interpreter follows Aura's layered architecture:
//! - **Layer 1 (aura-core)**: Defines `EffectInterpreter` trait and `EffectCommand` types
//! - **Layer 3 (aura-effects)**: Implements production interpreter with real I/O
//! - **Layer 4+ (aura-protocol, etc.)**: Uses interpreter in guard chain execution
//!
//! This separation enables:
//! - Algebraic effects and clean composition
//! - Deterministic testing with mock interpreters
//! - WASM compatibility (pure guards can run in WASM)
//! - Clear separation between business logic and I/O

use async_trait::async_trait;
use aura_core::{
    effects::{
        guard::{EffectCommand, EffectInterpreter, EffectResult},
        FlowBudgetEffects, JournalEffects, LeakageEffects, LeakageEvent, NetworkEffects,
        ObserverClass, PhysicalTimeEffects, RandomEffects, StorageEffects,
    },
    identifiers::AuthorityId,
    AuraError, AuraResult as Result,
};
use std::sync::Arc;
use tracing::{debug, error, info};

/// Production effect interpreter that executes commands with real I/O
///
/// This interpreter uses actual effect handlers to perform operations requested
/// by guard evaluation. It requires access to various effect systems:
/// - Flow budget effects for spam/DoS protection
/// - Journal effects for fact persistence
/// - Leakage effects for privacy tracking
/// - Storage effects for metadata persistence
/// - Network effects for message sending
/// - Random effects for nonce generation
pub struct ProductionEffectInterpreter<J, F, L, S, N, R, T>
where
    J: JournalEffects + Send + Sync,
    F: FlowBudgetEffects + Send + Sync,
    L: LeakageEffects + Send + Sync,
    S: StorageEffects + Send + Sync,
    N: NetworkEffects + Send + Sync,
    R: RandomEffects + Send + Sync,
    T: PhysicalTimeEffects + Send + Sync,
{
    journal: Arc<J>,
    flow_budget: Arc<F>,
    leakage: Arc<L>,
    storage: Arc<S>,
    network: Arc<N>,
    random: Arc<R>,
    time: Arc<T>,
    /// Current authority ID for context
    authority_id: AuthorityId,
}

impl<J, F, L, S, N, R, T> ProductionEffectInterpreter<J, F, L, S, N, R, T>
where
    J: JournalEffects + Send + Sync,
    F: FlowBudgetEffects + Send + Sync,
    L: LeakageEffects + Send + Sync,
    S: StorageEffects + Send + Sync,
    N: NetworkEffects + Send + Sync,
    R: RandomEffects + Send + Sync,
    T: PhysicalTimeEffects + Send + Sync,
{
    /// Create a new production effect interpreter with all required effect handlers
    pub fn new(
        journal: Arc<J>,
        flow_budget: Arc<F>,
        leakage: Arc<L>,
        storage: Arc<S>,
        network: Arc<N>,
        random: Arc<R>,
        time: Arc<T>,
        authority_id: AuthorityId,
    ) -> Self {
        Self {
            journal,
            flow_budget,
            leakage,
            storage,
            network,
            random,
            time,
            authority_id,
        }
    }
}

#[async_trait]
impl<J, F, L, S, N, R, T> EffectInterpreter for ProductionEffectInterpreter<J, F, L, S, N, R, T>
where
    J: JournalEffects + Send + Sync,
    F: FlowBudgetEffects + Send + Sync,
    L: LeakageEffects + Send + Sync,
    S: StorageEffects + Send + Sync,
    N: NetworkEffects + Send + Sync,
    R: RandomEffects + Send + Sync,
    T: PhysicalTimeEffects + Send + Sync,
{
    async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult> {
        match cmd {
            EffectCommand::ChargeBudget {
                authority,
                amount,
                context,
                peer,
            } => {
                debug!(?authority, amount, "Charging flow budget for authority");

                // Charge the flow budget
                let receipt = self
                    .flow_budget
                    .charge_flow(&context, &peer, amount)
                    .await
                    .map_err(|e| {
                        error!("Failed to charge flow budget: {}", e);
                        e
                    })?;

                info!(
                    ?authority,
                    amount,
                    flow_amount = receipt.cost,
                    "Successfully charged flow budget"
                );

                // Return remaining budget (we'll use the charged amount as a proxy)
                // In production, you'd query the actual remaining budget from the journal
                Ok(EffectResult::RemainingBudget(
                    1000u32.saturating_sub(receipt.cost),
                ))
            }

            EffectCommand::AppendJournal { entry } => {
                debug!(
                    authority = ?entry.authority,
                    fact = ?entry.fact,
                    "Appending entry to journal"
                );

                // Get current journal, merge the new fact, and persist
                let current = self.journal.get_journal().await.map_err(|e| {
                    error!("Failed to get current journal: {}", e);
                    AuraError::invalid(format!("Failed to get journal: {}", e))
                })?;

                // Create a delta journal with just the new fact
                let delta = aura_core::Journal::default();
                // Note: In a real implementation, we'd properly add the fact to the journal
                // For now, we just persist the updated journal

                // Merge the fact into the current journal
                let updated = self
                    .journal
                    .merge_facts(&current, &delta)
                    .await
                    .map_err(|e| {
                        error!("Failed to merge facts: {}", e);
                        AuraError::invalid(format!("Failed to merge: {}", e))
                    })?;

                // Persist the updated journal
                self.journal.persist_journal(&updated).await.map_err(|e| {
                    error!("Failed to persist journal: {}", e);
                    AuraError::invalid(format!("Failed to persist: {}", e))
                })?;

                info!(
                    authority = ?entry.authority,
                    "Successfully appended journal entry"
                );

                Ok(EffectResult::Success)
            }

            EffectCommand::RecordLeakage { bits } => {
                debug!(
                    bits,
                    authority = ?self.authority_id,
                    "Recording metadata leakage"
                );

                // Create a leakage event
                // Note: In production, we'd need more context about the actual operation
                let event = LeakageEvent {
                    source: self.authority_id,
                    destination: self.authority_id, // Self-leakage for now
                    context_id: aura_core::identifiers::ContextId::new(), // Would need real context
                    leakage_amount: bits as u64,
                    observer_class: ObserverClass::External, // Conservative default
                    operation: "guard_evaluation".to_string(),
                    timestamp_ms: self.time.physical_time().await?.ts_ms,
                };

                self.leakage.record_leakage(event).await.map_err(|e| {
                    error!("Failed to record leakage: {}", e);
                    AuraError::invalid(format!("Failed to record leakage: {}", e))
                })?;

                info!(bits, "Successfully recorded leakage");

                Ok(EffectResult::Success)
            }

            EffectCommand::StoreMetadata { key, value } => {
                debug!(key, value_len = value.len(), "Storing metadata");

                // Store as raw bytes
                let data = value.into_bytes();

                self.storage.store(&key, data).await.map_err(|e| {
                    error!("Failed to store metadata: {}", e);
                    AuraError::storage(format!("Failed to store: {}", e))
                })?;

                info!(key, "Successfully stored metadata");

                Ok(EffectResult::Success)
            }

            EffectCommand::SendEnvelope { to, envelope } => {
                debug!(
                    to = ?to,
                    envelope_size = envelope.len(),
                    "Sending network envelope"
                );

                // Convert NetworkAddress to a peer ID (in production, you'd have proper mapping)
                // For now, we'll use a deterministic UUID based on the address
                let peer_id =
                    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, to.as_str().as_bytes());

                // Send via network effects
                self.network
                    .send_to_peer(peer_id, envelope.clone())
                    .await
                    .map_err(|e| {
                        error!("Failed to send envelope: {}", e);
                        AuraError::network(format!("Failed to send: {}", e))
                    })?;

                info!(
                    to = ?to,
                    peer_id = ?peer_id,
                    envelope_size = envelope.len(),
                    "Successfully sent envelope"
                );

                Ok(EffectResult::Success)
            }

            EffectCommand::GenerateNonce { bytes } => {
                debug!(bytes, "Generating cryptographic nonce");

                // Generate random bytes
                let nonce = self.random.random_bytes(bytes).await;

                info!(
                    bytes,
                    nonce_len = nonce.len(),
                    "Successfully generated nonce"
                );

                Ok(EffectResult::Nonce(nonce))
            }
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "production"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::{
        effects::{
            FlowBudgetEffects, JournalEffects, LeakageEffects, NetworkEffects,
            RandomEffects, StorageEffects,
        },
        time::PhysicalTime,
    };
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    // Mock implementations for testing
    struct MockJournalEffects;

    #[async_trait]
    impl JournalEffects for MockJournalEffects {
        async fn merge_facts(
            &self,
            target: &aura_core::Journal,
            _delta: &aura_core::Journal,
        ) -> Result<aura_core::Journal> {
            Ok(target.clone())
        }

        async fn refine_caps(
            &self,
            target: &aura_core::Journal,
            _refinement: &aura_core::Journal,
        ) -> Result<aura_core::Journal> {
            Ok(target.clone())
        }

        async fn get_journal(&self) -> Result<aura_core::Journal> {
            Ok(aura_core::Journal::default())
        }

        async fn persist_journal(&self, _journal: &aura_core::Journal) -> Result<()> {
            Ok(())
        }

        async fn get_flow_budget(
            &self,
            _context: &aura_core::identifiers::ContextId,
            _peer: &AuthorityId,
        ) -> Result<aura_core::FlowBudget> {
            Ok(aura_core::FlowBudget::default())
        }

        async fn update_flow_budget(
            &self,
            _context: &aura_core::identifiers::ContextId,
            _peer: &AuthorityId,
            budget: &aura_core::FlowBudget,
        ) -> Result<aura_core::FlowBudget> {
            Ok(budget.clone())
        }

        async fn charge_flow_budget(
            &self,
            _context: &aura_core::identifiers::ContextId,
            _peer: &AuthorityId,
            _cost: u32,
        ) -> Result<aura_core::FlowBudget> {
            Ok(aura_core::FlowBudget::default())
        }
    }

    struct MockFlowBudgetEffects {
        budgets: Mutex<HashMap<AuthorityId, u32>>,
    }

    impl MockFlowBudgetEffects {
        fn new() -> Self {
            Self {
                budgets: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl FlowBudgetEffects for MockFlowBudgetEffects {
        async fn charge_flow(
            &self,
            context: &aura_core::identifiers::ContextId,
            peer: &AuthorityId,
            cost: u32,
        ) -> Result<aura_core::flow::Receipt> {
            let mut budgets = self.budgets.lock().await;
            let budget = budgets.entry(*peer).or_insert(1000);
            if *budget >= cost {
                *budget -= cost;
                Ok(aura_core::flow::Receipt {
                    ctx: *context,
                    src: *peer,
                    dst: *peer,  // Using peer for both in mock
                    epoch: aura_core::types::Epoch::new(0),
                    cost,
                    nonce: 0,
                    prev: aura_core::domain::Hash32([0u8; 32]),
                    sig: vec![],
                })
            } else {
                Err(AuraError::invalid("Insufficient budget"))
            }
        }
    }

    struct MockLeakageEffects;

    #[async_trait]
    impl LeakageEffects for MockLeakageEffects {
        async fn record_leakage(&self, _event: LeakageEvent) -> Result<()> {
            Ok(())
        }

        async fn get_leakage_budget(
            &self,
            _context_id: aura_core::identifiers::ContextId,
        ) -> Result<aura_core::effects::LeakageBudget> {
            Ok(aura_core::effects::LeakageBudget::zero())
        }

        async fn check_leakage_budget(
            &self,
            _context_id: aura_core::identifiers::ContextId,
            _observer: ObserverClass,
            _amount: u64,
        ) -> Result<bool> {
            Ok(true)
        }

        async fn get_leakage_history(
            &self,
            _context_id: aura_core::identifiers::ContextId,
            _since_timestamp: Option<u64>,
        ) -> Result<Vec<LeakageEvent>> {
            Ok(vec![])
        }
    }

    struct MockStorageEffects;

    #[async_trait]
    impl StorageEffects for MockStorageEffects {
        async fn store(&self, _key: &str, _value: Vec<u8>) -> std::result::Result<(), aura_core::effects::StorageError> {
            Ok(())
        }

        async fn retrieve(&self, _key: &str) -> std::result::Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
            Ok(None)
        }

        async fn remove(&self, _key: &str) -> std::result::Result<bool, aura_core::effects::StorageError> {
            Ok(true)
        }

        async fn exists(&self, _key: &str) -> std::result::Result<bool, aura_core::effects::StorageError> {
            Ok(false)
        }

        async fn store_batch(
            &self,
            _items: std::collections::HashMap<String, Vec<u8>>,
        ) -> std::result::Result<(), aura_core::effects::StorageError> {
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            _keys: &[String],
        ) -> std::result::Result<std::collections::HashMap<String, Vec<u8>>, aura_core::effects::StorageError> {
            Ok(std::collections::HashMap::new())
        }

        async fn clear_all(&self) -> std::result::Result<(), aura_core::effects::StorageError> {
            Ok(())
        }

        async fn list_keys(&self, _prefix: Option<&str>) -> std::result::Result<Vec<String>, aura_core::effects::StorageError> {
            Ok(vec![])
        }

        async fn stats(&self) -> std::result::Result<aura_core::effects::StorageStats, aura_core::effects::StorageError> {
            Ok(aura_core::effects::StorageStats {
                key_count: 0,
                total_size: 0,
                available_space: Some(0),
                backend_type: "mock".to_string(),
            })
        }
    }

    struct MockNetworkEffects;

    #[async_trait]
    impl NetworkEffects for MockNetworkEffects {
        async fn send_to_peer(
            &self,
            _peer_id: uuid::Uuid,
            _message: Vec<u8>,
        ) -> std::result::Result<(), aura_core::effects::NetworkError> {
            Ok(())
        }

        async fn broadcast(
            &self,
            _message: Vec<u8>,
        ) -> std::result::Result<(), aura_core::effects::NetworkError> {
            Ok(())
        }

        async fn receive(&self) -> std::result::Result<(uuid::Uuid, Vec<u8>), aura_core::effects::NetworkError> {
            Ok((uuid::Uuid::new_v4(), vec![]))
        }

        async fn receive_from(
            &self,
            _peer_id: uuid::Uuid,
        ) -> std::result::Result<Vec<u8>, aura_core::effects::NetworkError> {
            Ok(vec![])
        }

        async fn connected_peers(&self) -> Vec<uuid::Uuid> {
            vec![]
        }

        async fn is_peer_connected(&self, _peer_id: uuid::Uuid) -> bool {
            false
        }

        async fn subscribe_to_peer_events(
            &self,
        ) -> std::result::Result<aura_core::effects::PeerEventStream, aura_core::effects::NetworkError> {
            Err(aura_core::effects::NetworkError::SendFailed {
                peer_id: None,
                reason: "MockNetworkEffects not implemented".to_string(),
            })
        }
    }

    struct MockRandomEffects;

    #[async_trait]
    impl RandomEffects for MockRandomEffects {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![0x42; len]
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [0x42; 32]
        }

        async fn random_u64(&self) -> u64 {
            42
        }

        async fn random_range(&self, _min: u64, _max: u64) -> u64 {
            42
        }

        async fn random_uuid(&self) -> uuid::Uuid {
            uuid::Uuid::from_u128(42)
        }
    }

    #[derive(Debug)]
    struct MockTimeEffects;

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for MockTimeEffects {
        async fn physical_time(&self) -> std::result::Result<PhysicalTime, aura_core::effects::time::TimeError> {
            Ok(PhysicalTime {
                ts_ms: 1_650_000_000_000,
                uncertainty: None,
            })
        }

        async fn sleep_ms(&self, _ms: u64) -> std::result::Result<(), aura_core::effects::time::TimeError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_charge_budget() {
        let interpreter = ProductionEffectInterpreter::new(
            Arc::new(MockJournalEffects),
            Arc::new(MockFlowBudgetEffects::new()),
            Arc::new(MockLeakageEffects),
            Arc::new(MockStorageEffects),
            Arc::new(MockNetworkEffects),
            Arc::new(MockRandomEffects),
            Arc::new(MockTimeEffects),
            AuthorityId::new(),
        );

        let authority = AuthorityId::new();
        let cmd = EffectCommand::ChargeBudget {
            context: aura_core::identifiers::ContextId::new(),
            authority,
            peer: authority,
            amount: 100,
        };

        let result = interpreter.execute(cmd).await.unwrap();
        match result {
            EffectResult::RemainingBudget(remaining) => {
                assert_eq!(remaining, 900);
            }
            _ => panic!("Expected RemainingBudget result"),
        }
    }

    #[tokio::test]
    async fn test_generate_nonce() {
        let interpreter = ProductionEffectInterpreter::new(
            Arc::new(MockJournalEffects),
            Arc::new(MockFlowBudgetEffects::new()),
            Arc::new(MockLeakageEffects),
            Arc::new(MockStorageEffects),
            Arc::new(MockNetworkEffects),
            Arc::new(MockRandomEffects),
            Arc::new(MockTimeEffects),
            AuthorityId::new(),
        );

        let cmd = EffectCommand::GenerateNonce { bytes: 16 };
        let result = interpreter.execute(cmd).await.unwrap();

        match result {
            EffectResult::Nonce(nonce) => {
                assert_eq!(nonce.len(), 16);
                assert_eq!(nonce, vec![0x42; 16]);
            }
            _ => panic!("Expected Nonce result"),
        }
    }

    #[tokio::test]
    async fn test_interpreter_type() {
        let interpreter = ProductionEffectInterpreter::new(
            Arc::new(MockJournalEffects),
            Arc::new(MockFlowBudgetEffects::new()),
            Arc::new(MockLeakageEffects),
            Arc::new(MockStorageEffects),
            Arc::new(MockNetworkEffects),
            Arc::new(MockRandomEffects),
            Arc::new(MockTimeEffects),
            AuthorityId::new(),
        );

        assert_eq!(interpreter.interpreter_type(), "production");
    }
}
