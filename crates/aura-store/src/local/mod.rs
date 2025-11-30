//! # Encrypted Local Storage
//!
//! This module provides encrypted local storage for CLI/TUI preferences and cached data.
//!
//! ## Features
//!
//! - **Encryption at rest**: All data encrypted with ChaCha20-Poly1305
//! - **Key derivation**: HKDF from authority cryptographic material
//! - **Effect-based randomness**: Nonce generation via `RandomEffects` for deterministic testing
//! - **Contact caching**: Offline display of contact information
//! - **Theme preferences**: User customization settings
//!
//! ## Usage
//!
//! ```ignore
//! use aura_store::local::{LocalStore, LocalStoreConfig, ThemePreference};
//!
//! // Create store with key material from authority
//! let config = LocalStoreConfig::new("/path/to/store.dat");
//! let mut store = LocalStore::new(config, authority_key_material)?;
//!
//! // Modify preferences
//! store.data_mut().theme = ThemePreference::Dark;
//!
//! // Save with RandomEffects for deterministic nonce generation
//! store.save(&random_effects).await?;
//! ```

mod errors;
mod store;
mod types;

pub use errors::LocalStoreError;
pub use store::LocalStore;
pub use types::{ContactCache, LocalData, LocalStoreConfig, ThemePreference};
