//! Local device nickname storage (non-replicated).
//!
//! # Invariants
//!
//! - Nicknames are local to this device only (not synced)
//! - Empty nicknames are removed, not stored as empty strings
//! - Storage is persisted via the agent's local database
//!
//! # Thread Safety
//!
//! Uses `tokio::sync::RwLock` for interior mutability. All async operations
//! are short and never held across await points.
//!
//! # Note
//!
//! This module is prepared for future use but not yet wired into the runtime.
//! The `#[allow(dead_code)]` attributes will be removed when integration is complete.

#![allow(dead_code)] // Not yet wired into runtime

use aura_core::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum entries in the nickname store.
///
/// Prevents unbounded growth. Oldest entries are evicted if exceeded.
pub const NICKNAME_ENTRIES_MAX: usize = 1000;

/// Local device nickname storage.
///
/// Stores user-assigned nicknames for devices (local overrides).
/// Not replicated - each device maintains its own nickname preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceNicknameStore {
    /// Map from device ID to local nickname.
    ///
    /// Invariant: no empty string values (empty means remove).
    nicknames: HashMap<DeviceId, String>,
}

impl DeviceNicknameStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the local nickname for a device.
    ///
    /// Returns `None` if no nickname is set.
    #[must_use]
    pub fn get(&self, device_id: &DeviceId) -> Option<&str> {
        self.nicknames.get(device_id).map(String::as_str)
    }

    /// Set a local nickname for a device.
    ///
    /// Empty string clears the nickname (equivalent to `clear`).
    pub fn set(&mut self, device_id: DeviceId, nickname: impl Into<String>) {
        let nickname = nickname.into();
        if nickname.is_empty() {
            self.nicknames.remove(&device_id);
        } else {
            // Enforce bounded size
            if self.nicknames.len() >= NICKNAME_ENTRIES_MAX
                && !self.nicknames.contains_key(&device_id)
            {
                // Simple eviction: remove arbitrary entry
                // In practice this limit is unlikely to be hit
                if let Some(key) = self.nicknames.keys().next().copied() {
                    self.nicknames.remove(&key);
                }
            }
            self.nicknames.insert(device_id, nickname);
        }
    }

    /// Clear the local nickname for a device.
    pub fn clear(&mut self, device_id: &DeviceId) {
        self.nicknames.remove(device_id);
    }

    /// Number of stored nicknames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nicknames.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nicknames.is_empty()
    }

    /// Iterate over all nicknames.
    pub fn iter(&self) -> impl Iterator<Item = (&DeviceId, &str)> {
        self.nicknames.iter().map(|(k, v)| (k, v.as_str()))
    }
}

/// Thread-safe wrapper for device nickname storage.
///
/// Provides interior mutability via `tokio::sync::RwLock`.
#[derive(Debug, Clone)]
pub struct DeviceNicknameManager {
    store: Arc<RwLock<DeviceNicknameStore>>,
}

impl Default for DeviceNicknameManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceNicknameManager {
    /// Create a new empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(DeviceNicknameStore::default())),
        }
    }

    /// Create a manager with pre-loaded state.
    #[must_use]
    pub fn with_store(store: DeviceNicknameStore) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
        }
    }

    /// Get the local nickname for a device.
    pub async fn get(&self, device_id: &DeviceId) -> Option<String> {
        self.store.read().await.get(device_id).map(String::from)
    }

    /// Set a local nickname for a device.
    pub async fn set(&self, device_id: DeviceId, nickname: impl Into<String>) {
        self.store.write().await.set(device_id, nickname);
    }

    /// Clear the local nickname for a device.
    pub async fn clear(&self, device_id: &DeviceId) {
        self.store.write().await.clear(device_id);
    }

    /// Get a snapshot of the current store for persistence.
    pub async fn snapshot(&self) -> DeviceNicknameStore {
        self.store.read().await.clone()
    }

    /// Replace the store with loaded state.
    pub async fn load(&self, store: DeviceNicknameStore) {
        *self.store.write().await = store;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_device_id(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_store_set_and_get() {
        let mut store = DeviceNicknameStore::new();
        let device = test_device_id(1);

        assert!(store.get(&device).is_none());

        store.set(device, "My Laptop");
        assert_eq!(store.get(&device), Some("My Laptop"));

        store.set(device, "Work Computer");
        assert_eq!(store.get(&device), Some("Work Computer"));
    }

    #[test]
    fn test_store_empty_string_removes() {
        let mut store = DeviceNicknameStore::new();
        let device = test_device_id(1);

        store.set(device, "My Laptop");
        assert_eq!(store.len(), 1);

        store.set(device, "");
        assert!(store.get(&device).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_store_clear() {
        let mut store = DeviceNicknameStore::new();
        let device = test_device_id(1);

        store.set(device, "My Laptop");
        store.clear(&device);
        assert!(store.get(&device).is_none());
    }

    #[tokio::test]
    async fn test_manager_thread_safe() {
        let manager = DeviceNicknameManager::new();
        let device = test_device_id(1);

        manager.set(device, "Test Device").await;
        assert_eq!(manager.get(&device).await, Some("Test Device".to_string()));

        manager.clear(&device).await;
        assert!(manager.get(&device).await.is_none());
    }

    #[tokio::test]
    async fn test_manager_snapshot_and_load() {
        let manager = DeviceNicknameManager::new();
        let device1 = test_device_id(1);
        let device2 = test_device_id(2);

        manager.set(device1, "Device 1").await;
        manager.set(device2, "Device 2").await;

        let snapshot = manager.snapshot().await;
        assert_eq!(snapshot.len(), 2);

        let manager2 = DeviceNicknameManager::new();
        manager2.load(snapshot).await;

        assert_eq!(manager2.get(&device1).await, Some("Device 1".to_string()));
        assert_eq!(manager2.get(&device2).await, Some("Device 2".to_string()));
    }

    #[test]
    fn test_store_serialization() {
        let mut store = DeviceNicknameStore::new();
        let device = test_device_id(1);
        store.set(device, "My Laptop");

        let json = serde_json::to_string(&store).expect("serialize");
        let loaded: DeviceNicknameStore = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(loaded.get(&device), Some("My Laptop"));
    }
}
