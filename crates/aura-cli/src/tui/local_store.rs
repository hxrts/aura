//! # LocalStore TUI Integration
//!
//! Provides helpers for integrating the LocalStore with the TUI lifecycle.
//! Handles loading preferences on startup and saving on shutdown.

use aura_core::effects::{CryptoEffects, StorageEffects};
use aura_core::AuthorityId;
use aura_store::local::{ContactCache, LocalData, LocalStore, LocalStoreConfig, LocalStoreError};
use std::path::PathBuf;
use std::sync::Arc;

/// Manager for the TUI's local store
///
/// Wraps the LocalStore with TUI-specific convenience methods
/// and manages the store lifecycle with crypto and storage effects.
pub struct TuiLocalStore<C: CryptoEffects, S: StorageEffects> {
    /// The underlying local store
    store: LocalStore,
    /// Crypto effects handler
    crypto: Arc<C>,
    /// Storage effects handler
    storage: Arc<S>,
    /// Whether there are unsaved changes
    dirty: bool,
}

impl<C: CryptoEffects, S: StorageEffects> TuiLocalStore<C, S> {
    /// Open or create a local store for the TUI
    ///
    /// # Arguments
    ///
    /// * `authority_id` - The authority this store belongs to
    /// * `key_material` - Secret material for encryption key derivation
    /// * `data_dir` - Optional custom data directory
    /// * `crypto` - Crypto effects handler
    /// * `storage` - Storage effects handler
    pub async fn open(
        _authority_id: AuthorityId,
        key_material: &[u8],
        data_dir: Option<PathBuf>,
        crypto: Arc<C>,
        storage: Arc<S>,
    ) -> Result<Self, LocalStoreError> {
        let path = data_dir.unwrap_or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".aura").join("local.store"))
                .unwrap_or_else(|| PathBuf::from(".aura/local.store"))
        });

        let config = LocalStoreConfig::new(path);
        let store =
            LocalStore::load(config, key_material, crypto.as_ref(), storage.as_ref()).await?;

        Ok(Self {
            store,
            crypto,
            storage,
            dirty: false,
        })
    }

    /// Get the current local data
    pub fn data(&self) -> &LocalData {
        self.store.data()
    }

    /// Get mutable access to local data (marks as dirty)
    pub fn data_mut(&mut self) -> &mut LocalData {
        self.dirty = true;
        self.store.data_mut()
    }

    /// Check if there are unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Save changes to disk
    pub async fn save(&mut self) -> Result<(), LocalStoreError> {
        if self.dirty {
            self.store
                .save(self.crypto.as_ref(), self.storage.as_ref())
                .await?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Force save even if not dirty
    pub async fn force_save(&mut self) -> Result<(), LocalStoreError> {
        self.store
            .save(self.crypto.as_ref(), self.storage.as_ref())
            .await?;
        self.dirty = false;
        Ok(())
    }

    /// Reload data from disk (discards unsaved changes)
    pub async fn reload(&mut self, key_material: &[u8]) -> Result<(), LocalStoreError> {
        let config = LocalStoreConfig::new(self.store.path());
        self.store = LocalStore::load(
            config,
            key_material,
            self.crypto.as_ref(),
            self.storage.as_ref(),
        )
        .await?;
        self.dirty = false;
        Ok(())
    }

    // ─── Convenience Methods ─────────────────────────────────────────────────

    /// Get theme preference
    pub fn theme(&self) -> aura_store::local::ThemePreference {
        self.data().theme
    }

    /// Set theme preference
    pub fn set_theme(&mut self, theme: aura_store::local::ThemePreference) {
        self.data_mut().theme = theme;
        self.dirty = true;
    }

    /// Add or update a contact
    pub fn update_contact(&mut self, contact: ContactCache) {
        self.data_mut().cache_contact(contact);
        self.dirty = true;
    }

    /// Get a contact
    pub fn get_contact(&self, authority_id: &AuthorityId) -> Option<&ContactCache> {
        self.data().get_contact(authority_id)
    }

    /// Record a conversation visit (adds to recent)
    pub fn visit_conversation(&mut self, conversation_id: impl Into<String>) {
        self.data_mut()
            .add_recent_conversation(conversation_id.into());
        self.dirty = true;
    }

    /// Get recent conversations
    pub fn recent_conversations(&self) -> &[String] {
        &self.data().recent_conversations
    }

    /// Set last active screen
    pub fn set_last_screen(&mut self, screen: Option<String>) {
        self.data_mut().last_screen = screen;
        self.dirty = true;
    }

    /// Get last active screen
    pub fn last_screen(&self) -> Option<&str> {
        self.data().last_screen.as_deref()
    }

    /// Set a custom setting
    pub fn set_setting(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data_mut().set_setting(key, value);
        self.dirty = true;
    }

    /// Get a custom setting
    pub fn get_setting(&self, key: &str) -> Option<&String> {
        self.data().get_setting(key)
    }
}

/// Helper to derive key material from an authority
///
/// In a real implementation, this would derive from the authority's
/// secret key. For now, we use a placeholder that hashes the authority ID.
pub fn derive_key_material(authority_id: &AuthorityId) -> Vec<u8> {
    // In production, this should derive from actual authority secret material
    // For now, use a deterministic derivation from the authority ID
    use aura_core::hash::hash;
    let mut data = Vec::new();
    data.extend_from_slice(b"aura-local-store-key-v1");
    data.extend_from_slice(&authority_id.to_bytes());
    hash(&data).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::stateful_effects::MemoryStorageHandler;
    use aura_testkit::MockCryptoHandler;
    use tempfile::TempDir;

    fn test_authority() -> AuthorityId {
        AuthorityId::new()
    }

    #[tokio::test]
    async fn test_tui_local_store_open() {
        let temp_dir = TempDir::new().unwrap();
        let authority_id = test_authority();
        let key_material = derive_key_material(&authority_id);
        let crypto = Arc::new(MockCryptoHandler::new());
        let storage = Arc::new(MemoryStorageHandler::new());

        let store = TuiLocalStore::open(
            authority_id,
            &key_material,
            Some(temp_dir.path().join("test.store")),
            crypto,
            storage,
        )
        .await
        .unwrap();

        assert!(!store.is_dirty());
    }

    #[tokio::test]
    async fn test_dirty_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let authority_id = test_authority();
        let key_material = derive_key_material(&authority_id);
        let crypto = Arc::new(MockCryptoHandler::new());
        let storage = Arc::new(MemoryStorageHandler::new());

        let mut store = TuiLocalStore::open(
            authority_id,
            &key_material,
            Some(temp_dir.path().join("test.store")),
            crypto,
            storage,
        )
        .await
        .unwrap();

        assert!(!store.is_dirty());

        store.set_theme(aura_store::local::ThemePreference::Light);
        assert!(store.is_dirty());

        store.save().await.unwrap();
        assert!(!store.is_dirty());
    }

    #[tokio::test]
    async fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.store");
        let authority_id = test_authority();
        let key_material = derive_key_material(&authority_id);
        let crypto = Arc::new(MockCryptoHandler::new());
        let storage = Arc::new(MemoryStorageHandler::new());

        // Create and save
        {
            let mut store = TuiLocalStore::open(
                authority_id.clone(),
                &key_material,
                Some(path.clone()),
                crypto.clone(),
                storage.clone(),
            )
            .await
            .unwrap();
            store.visit_conversation("conversation-1");
            store.save().await.unwrap();
        }

        // Reopen and verify
        {
            let store =
                TuiLocalStore::open(authority_id, &key_material, Some(path), crypto, storage)
                    .await
                    .unwrap();
            assert_eq!(store.recent_conversations(), &["conversation-1"]);
        }
    }

    #[tokio::test]
    async fn test_contact_management() {
        let temp_dir = TempDir::new().unwrap();
        let authority_id = test_authority();
        let contact_authority = AuthorityId::new();
        let key_material = derive_key_material(&authority_id);
        let crypto = Arc::new(MockCryptoHandler::new());
        let storage = Arc::new(MemoryStorageHandler::new());

        let mut store = TuiLocalStore::open(
            authority_id,
            &key_material,
            Some(temp_dir.path().join("test.store")),
            crypto,
            storage,
        )
        .await
        .unwrap();

        let mut contact = ContactCache::new(contact_authority.clone());
        contact.display_name = Some("Alice".to_string());
        store.update_contact(contact);
        assert!(store.is_dirty());

        let cached = store.get_contact(&contact_authority).unwrap();
        assert_eq!(cached.display_name.as_deref(), Some("Alice"));
    }
}
