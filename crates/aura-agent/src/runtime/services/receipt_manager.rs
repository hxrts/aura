//! Receipt Manager Service
//!
//! Manages receipt chains and audit trails for flow budget charges.
//! Receipts provide cryptographic proof of budget consumption.
//!
//! ## Lifecycle Integration
//!
//! The receipt manager integrates with the runtime lifecycle through
//! `RuntimeService::start()`. When enabled, it periodically prunes expired
//! receipts based on the configured retention period.

use super::config_profiles::impl_service_config_profiles;
use super::state::with_state_mut_validated;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::core::AgentConfig;
use crate::runtime::TaskGroup;
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

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

impl_service_config_profiles!(ReceiptManagerConfig {
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
});

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

#[derive(Default)]
struct ReceiptState {
    receipts: HashMap<ReceiptId, Receipt>,
    chains: HashMap<(ContextId, AuthorityId), Vec<ReceiptId>>,
}

impl ReceiptState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for (key, chain) in &self.chains {
            for receipt_id in chain {
                if !self.receipts.contains_key(receipt_id) {
                    return Err(super::invariant::InvariantViolation::new(
                        "ReceiptManager",
                        format!(
                            "chain {:?} references missing receipt {:?}",
                            key, receipt_id
                        ),
                    ));
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
    shared: Arc<ReceiptManagerShared>,
}

struct ReceiptManagerShared {
    /// Owned receipt state (receipts + chains)
    state: RwLock<ReceiptState>,
    /// Authoritative lifecycle state for runtime health and shutdown.
    lifecycle: RwLock<ServiceHealth>,
    /// Owned cleanup task group for receipt-local maintenance.
    cleanup_tasks: RwLock<Option<TaskGroup>>,
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
            shared: Arc::new(ReceiptManagerShared {
                state: RwLock::new(ReceiptState::default()),
                lifecycle: RwLock::new(ServiceHealth::NotStarted),
                cleanup_tasks: RwLock::new(None),
            }),
        }
    }

    /// Start the background cleanup task.
    fn spawn_cleanup_task(&self, tasks: TaskGroup, time: Arc<dyn PhysicalTimeEffects>) {
        if !self.config.auto_cleanup_enabled {
            tracing::debug!("Receipt cleanup task disabled by configuration");
            return;
        }

        let shared = Arc::clone(&self.shared);
        let retention_ms = self.config.retention_period.as_millis() as u64;
        let interval = self.config.cleanup_interval;

        let _cleanup_task_handle = tasks.spawn_interval_until_named(
            "receipt.cleanup",
            time.clone(),
            interval,
            move || {
                let shared = Arc::clone(&shared);
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
                        &shared.state,
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
                        let remaining = shared.state.read().await.receipts.len();
                        tracing::debug!(
                            pruned = count,
                            cutoff_ms = cutoff,
                            remaining,
                            "Pruned expired receipts"
                        );
                    }

                    true // Continue running
                }
            },
        );

        tracing::info!(
            interval_secs = self.config.cleanup_interval.as_secs(),
            retention_days = self.config.retention_period.as_secs() / 86400,
            "Receipt cleanup task started"
        );
    }

    async fn set_lifecycle(&self, health: ServiceHealth) {
        *self.shared.lifecycle.write().await = health;
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
            &self.shared.state,
            |state| {
                state.receipts.insert(id, receipt);
                state
                    .chains
                    .entry((context_id, peer_id))
                    .or_default()
                    .push(id);
            },
            |state| state.validate(),
        )
        .await;

        Ok(id)
    }

    /// Get a receipt by ID
    pub async fn get_receipt(&self, id: ReceiptId) -> Result<Option<Receipt>, ReceiptError> {
        let state = self.shared.state.read().await;
        Ok(state.receipts.get(&id).cloned())
    }

    /// Get the receipt chain for a context-peer pair
    pub async fn get_receipt_chain(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<Vec<Receipt>, ReceiptError> {
        let state = self.shared.state.read().await;
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
            &self.shared.state,
            |state| {
                // Find expired receipt IDs
                let expired_ids: Vec<ReceiptId> = state
                    .receipts
                    .iter()
                    .filter(|(_, r)| r.timestamp < before_timestamp)
                    .map(|(id, _)| *id)
                    .collect();
                let expired_set: HashSet<ReceiptId> = expired_ids.iter().copied().collect();

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
            let state = self.shared.state.read().await;
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for ReceiptManager {
    fn name(&self) -> &'static str {
        "receipt_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["flow_budget_manager"]
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.set_lifecycle(ServiceHealth::Starting).await;
        let cleanup_group = context.tasks().group(self.name());
        self.spawn_cleanup_task(cleanup_group.clone(), context.time_effects());
        *self.shared.cleanup_tasks.write().await = Some(cleanup_group);
        self.set_lifecycle(ServiceHealth::Healthy).await;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.set_lifecycle(ServiceHealth::Stopping).await;
        if let Some(task_group) = self.shared.cleanup_tasks.write().await.take() {
            task_group
                .shutdown_with_timeout(Duration::from_secs(2))
                .await
                .map_err(|error| {
                    ServiceError::shutdown_failed(
                        "receipt_manager",
                        format!("failed to stop cleanup task group: {error}"),
                    )
                })?;
        }
        // Clear all receipts on shutdown
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.receipts.clear();
                state.chains.clear();
            },
            |state| state.validate(),
        )
        .await;
        self.set_lifecycle(ServiceHealth::Stopped).await;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.shared.lifecycle.read().await.clone()
    }
}
