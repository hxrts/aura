//! Receipt Manager Service
//!
//! Manages receipt chains and audit trails for flow budget charges.
//! Receipts provide cryptographic proof of budget consumption.
//!
//! ## Lifecycle Integration
//!
//! The receipt manager integrates with the runtime lifecycle via `start_cleanup_task()`.
//! When enabled, it periodically prunes expired receipts based on the configured
//! retention period.

use crate::core::AgentConfig;
use super::state::with_state_mut_validated;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::RuntimeTaskRegistry;

/// Configuration for the receipt manager
#[derive(Debug, Clone)]
pub struct ReceiptManagerConfig {
    /// Enable automatic cleanup of expired receipts
    pub auto_cleanup_enabled: bool,

    /// Interval between cleanup runs (default: 5 minutes)
    pub cleanup_interval: Duration,

    /// How long to retain receipts before pruning (default: 7 days)
    pub retention_period: Duration,
}

impl Default for ReceiptManagerConfig {
    fn default() -> Self {
        Self {
            auto_cleanup_enabled: true,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
            retention_period: Duration::from_secs(604800), // 7 days
        }
    }
}

impl ReceiptManagerConfig {
    /// Create config for testing with shorter intervals
    pub fn for_testing() -> Self {
        Self {
            auto_cleanup_enabled: true,
            cleanup_interval: Duration::from_secs(10),
            retention_period: Duration::from_secs(60), // 1 minute
        }
    }

    /// Create config with cleanup disabled
    pub fn no_cleanup() -> Self {
        Self {
            auto_cleanup_enabled: false,
            ..Default::default()
        }
    }
}

/// Unique identifier for a receipt
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiptId(pub [u8; 32]);

impl ReceiptId {
    /// Generate a new receipt ID from content hash
    pub fn from_hash(hash: &[u8]) -> Self {
        let mut id = [0u8; 32];
        let len = hash.len().min(32);
        id[..len].copy_from_slice(&hash[..len]);
        Self(id)
    }
}

/// A receipt for a flow budget charge
#[derive(Debug, Clone)]
pub struct Receipt {
    /// Unique receipt ID
    pub id: ReceiptId,
    /// Context where charge occurred
    pub context_id: ContextId,
    /// Peer authority charged
    pub peer_id: AuthorityId,
    /// Amount charged
    pub amount: u32,
    /// Timestamp (ms since epoch)
    pub timestamp: u64,
    /// Previous receipt in chain (for chaining)
    pub previous: Option<ReceiptId>,
    /// Hash of the receipt content
    pub content_hash: [u8; 32],
}

/// Receipt manager error
#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    #[error("Receipt not found: {0:?}")]
    NotFound(ReceiptId),
    #[error("Lock error")]
    LockError,
    #[error("Invalid receipt chain")]
    InvalidChain,
    #[error("Receipt verification failed")]
    VerificationFailed,
}

#[derive(Debug, Default)]
struct ReceiptState {
    receipts: HashMap<ReceiptId, Receipt>,
    chains: HashMap<(ContextId, AuthorityId), Vec<ReceiptId>>,
}

impl ReceiptState {
    fn validate(&self) -> Result<(), String> {
        for (key, chain) in &self.chains {
            for receipt_id in chain {
                if !self.receipts.contains_key(receipt_id) {
                    return Err(format!("chain {:?} references missing receipt {:?}", key, receipt_id));
                }
            }
        }
        Ok(())
    }
}

/// Receipt manager service
///
/// Manages receipt chains for flow budget audit trails. Integrates with the
/// runtime lifecycle for periodic cleanup of expired receipts.
pub struct ReceiptManager {
    #[allow(dead_code)] // Will be used for receipt configuration
    agent_config: AgentConfig,
    /// Receipt manager configuration
    config: ReceiptManagerConfig,
    /// Owned receipt state (receipts + chains)
    state: Arc<RwLock<ReceiptState>>,
}

impl ReceiptManager {
    /// Create a new receipt manager with default configuration
    pub fn new(agent_config: &AgentConfig) -> Self {
        Self::with_config(agent_config, ReceiptManagerConfig::default())
    }

    /// Create a new receipt manager with custom configuration
    pub fn with_config(agent_config: &AgentConfig, config: ReceiptManagerConfig) -> Self {
        Self {
            agent_config: agent_config.clone(),
            config,
            state: Arc::new(RwLock::new(ReceiptState::default())),
        }
    }

    /// Start the background cleanup task
    ///
    /// This integrates the receipt manager with the runtime lifecycle. The cleanup
    /// task will periodically prune expired receipts based on the retention period.
    ///
    /// The task is registered with the `RuntimeTaskRegistry` and will be automatically
    /// stopped when the runtime shuts down.
    pub fn start_cleanup_task(
        &self,
        tasks: Arc<RuntimeTaskRegistry>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) {
        if !self.config.auto_cleanup_enabled {
            tracing::debug!("Receipt cleanup task disabled by configuration");
            return;
        }

        let state = self.state.clone();
        let retention_ms = self.config.retention_period.as_millis() as u64;
        let interval = self.config.cleanup_interval;

        tasks.spawn_interval_until(time.clone(), interval, move || {
            let state = state.clone();
            let time = time.clone();

            async move {
                // Get current time
                let now_ms = match time.physical_time().await {
                    Ok(t) => t.ts_ms,
                    Err(e) => {
                        tracing::warn!("Receipt cleanup: failed to get time: {}", e);
                        return true; // Continue task
                    }
                };

                // Calculate cutoff timestamp
                let cutoff = now_ms.saturating_sub(retention_ms);

                // Prune expired receipts
                let count = with_state_mut_validated(
                    &state,
                    |state| {
                        let expired_ids: Vec<ReceiptId> = state
                            .receipts
                            .iter()
                            .filter(|(_, r)| r.timestamp < cutoff)
                            .map(|(id, _)| *id)
                            .collect();
                        let expired_set: HashSet<ReceiptId> =
                            expired_ids.iter().copied().collect();
                        let count = expired_ids.len();

                        if count > 0 {
                            for id in &expired_ids {
                                state.receipts.remove(id);
                            }

                            for chain in state.chains.values_mut() {
                                chain.retain(|id| !expired_set.contains(id));
                            }
                            state.chains.retain(|_, chain| !chain.is_empty());
                        }

                        count
                    },
                    |state| state.validate(),
                )
                .await;

                if count > 0 {
                    let remaining = state.read().await.receipts.len();
                    tracing::debug!(
                        pruned = count,
                        cutoff_ms = cutoff,
                        remaining,
                        "Pruned expired receipts"
                    );
                }

                true // Continue running
            }
        });

        tracing::info!(
            interval_secs = self.config.cleanup_interval.as_secs(),
            retention_days = self.config.retention_period.as_secs() / 86400,
            "Receipt cleanup task started"
        );
    }

    /// Get the configuration
    pub fn config(&self) -> &ReceiptManagerConfig {
        &self.config
    }

    /// Store a new receipt
    pub async fn store_receipt(&self, receipt: Receipt) -> Result<ReceiptId, ReceiptError> {
        let id = receipt.id;
        let context_id = receipt.context_id;
        let peer_id = receipt.peer_id;

        with_state_mut_validated(
            &self.state,
            |state| {
                state.receipts.insert(id, receipt);
                state.chains.entry((context_id, peer_id)).or_default().push(id);
            },
            |state| state.validate(),
        )
        .await;

        Ok(id)
    }

    /// Get a receipt by ID
    pub async fn get_receipt(&self, id: ReceiptId) -> Result<Option<Receipt>, ReceiptError> {
        let state = self.state.read().await;
        Ok(state.receipts.get(&id).cloned())
    }

    /// Get the receipt chain for a context-peer pair
    pub async fn get_receipt_chain(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<Vec<Receipt>, ReceiptError> {
        let state = self.state.read().await;
        let receipt_ids = state
            .chains
            .get(&(context, peer))
            .cloned()
            .unwrap_or_default();

        Ok(receipt_ids
            .into_iter()
            .filter_map(|id| state.receipts.get(&id).cloned())
            .collect())
    }

    /// Verify a receipt's integrity
    pub fn verify_receipt(&self, receipt: &Receipt) -> Result<bool, ReceiptError> {
        // Verify the content hash matches the receipt data
        let computed_hash = self.compute_receipt_hash(receipt);
        Ok(computed_hash == receipt.content_hash)
    }

    /// Prune receipts older than the given timestamp
    pub async fn prune_expired_receipts(
        &self,
        before_timestamp: u64,
    ) -> Result<usize, ReceiptError> {
        let count = with_state_mut_validated(
            &self.state,
            |state| {
                // Find expired receipt IDs
                let expired_ids: Vec<ReceiptId> = state
                    .receipts
                    .iter()
                    .filter(|(_, r)| r.timestamp < before_timestamp)
                    .map(|(id, _)| *id)
                    .collect();
                let expired_set: HashSet<ReceiptId> =
                    expired_ids.iter().copied().collect();

                let count = expired_ids.len();

                for id in &expired_ids {
                    state.receipts.remove(id);
                }

                for chain in state.chains.values_mut() {
                    chain.retain(|id| !expired_set.contains(id));
                }
                state.chains.retain(|_, chain| !chain.is_empty());

                count
            },
            |state| state.validate(),
        )
        .await;

        Ok(count)
    }

    /// Compute hash for a receipt
    fn compute_receipt_hash(&self, receipt: &Receipt) -> [u8; 32] {
        use aura_core::hash::hash;

        let mut data = Vec::new();
        data.extend_from_slice(receipt.context_id.as_bytes());
        data.extend_from_slice(&receipt.peer_id.to_bytes());
        data.extend_from_slice(&receipt.amount.to_le_bytes());
        data.extend_from_slice(&receipt.timestamp.to_le_bytes());
        if let Some(prev) = &receipt.previous {
            data.extend_from_slice(&prev.0);
        }

        hash(&data)
    }

    /// Create a new receipt for a charge
    pub async fn create_receipt(
        &self,
        context_id: ContextId,
        peer_id: AuthorityId,
        amount: u32,
        timestamp: u64,
    ) -> Result<Receipt, ReceiptError> {
        // Get the previous receipt in the chain
        let previous = {
            let state = self.state.read().await;
            state
                .chains
                .get(&(context_id, peer_id))
                .and_then(|chain| chain.last().copied())
        };

        // Compute the content hash
        let mut data = Vec::new();
        data.extend_from_slice(context_id.as_bytes());
        data.extend_from_slice(&peer_id.to_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&timestamp.to_le_bytes());
        if let Some(prev) = &previous {
            data.extend_from_slice(&prev.0);
        }

        let content_hash = aura_core::hash::hash(&data);
        let id = ReceiptId::from_hash(&content_hash);

        Ok(Receipt {
            id,
            context_id,
            peer_id,
            amount,
            timestamp,
            previous,
            content_hash,
        })
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

use super::traits::{RuntimeService, ServiceError, ServiceHealth};
use async_trait::async_trait;

#[async_trait]
impl RuntimeService for ReceiptManager {
    fn name(&self) -> &'static str {
        "receipt_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["flow_budget_manager"]
    }

    async fn start(&self, tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError> {
        // Start cleanup task if auto-cleanup is enabled
        // Note: This requires time effects which should be configured externally
        // The cleanup task is typically started via start_cleanup_task()
        if self.config.auto_cleanup_enabled {
            tracing::debug!(
                "ReceiptManager: auto-cleanup enabled, call start_cleanup_task() with time effects"
            );
        }
        let _ = tasks; // Acknowledge tasks param
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        // Clear all receipts on shutdown
        with_state_mut_validated(
            &self.state,
            |state| {
                state.receipts.clear();
                state.chains.clear();
            },
            |state| state.validate(),
        )
        .await;
        Ok(())
    }

    fn health(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}
