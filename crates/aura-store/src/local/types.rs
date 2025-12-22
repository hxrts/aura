//! Local store data types
//!
//! Types for local storage of CLI/TUI preferences and cached data.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use aura_core::AuthorityId;

/// User theme preference for the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemePreference {
    /// Light theme
    Light,
    /// Dark theme
    #[default]
    Dark,
    /// Follow system preference
    System,
}

/// Cached contact information for offline display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactCache {
    /// Authority ID of the contact
    pub authority_id: AuthorityId,
    /// Nickname assigned by the user
    pub nickname: Option<String>,
    /// Display name from the contact
    pub display_name: Option<String>,
    /// Last seen timestamp (unix millis)
    pub last_seen: Option<u64>,
    /// Cached avatar hash for display
    pub avatar_hash: Option<String>,
}

impl ContactCache {
    /// Create a new contact cache entry
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            nickname: None,
            display_name: None,
            last_seen: None,
            avatar_hash: None,
        }
    }

    /// Get the best display name for this contact
    pub fn display(&self) -> String {
        self.nickname
            .clone()
            .or_else(|| self.display_name.clone())
            .unwrap_or_else(|| {
                let id = self.authority_id.to_string();
                format!("{}...", &id[..8.min(id.len())])
            })
    }
}

/// Local data stored encrypted on disk
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalData {
    /// User's theme preference
    pub theme: ThemePreference,

    /// Cached contacts for offline display
    pub contacts: HashMap<String, ContactCache>,

    /// Recent conversation IDs for quick access
    pub recent_conversations: Vec<String>,

    /// Last active screen in the TUI
    pub last_screen: Option<String>,

    /// Custom user settings
    pub settings: HashMap<String, String>,
}

impl LocalData {
    /// Create empty local data
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a contact in the cache
    pub fn cache_contact(&mut self, contact: ContactCache) {
        let key = contact.authority_id.to_string();
        self.contacts.insert(key, contact);
    }

    /// Get a cached contact by authority ID
    pub fn get_contact(&self, authority_id: &AuthorityId) -> Option<&ContactCache> {
        self.contacts.get(&authority_id.to_string())
    }

    /// Remove a contact from the cache
    pub fn remove_contact(&mut self, authority_id: &AuthorityId) -> Option<ContactCache> {
        self.contacts.remove(&authority_id.to_string())
    }

    /// Add a conversation to recent list
    pub fn add_recent_conversation(&mut self, conversation_id: String) {
        // Remove if already exists
        self.recent_conversations
            .retain(|id| id != &conversation_id);
        // Add to front
        self.recent_conversations.insert(0, conversation_id);
        // Keep only last 20
        self.recent_conversations.truncate(20);
    }

    /// Set a custom setting
    pub fn set_setting(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.settings.insert(key.into(), value.into());
    }

    /// Get a custom setting
    pub fn get_setting(&self, key: &str) -> Option<&String> {
        self.settings.get(key)
    }
}

/// Configuration for the local store
#[derive(Debug, Clone)]
pub struct LocalStoreConfig {
    /// Path to the store file
    pub path: std::path::PathBuf,
}

impl LocalStoreConfig {
    /// Create a new config with the given path
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Get the storage key derived from the path
    ///
    /// This converts the file path to a key suitable for StorageEffects.
    /// The path is converted to a canonical string representation.
    pub fn storage_key(&self) -> String {
        format!("local-store:{}", self.path.display())
    }
}
