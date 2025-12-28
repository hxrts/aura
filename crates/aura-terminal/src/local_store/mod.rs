//! # Local Storage
//!
//! This module provides local storage for CLI/TUI preferences and cached data.
//!
//! Note: Encryption at rest is handled by the unified storage layer (`EncryptedStorage`)
//! beneath `StorageEffects`; this module intentionally does not implement encryption itself.
//!
//! ## Features
//!
//! - **Contact caching**: Offline display of contact information
//! - **Theme preferences**: User customization settings
//!
//! ## Usage
//!
//! ```ignore
//! use aura_terminal::local_store::{LocalStore, LocalStoreConfig, ThemePreference};
//! use aura_core::effects::StorageEffects;
//!
//! // Create store
//! let config = LocalStoreConfig::new("/path/to/store.dat");
//! let mut store = LocalStore::new(config);
//!
//! // Modify preferences
//! store.data_mut().theme = ThemePreference::Dark;
//!
//! // Save via StorageEffects
//! store.save(&storage_effects).await?;
//! ```

mod errors;
mod store;
mod types;

pub use errors::LocalStoreError;
pub use store::LocalStore;
pub use types::{ContactCache, LocalData, LocalStoreConfig, ThemePreference};
