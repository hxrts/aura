//! Android preset builder for Android platform applications.
//!
//! Provides sensible defaults for Android applications:
//! - Android Keystore for crypto operations (requires `android` feature)
//! - App-private storage with encryption
//! - System time and SecureRandom sources
//! - Logcat-based console output
//!
//! # Platform Requirements
//!
//! This preset requires the `android` feature flag and platform-specific dependencies:
//! - `jni` for Java Native Interface access
//! - Android API level 23+ for Keystore support
//! - Android API level 28+ for StrongBox (hardware-backed) support
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_agent::AgentBuilder;
//!
//! let agent = AgentBuilder::android()
//!     .application_id("com.example.aura")
//!     .use_strongbox(true)
//!     .build()
//!     .await?;
//! ```

use std::path::PathBuf;

use aura_core::effects::ExecutionMode;
#[cfg(feature = "android")]
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

use crate::builder::BuildError;
use crate::core::AgentConfig;
use crate::{AgentResult, AuraAgent};

/// Android-specific builder with sensible defaults for Android applications.
///
/// # Required Configuration
///
/// - `application_id`: The Android application ID (package name)
///
/// # Optional Configuration
///
/// - `use_strongbox`: Use hardware-backed StrongBox if available
/// - `authority`: Custom authority ID (defaults to derived from application ID)
/// - `context`: Custom context ID (defaults to derived from authority)
///
/// # Platform Effects
///
/// | Effect | Android Implementation |
/// |--------|----------------------|
/// | `CryptoEffects` | Android Keystore (+ StrongBox if enabled) |
/// | `StorageEffects` | App-private encrypted storage |
/// | `PhysicalTimeEffects` | `System.currentTimeMillis()` |
/// | `RandomEffects` | `SecureRandom` |
/// | `ConsoleEffects` | `android.util.Log` |
/// | `TransportEffects` | `OkHttp` / `HttpURLConnection` |
pub struct AndroidPresetBuilder {
    application_id: Option<String>,
    use_strongbox: bool,
    require_user_authentication: bool,
    authentication_validity_seconds: Option<u32>,
    authority_id: Option<AuthorityId>,
    context_id: Option<ContextId>,
    execution_mode: ExecutionMode,
    config: AgentConfig,
}

impl AndroidPresetBuilder {
    /// Create a new Android preset builder.
    pub fn new() -> Self {
        Self {
            application_id: None,
            use_strongbox: false,
            require_user_authentication: false,
            authentication_validity_seconds: None,
            authority_id: None,
            context_id: None,
            execution_mode: ExecutionMode::Production,
            config: AgentConfig::default(),
        }
    }

    /// Set the Android application ID (package name).
    ///
    /// This is required and used for deriving storage paths and key aliases.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// .application_id("com.example.aura")
    /// ```
    pub fn application_id(mut self, id: impl Into<String>) -> Self {
        self.application_id = Some(id.into());
        self
    }

    /// Enable StrongBox hardware-backed key storage.
    ///
    /// StrongBox provides hardware-level key protection on supported devices
    /// (Android 9+ with dedicated security chip).
    ///
    /// If enabled but not available, falls back to TEE-backed Keystore.
    pub fn use_strongbox(mut self, enabled: bool) -> Self {
        self.use_strongbox = enabled;
        self
    }

    /// Require user authentication for key operations.
    ///
    /// When enabled, cryptographic operations require the user to authenticate
    /// via biometrics or device credentials.
    ///
    /// # Arguments
    ///
    /// * `validity_seconds` - How long the authentication is valid (None = every operation)
    pub fn require_user_authentication(mut self, validity_seconds: Option<u32>) -> Self {
        self.require_user_authentication = true;
        self.authentication_validity_seconds = validity_seconds;
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
    /// - The `android` feature is not enabled
    /// - Application ID is not configured
    /// - Keystore initialization fails
    /// - Platform APIs are unavailable
    ///
    /// # Platform Note
    ///
    /// This method requires the `android` feature flag. Without it, this will
    /// return an error indicating that Android support is not compiled in.
    pub async fn build(self) -> AgentResult<AuraAgent> {
        #[cfg(not(feature = "android"))]
        {
            Err(BuildError::EffectInit {
                effect: "android",
                message: "Android preset requires the 'android' feature flag. \
                         Compile with --features android or use a different preset."
                    .to_string(),
            }
            .into())
        }

        #[cfg(feature = "android")]
        {
            // Validate required configuration
            let application_id = self.application_id.ok_or_else(|| BuildError::MissingRequired(
                "application_id is required for Android preset. Call .application_id(\"com.example\") before building."
            ))?;

            // Get or generate authority ID
            let authority_id = self.authority_id.unwrap_or_else(|| {
                let id_str = format!("android:{}", application_id);
                AuthorityId::new_from_entropy(hash::hash(id_str.as_bytes()))
            });

            // Derive context ID if not set
            let context_id = self.context_id.unwrap_or_else(|| {
                let context_entropy = hash::hash(&authority_id.to_bytes());
                ContextId::new_from_entropy(context_entropy)
            });

            // TODO: Wire up Android-specific handlers when android feature is implemented
            // - KeystoreCryptoHandler with StrongBox support
            // - EncryptedSharedPreferencesStorageHandler
            // - AndroidTimeHandler
            // - SecureRandomHandler
            // - LogcatConsoleHandler
            // - OkHttpTransportHandler

            let _ = (
                application_id,
                authority_id,
                context_id,
                self.use_strongbox,
                self.require_user_authentication,
                self.authentication_validity_seconds,
                self.execution_mode,
                self.config,
            );

            Err(BuildError::EffectInit {
                effect: "android",
                message: "Android handlers not yet implemented. This is a placeholder for future development.".to_string(),
            }.into())
        }
    }
}

impl Default for AndroidPresetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the default Android data directory.
#[allow(dead_code)]
fn default_android_data_dir() -> PathBuf {
    // On Android, this would be obtained from:
    // context.getFilesDir() or context.getNoBackupFilesDir()
    PathBuf::from("/data/data/com.example.aura/files")
}
