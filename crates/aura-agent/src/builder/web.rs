//! Web/WASM preset builder for browser applications.
//!
//! Provides sensible defaults for web applications running in browsers:
//! - Web Crypto API for cryptographic operations (requires `web` feature)
//! - IndexedDB for persistent storage
//! - Performance.now() for time
//! - Web Crypto getRandomValues for randomness
//! - Console API for logging
//!
//! # Platform Requirements
//!
//! This preset requires the `web` feature flag and platform-specific dependencies:
//! - `wasm-bindgen` for JavaScript interop
//! - `web-sys` for Web API bindings
//! - `js-sys` for JavaScript runtime bindings
//! - Modern browser with Web Crypto API support
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_agent::AgentBuilder;
//!
//! let agent = AgentBuilder::web()
//!     .storage_prefix("aura_")
//!     .build()
//!     .await?;
//! ```

use aura_core::effects::ExecutionMode;
#[cfg(feature = "web")]
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

use crate::builder::BuildError;
use crate::core::AgentConfig;
use crate::{AgentBuilder, AgentResult, AuraAgent};

/// Web/WASM-specific builder with sensible defaults for browser applications.
///
/// # Optional Configuration
///
/// - `storage_prefix`: Prefix for IndexedDB database names
/// - `use_session_storage`: Use SessionStorage instead of IndexedDB for ephemeral data
/// - `authority`: Custom authority ID (defaults to derived from origin)
/// - `context`: Custom context ID (defaults to derived from authority)
///
/// # Platform Effects
///
/// | Effect | Web Implementation |
/// |--------|-------------------|
/// | `CryptoEffects` | Web Crypto API (SubtleCrypto) |
/// | `StorageEffects` | IndexedDB |
/// | `PhysicalTimeEffects` | `Date.now()` / `performance.now()` |
/// | `RandomEffects` | `crypto.getRandomValues()` |
/// | `ConsoleEffects` | `console.log/warn/error` |
/// | `TransportEffects` | `fetch` API / WebSocket |
///
/// # Security Considerations
///
/// - Keys are stored in IndexedDB which is accessible to JavaScript
/// - Consider using non-extractable CryptoKeys where possible
/// - Cross-origin isolation affects some APIs
/// - Service Workers may be needed for background operations
pub struct WebPresetBuilder {
    storage_prefix: String,
    use_session_storage: bool,
    enable_persistence: bool,
    authority_id: Option<AuthorityId>,
    context_id: Option<ContextId>,
    execution_mode: ExecutionMode,
    config: AgentConfig,
}

impl WebPresetBuilder {
    /// Create a new Web/WASM preset builder.
    pub fn new() -> Self {
        Self {
            storage_prefix: "aura_".to_string(),
            use_session_storage: false,
            enable_persistence: true,
            authority_id: None,
            context_id: None,
            execution_mode: ExecutionMode::Production,
            config: AgentConfig::default(),
        }
    }

    /// Set the storage prefix for IndexedDB database names.
    ///
    /// This prefix is applied to all database and object store names
    /// to avoid conflicts with other applications.
    ///
    /// Default: `"aura_"`
    pub fn storage_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.storage_prefix = prefix.into();
        self
    }

    /// Use SessionStorage for ephemeral data instead of IndexedDB.
    ///
    /// When enabled, data is only persisted for the browser session.
    /// This is useful for privacy-sensitive applications.
    pub fn use_session_storage(mut self, enabled: bool) -> Self {
        self.use_session_storage = enabled;
        self
    }

    /// Enable or disable persistent storage.
    ///
    /// When disabled, all data is kept in memory only.
    /// Default: enabled
    pub fn enable_persistence(mut self, enabled: bool) -> Self {
        self.enable_persistence = enabled;
        self
    }

    /// Set the authority ID for this agent.
    pub fn authority(mut self, id: AuthorityId) -> Self {
        self.authority_id = Some(id);
        self
    }

    /// Set the default context ID for this agent.
    pub fn context(mut self, id: ContextId) -> Self {
        self.context_id = Some(id);
        self
    }

    /// Use testing execution mode.
    pub fn testing_mode(mut self) -> Self {
        self.execution_mode = ExecutionMode::Testing;
        self
    }

    /// Use simulation execution mode with a specific seed.
    pub fn simulation_mode(mut self, seed: u64) -> Self {
        self.execution_mode = ExecutionMode::Simulation { seed };
        self
    }

    /// Set the agent configuration.
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the agent asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `web` feature is not enabled
    /// - Web Crypto API is not available
    /// - IndexedDB initialization fails
    /// - Running outside a browser context
    ///
    /// # Platform Note
    ///
    /// This method requires the `web` feature flag. Without it, this will
    /// return an error indicating that Web support is not compiled in.
    pub async fn build(self) -> AgentResult<AuraAgent> {
        #[cfg(not(feature = "web"))]
        {
            return Err(BuildError::EffectInit {
                effect: "web",
                message: "Web preset requires the 'web' feature flag. \
                         Compile with --features web or use a different preset."
                    .to_string(),
            }
            .into());
        }

        #[cfg(feature = "web")]
        {
            // Get or generate authority ID
            // In a real implementation, this might derive from window.location.origin
            let authority_id = self.authority_id.unwrap_or_else(|| {
                let id_str = format!("web:{}", self.storage_prefix);
                AuthorityId::new_from_entropy(hash::hash(id_str.as_bytes()))
            });

            // Derive context ID if not set
            let context_id = self.context_id.unwrap_or_else(|| {
                let context_entropy = hash::hash(&authority_id.to_bytes());
                ContextId::new_from_entropy(context_entropy)
            });

            // TODO: Wire up Web-specific handlers when web feature is implemented
            // - WebCryptoCryptoHandler using SubtleCrypto
            // - IndexedDBStorageHandler
            // - PerformanceTimeHandler
            // - WebCryptoRandomHandler
            // - ConsoleLogHandler
            // - FetchTransportHandler / WebSocketTransportHandler

            let _ = (
                authority_id,
                context_id,
                self.storage_prefix,
                self.use_session_storage,
                self.enable_persistence,
                self.execution_mode,
                self.config,
            );

            Err(BuildError::EffectInit {
                effect: "web",
                message: "Web handlers not yet implemented. This is a placeholder for future development.".to_string(),
            }.into())
        }
    }
}

impl Default for WebPresetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// Extend AgentBuilder with Web preset entry point
impl AgentBuilder {
    /// Create a Web/WASM preset builder for browser applications.
    ///
    /// This provides sensible defaults for web apps:
    /// - Web Crypto API for cryptographic operations
    /// - IndexedDB for persistent storage
    /// - Performance API for timing
    /// - Console API for logging
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let agent = AgentBuilder::web()
    ///     .storage_prefix("myapp_")
    ///     .build()
    ///     .await?;
    /// ```
    ///
    /// # Feature Flag
    ///
    /// Requires the `web` feature flag for full functionality.
    /// Without it, `build()` will return an error.
    pub fn web() -> WebPresetBuilder {
        WebPresetBuilder::new()
    }
}
