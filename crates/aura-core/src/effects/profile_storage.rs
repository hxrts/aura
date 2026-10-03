//! Process-lifetime infrastructure ownership of a storage profile.
//! A lease does not authorize product operations or replace secure storage.
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ProfileStorageError {
    #[error("storage profile is already owned")]
    Busy,
    #[error("legacy plaintext secure storage remains in browser namespace {namespace}")]
    LegacyBrowserSecureStorage { namespace: String },
    #[error("profile ownership deadline exhausted: {operation}")]
    Timeout { operation: &'static str },
    /// Explicit typed foreign diagnostic conversion at the browser boundary.
    #[error("browser profile {operation} failed ({name}): {message}")]
    Browser {
        operation: &'static str,
        name: String,
        message: String,
    },
    #[error("exclusive profile ownership is unsupported on this backend")]
    Unsupported,
    #[error("invalid storage profile: {0}")]
    Invalid(String),
    #[error("profile ownership operation failed: {source}")]
    Io {
        #[source]
        source: Arc<std::io::Error>,
    },
}

/// Adapter-produced resource guard. No Clone or serialization contract.
/// Release is synchronous on drop, including cancellation. A process crash
/// releases the OS resource; durable recovery remains the runtime's job.
#[cfg(not(target_arch = "wasm32"))]
pub trait ProfileStorageLease: Send + Sync + std::fmt::Debug {
    /// Canonical physical profile selected by the adapter, for diagnostics.
    fn profile_identity(&self) -> &str;
}

#[cfg(target_arch = "wasm32")]
pub trait ProfileStorageLease: std::fmt::Debug {
    fn profile_identity(&self) -> &str;
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ProfileStorageEffects: Send + Sync {
    /// Nonblocking acquisition before constructing any profile writer.
    async fn acquire_profile_lease(
        &self,
    ) -> Result<Box<dyn ProfileStorageLease>, ProfileStorageError>;
}
