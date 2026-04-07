//! Configuration records and callback subscription tokens for `AppCore`.

use serde::{Deserialize, Serialize};

/// Configuration for creating an AppCore instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AppConfig {
    /// Base directory for durable app data.
    pub data_dir: String,
    /// Enables extra debug behavior in supported frontends.
    pub debug: bool,
    /// Optional path override for the journal backing store.
    pub journal_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            debug: false,
            journal_path: None,
        }
    }
}

/// Unique identifier for a subscription (callbacks feature only).
#[cfg(feature = "callbacks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SubscriptionId {
    pub(crate) id: u64,
}
