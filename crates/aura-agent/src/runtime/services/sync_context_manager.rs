//! Synchronous context management for testing
//!
//! This module provides a synchronous alternative to ContextManager
//! specifically for use in tests to avoid async runtime issues.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::handlers::AuraContext;
use aura_core::{AuraError, AuraResult, DeviceId};

/// Synchronous context manager for testing
///
/// This implementation uses std::sync::RwLock instead of tokio::sync::RwLock
/// to avoid requiring an async runtime.
#[derive(Clone)]
pub struct SyncContextManager {
    contexts: Arc<RwLock<HashMap<DeviceId, AuraContext>>>,
}

impl Default for SyncContextManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncContextManager {
    /// Create a new synchronous context manager
    pub fn new() -> Self {
        Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with pre-initialized context for testing
    pub fn new_with_context(device_id: DeviceId, context: AuraContext) -> Self {
        let mut contexts = HashMap::new();
        contexts.insert(device_id, context);
        Self {
            contexts: Arc::new(RwLock::new(contexts)),
        }
    }

    /// Get a snapshot of a device's context synchronously
    pub fn get_snapshot(&self, device_id: DeviceId) -> AuraResult<AuraContext> {
        let contexts = self
            .contexts
            .read()
            .map_err(|e| AuraError::internal(format!("Lock poisoned: {}", e)))?;
        contexts
            .get(&device_id)
            .cloned()
            .ok_or_else(|| AuraError::not_found(format!("Context for device {}", device_id)))
    }

    /// Update a device's context synchronously
    pub fn update(&self, device_id: DeviceId, context: AuraContext) -> AuraResult<()> {
        let mut contexts = self
            .contexts
            .write()
            .map_err(|e| AuraError::internal(format!("Lock poisoned: {}", e)))?;
        contexts.insert(device_id, context);
        Ok(())
    }

    /// Initialize a context for a device if it doesn't exist
    pub fn initialize(&self, device_id: DeviceId) -> AuraResult<AuraContext> {
        let mut contexts = self
            .contexts
            .write()
            .map_err(|e| AuraError::internal(format!("Lock poisoned: {}", e)))?;
        let context = contexts
            .entry(device_id)
            .or_insert_with(|| AuraContext::for_testing(device_id));
        Ok(context.clone())
    }
}

// Implement the async ContextManager trait methods synchronously
impl SyncContextManager {
    /// Async-compatible wrapper for get_snapshot
    pub async fn get_snapshot_async(&self, device_id: DeviceId) -> AuraResult<AuraContext> {
        self.get_snapshot(device_id)
    }

    /// Async-compatible wrapper for update
    pub async fn update_async(&self, device_id: DeviceId, context: AuraContext) -> AuraResult<()> {
        self.update(device_id, context)
    }

    /// Async-compatible wrapper for initialize
    pub async fn initialize_async(&self, device_id: DeviceId) -> AuraResult<AuraContext> {
        self.initialize(device_id)
    }
}
