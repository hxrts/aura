//! Context management service
//!
//! Provides isolated management of device contexts with atomic updates
//! and snapshot capabilities for lock-free effect execution.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use aura_core::{AuraError, AuraResult, DeviceId};

use crate::handlers::context_immutable::AuraContext;

/// Manages device contexts in isolation from effect execution
#[derive(Clone)]
pub struct ContextManager {
    /// Device contexts - only accessed through brief lock acquisitions
    contexts: Arc<RwLock<HashMap<DeviceId, AuraContext>>>,
}

impl ContextManager {
    /// Create a new context manager
    pub fn new() -> Self {
        Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get an immutable snapshot of a device's context
    ///
    /// This method acquires a read lock briefly to clone the context,
    /// then releases it immediately. The returned context is a complete
    /// copy that can be used without holding any locks.
    pub async fn get_snapshot(&self, device_id: DeviceId) -> AuraResult<AuraContext> {
        let contexts = self.contexts.read().await;
        contexts
            .get(&device_id)
            .cloned()
            .ok_or_else(|| AuraError::not_found(format!("Context for device {}", device_id)))
    }

    /// Update a device's context atomically
    ///
    /// This method acquires a write lock briefly to update the context,
    /// then releases it immediately.
    pub async fn update(&self, device_id: DeviceId, context: AuraContext) -> AuraResult<()> {
        let mut contexts = self.contexts.write().await;
        contexts.insert(device_id, context);
        Ok(())
    }

    /// Initialize a context for a device if it doesn't exist
    pub async fn initialize(&self, device_id: DeviceId) -> AuraResult<AuraContext> {
        let mut contexts = self.contexts.write().await;
        let context = contexts
            .entry(device_id)
            .or_insert_with(|| AuraContext::for_testing(device_id));
        Ok(context.clone())
    }

    /// Remove a device's context
    pub async fn remove(&self, device_id: DeviceId) -> AuraResult<Option<AuraContext>> {
        let mut contexts = self.contexts.write().await;
        Ok(contexts.remove(&device_id))
    }

    /// Check if a context exists for a device
    pub async fn exists(&self, device_id: DeviceId) -> bool {
        let contexts = self.contexts.read().await;
        contexts.contains_key(&device_id)
    }

    /// Get all device IDs with contexts
    pub async fn device_ids(&self) -> Vec<DeviceId> {
        let contexts = self.contexts.read().await;
        contexts.keys().copied().collect()
    }

    /// Apply a function to update a context atomically
    ///
    /// This ensures the context update is atomic even if it requires
    /// reading the current state first.
    pub async fn update_with<F>(&self, device_id: DeviceId, f: F) -> AuraResult<AuraContext>
    where
        F: FnOnce(&mut AuraContext),
    {
        let mut contexts = self.contexts.write().await;
        let context = contexts
            .get_mut(&device_id)
            .ok_or_else(|| AuraError::not_found(format!("Context for device {}", device_id)))?;

        f(context);
        Ok(context.clone())
    }

    /// Clear all contexts (useful for testing)
    pub async fn clear(&self) {
        let mut contexts = self.contexts.write().await;
        contexts.clear();
    }

    /// Get the number of managed contexts
    pub async fn len(&self) -> usize {
        let contexts = self.contexts.read().await;
        contexts.len()
    }

    /// Check if the manager is empty
    pub async fn is_empty(&self) -> bool {
        let contexts = self.contexts.read().await;
        contexts.is_empty()
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::{aura_test, TestFixture};

    #[aura_test]
    async fn test_context_snapshot() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ContextManager::new();
        let device_id = fixture.device_id();

        // Initialize a context
        let context = manager.initialize(device_id).await?;
        assert_eq!(context.device_id, device_id);

        // Get snapshot - should not hold lock
        let snapshot1 = manager.get_snapshot(device_id).await?;
        let snapshot2 = manager.get_snapshot(device_id).await?;

        // Snapshots should be equal but independent
        assert_eq!(snapshot1.device_id, snapshot2.device_id);
        assert_eq!(snapshot1.epoch, snapshot2.epoch);

        Ok(())
    }

    #[aura_test]
    async fn test_atomic_update() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ContextManager::new();
        let device_id = fixture.device_id();

        // Initialize context
        manager.initialize(device_id).await?;

        // Update atomically
        let updated = manager
            .update_with(device_id, |ctx| {
                ctx.epoch = 42;
            })
            .await?;

        assert_eq!(updated.epoch, 42);

        // Verify update persisted
        let snapshot = manager.get_snapshot(device_id).await?;
        assert_eq!(snapshot.epoch, 42);

        Ok(())
    }

    #[aura_test]
    async fn test_context_not_found() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ContextManager::new();
        let device_id = fixture.device_id();

        let result = manager.get_snapshot(device_id).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Context for device"));

        Ok(())
    }

    #[aura_test]
    async fn test_concurrent_access() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = ContextManager::new();
        let device_id = fixture.device_id();

        // Initialize context
        manager.initialize(device_id).await?;

        // Spawn concurrent readers
        let mut handles = vec![];
        for _ in 0..10 {
            let mgr = manager.clone();
            let handle = tokio::spawn(async move { mgr.get_snapshot(device_id).await.unwrap() });
            handles.push(handle);
        }

        // All reads should succeed without blocking
        for handle in handles {
            let context = handle.await.unwrap();
            assert_eq!(context.device_id, device_id);
        }

        Ok(())
    }
}
