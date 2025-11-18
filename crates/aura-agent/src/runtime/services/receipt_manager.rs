//! Receipt chain management service
//!
//! Provides isolated management of receipt chains with atomic operations
//! for tracking flow budget charges across contexts.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use aura_core::relationships::ContextId;
use aura_core::{AuraResult, Receipt};

/// A chain of receipts for tracking flow budget charges
#[derive(Clone, Debug)]
pub struct ReceiptChain {
    /// The context this chain belongs to
    pub context: ContextId,
    /// Ordered list of receipts (newest last)
    pub receipts: Vec<Receipt>,
    /// The current head receipt hash
    pub head: Option<[u8; 32]>,
}

impl ReceiptChain {
    /// Create a new receipt chain
    pub fn new(context: ContextId) -> Self {
        Self {
            context,
            receipts: Vec::new(),
            head: None,
        }
    }

    /// Add a receipt to the chain
    pub fn add(&mut self, receipt: Receipt) {
        // Convert signature to fixed-size hash for head tracking
        let mut head_hash = [0u8; 32];
        let len = receipt.sig.len().min(32);
        head_hash[..len].copy_from_slice(&receipt.sig[..len]);
        self.head = Some(head_hash);
        self.receipts.push(receipt);
    }

    /// Get the latest receipt
    pub fn latest(&self) -> Option<&Receipt> {
        self.receipts.last()
    }

    /// Get receipt count
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Check if chain is empty
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    /// Get receipts within a range
    pub fn range(&self, start: usize, end: usize) -> &[Receipt] {
        let end = end.min(self.receipts.len());
        if start < self.receipts.len() {
            &self.receipts[start..end]
        } else {
            &[]
        }
    }
}

/// Manages receipt chains in isolation from effect execution
#[derive(Clone)]
pub struct ReceiptManager {
    /// Receipt chains indexed by context
    chains: Arc<RwLock<HashMap<ContextId, ReceiptChain>>>,
}

impl ReceiptManager {
    /// Create a new receipt manager
    pub fn new() -> Self {
        Self {
            chains: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a receipt to a context's chain
    ///
    /// This method takes a brief write lock to add the receipt,
    /// then releases it immediately.
    pub async fn add_receipt(&self, context: ContextId, receipt: Receipt) -> AuraResult<()> {
        let mut chains = self.chains.write().await;
        let chain = chains
            .entry(context.clone())
            .or_insert_with(|| ReceiptChain::new(context));

        chain.add(receipt);
        Ok(())
    }

    /// Get the latest receipt for a context
    ///
    /// This method takes a brief read lock to get the receipt,
    /// then releases it immediately.
    pub async fn latest_receipt(&self, context: &ContextId) -> AuraResult<Option<Receipt>> {
        let chains = self.chains.read().await;
        Ok(chains
            .get(context)
            .and_then(|chain| chain.latest().cloned()))
    }

    /// Get the head hash for a context's receipt chain
    pub async fn head_hash(&self, context: &ContextId) -> AuraResult<Option<[u8; 32]>> {
        let chains = self.chains.read().await;
        Ok(chains.get(context).and_then(|chain| chain.head))
    }

    /// Get a snapshot of a receipt chain
    pub async fn get_chain(&self, context: &ContextId) -> AuraResult<Option<ReceiptChain>> {
        let chains = self.chains.read().await;
        Ok(chains.get(context).cloned())
    }

    /// Initialize a receipt chain if it doesn't exist
    pub async fn initialize_chain(&self, context: ContextId) -> AuraResult<()> {
        let mut chains = self.chains.write().await;
        chains
            .entry(context.clone())
            .or_insert_with(|| ReceiptChain::new(context));
        Ok(())
    }

    /// Get the number of receipts in a chain
    pub async fn chain_length(&self, context: &ContextId) -> AuraResult<usize> {
        let chains = self.chains.read().await;
        Ok(chains.get(context).map(|chain| chain.len()).unwrap_or(0))
    }

    /// Remove a receipt chain
    pub async fn remove_chain(&self, context: &ContextId) -> AuraResult<Option<ReceiptChain>> {
        let mut chains = self.chains.write().await;
        Ok(chains.remove(context))
    }

    /// Clear all receipt chains (useful for testing)
    pub async fn clear(&self) {
        let mut chains = self.chains.write().await;
        chains.clear();
    }

    /// Get the number of managed chains
    pub async fn len(&self) -> usize {
        let chains = self.chains.read().await;
        chains.len()
    }

    /// Check if the manager is empty
    pub async fn is_empty(&self) -> bool {
        let chains = self.chains.read().await;
        chains.is_empty()
    }

    /// Get all context IDs with receipt chains
    pub async fn context_ids(&self) -> Vec<ContextId> {
        let chains = self.chains.read().await;
        chains.keys().cloned().collect()
    }

    /// Get receipts within a range for a context
    pub async fn get_receipts_range(
        &self,
        context: &ContextId,
        start: usize,
        count: usize,
    ) -> AuraResult<Vec<Receipt>> {
        let chains = self.chains.read().await;
        if let Some(chain) = chains.get(context) {
            Ok(chain.range(start, start + count).to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    /// Verify receipt chain integrity
    pub async fn verify_chain(&self, context: &ContextId) -> AuraResult<bool> {
        let chains = self.chains.read().await;
        if let Some(chain) = chains.get(context) {
            // Verify each receipt's previous hash matches
            for i in 1..chain.receipts.len() {
                // For testing, we'll compare using a simplified hash
                // In reality, we would verify the actual signature here
                let prev_hash = if i > 0 {
                    // Create a hash from the previous receipt's signature
                    let mut hash = [0u8; 32];
                    let sig_bytes = &chain.receipts[i - 1].sig;
                    let len = sig_bytes.len().min(32);
                    hash[..len].copy_from_slice(&sig_bytes[..len]);
                    hash
                } else {
                    [0u8; 32]
                };
                if chain.receipts[i].prev.0 != prev_hash {
                    return Ok(false);
                }
            }
            Ok(true)
        } else {
            Ok(true) // Empty chain is valid
        }
    }
}

impl Default for ReceiptManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::session_epochs::Epoch;
    use aura_core::AuraResult;
    use aura_core::DeviceId;
    use aura_macros::aura_test;
    use aura_testkit::{ TestFixture};

    fn create_test_receipt(
        context: ContextId,
        src: DeviceId,
        dst: DeviceId,
        nonce: u64,
        previous: [u8; 32],
    ) -> Receipt {
        Receipt {
            ctx: context,
            src,
            dst,
            epoch: Epoch::from(1),
            cost: 100,
            nonce,
            prev: aura_core::Hash32(previous),
            sig: {
                let mut sig = [0u8; 32];
                sig[0..8].copy_from_slice(&nonce.to_le_bytes());
                sig.to_vec()
            },
        }
    }

    #[aura_test]
    async fn test_receipt_chain() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ReceiptManager::new();
        let context = ContextId::from("test-context");
        let src = fixture.device_id();
        let dst = fixture.create_device_id();

        // Add receipts
        let receipt1 = create_test_receipt(context.clone(), src, dst, 1, [0u8; 32]);
        manager
            .add_receipt(context.clone(), receipt1.clone())
            .await?;

        let mut prev_hash = [0u8; 32];
        prev_hash[..receipt1.sig.len().min(32)]
            .copy_from_slice(&receipt1.sig[..receipt1.sig.len().min(32)]);
        let receipt2 = create_test_receipt(context.clone(), src, dst, 2, prev_hash);
        manager
            .add_receipt(context.clone(), receipt2.clone())
            .await?;

        // Check latest receipt
        let latest = manager.latest_receipt(&context).await?.unwrap();
        assert_eq!(latest.nonce, 2);

        // Check head hash
        let head = manager.head_hash(&context).await?.unwrap();
        assert_eq!(head, receipt2.sig.as_slice());

        // Check chain length
        let length = manager.chain_length(&context).await?;
        assert_eq!(length, 2);
        Ok(())
    }

    #[aura_test]
    async fn test_chain_verification() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ReceiptManager::new();
        let context = ContextId::from("test-context");
        let src = fixture.device_id();
        let dst = fixture.create_device_id();

        // Build valid chain
        let receipt1 = create_test_receipt(context.clone(), src, dst, 1, [0u8; 32]);
        manager
            .add_receipt(context.clone(), receipt1.clone())
            .await?;

        let mut prev_hash2 = [0u8; 32];
        prev_hash2[..receipt1.sig.len().min(32)]
            .copy_from_slice(&receipt1.sig[..receipt1.sig.len().min(32)]);
        let receipt2 = create_test_receipt(context.clone(), src, dst, 2, prev_hash2);
        manager.add_receipt(context.clone(), receipt2).await?;

        // Verify chain
        assert!(manager.verify_chain(&context).await?);
        Ok(())
    }

    #[aura_test]
    async fn test_range_queries() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ReceiptManager::new();
        let context = ContextId::from("test-context");
        let src = fixture.device_id();
        let dst = fixture.create_device_id();

        // Add multiple receipts
        let mut prev_hash = [0u8; 32];
        for i in 1..=10 {
            let receipt = create_test_receipt(context.clone(), src, dst, i, prev_hash);
            prev_hash[..receipt.sig.len().min(32)]
                .copy_from_slice(&receipt.sig[..receipt.sig.len().min(32)]);
            manager.add_receipt(context.clone(), receipt).await?;
        }

        // Get range
        let range = manager.get_receipts_range(&context, 2, 3).await?;
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].nonce, 3);
        assert_eq!(range[1].nonce, 4);
        assert_eq!(range[2].nonce, 5);
        Ok(())
    }

    #[aura_test]
    async fn test_concurrent_access() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ReceiptManager::new();
        let context = ContextId::from("test-context");
        let src = fixture.device_id();
        let dst = fixture.create_device_id();

        // Spawn concurrent writers
        let mut handles = vec![];
        for i in 0..10 {
            let mgr = manager.clone();
            let ctx = context.clone();
            let handle = tokio::spawn(async move {
                let receipt = create_test_receipt(ctx.clone(), src, dst, i, [0u8; 32]);
                mgr.add_receipt(ctx, receipt).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all writers
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all receipts were added
        let length = manager.chain_length(&context).await?;
        assert_eq!(length, 10);
        Ok(())
    }

    #[aura_test]
    async fn test_chain_removal() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ReceiptManager::new();
        let context = ContextId::from("test-context");
        let src = fixture.device_id();
        let dst = fixture.create_device_id();

        // Add receipt
        let receipt = create_test_receipt(context.clone(), src, dst, 1, [0u8; 32]);
        manager.add_receipt(context.clone(), receipt).await?;

        // Remove chain
        let removed = manager.remove_chain(&context).await?;
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().len(), 1);

        // Verify chain is gone
        assert!(manager.latest_receipt(&context).await?.is_none());
        Ok(())
    }
}
