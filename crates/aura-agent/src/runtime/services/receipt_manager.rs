//! Receipt Manager Service
//!
//! Manages receipt chains and audit trails for flow budget charges.
//! Receipts provide cryptographic proof of budget consumption.

use crate::core::AgentConfig;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;
use tokio::sync::RwLock;

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

/// Receipt manager service
pub struct ReceiptManager {
    #[allow(dead_code)] // Will be used for receipt configuration
    config: AgentConfig,
    /// Receipt storage by ID
    receipts: RwLock<HashMap<ReceiptId, Receipt>>,
    /// Chain index: (ContextId, AuthorityId) -> list of ReceiptIds in order
    chains: RwLock<HashMap<(ContextId, AuthorityId), Vec<ReceiptId>>>,
}

impl ReceiptManager {
    /// Create a new receipt manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
            receipts: RwLock::new(HashMap::new()),
            chains: RwLock::new(HashMap::new()),
        }
    }

    /// Store a new receipt
    pub async fn store_receipt(&self, receipt: Receipt) -> Result<ReceiptId, ReceiptError> {
        let id = receipt.id;
        let context_id = receipt.context_id;
        let peer_id = receipt.peer_id;

        // Store the receipt
        {
            let mut receipts = self.receipts.write().await;
            receipts.insert(id, receipt);
        }

        // Update the chain index
        {
            let mut chains = self.chains.write().await;
            chains.entry((context_id, peer_id)).or_default().push(id);
        }

        Ok(id)
    }

    /// Get a receipt by ID
    pub async fn get_receipt(&self, id: ReceiptId) -> Result<Option<Receipt>, ReceiptError> {
        let receipts = self.receipts.read().await;
        Ok(receipts.get(&id).cloned())
    }

    /// Get the receipt chain for a context-peer pair
    pub async fn get_receipt_chain(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<Vec<Receipt>, ReceiptError> {
        let chains = self.chains.read().await;
        let receipts = self.receipts.read().await;

        let receipt_ids = chains.get(&(context, peer)).cloned().unwrap_or_default();

        Ok(receipt_ids
            .into_iter()
            .filter_map(|id| receipts.get(&id).cloned())
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
        let mut receipts = self.receipts.write().await;
        let mut chains = self.chains.write().await;

        // Find expired receipt IDs
        let expired_ids: Vec<ReceiptId> = receipts
            .iter()
            .filter(|(_, r)| r.timestamp < before_timestamp)
            .map(|(id, _)| *id)
            .collect();

        let count = expired_ids.len();

        // Remove from receipts
        for id in &expired_ids {
            receipts.remove(id);
        }

        // Remove from chains
        for chain in chains.values_mut() {
            chain.retain(|id| !expired_ids.contains(id));
        }

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
            let chains = self.chains.read().await;
            chains
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
