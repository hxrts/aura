//! iOS preset builder for Apple platform applications.
//!
//! Provides sensible defaults for iOS applications:
//! - Keychain-based crypto storage (requires `ios` feature)
//! - App container storage with data protection
//! - System time and random sources
//! - os_log-based console output
//!
//! # Platform Requirements
//!
//! This preset requires the `ios` feature flag and platform-specific dependencies:
//! - `security-framework` for Keychain access
//! - iOS 13.0+ for modern cryptographic APIs
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_agent::AgentBuilder;
//!
//! let agent = AgentBuilder::ios()
//!     .app_group("group.com.example.aura")
//!     .keychain_access_group("com.example.aura")
//!     .build()
//!     .await?;
//! ```

use std::path::PathBuf;

use aura_core::effects::ExecutionMode;
#[cfg(feature = "ios")]
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

use crate::builder::BuildError;
use crate::core::AgentConfig;
use crate::{AgentBuilder, AgentResult, AuraAgent};

/// iOS-specific builder with sensible defaults for Apple platform applications.
///
/// # Required Configuration
///
/// - `app_group`: The app group identifier for shared storage between app and extensions
///
/// # Optional Configuration
///
/// - `keychain_access_group`: Keychain access group for shared credentials
/// - `authority`: Custom authority ID (defaults to derived from app group)
/// - `context`: Custom context ID (defaults to derived from authority)
///
/// # Platform Effects
///
/// | Effect | iOS Implementation |
/// |--------|-------------------|
/// | `CryptoEffects` | Keychain + Secure Enclave |
/// | `StorageEffects` | App container with data protection |
/// | `PhysicalTimeEffects` | `Date()` / `CFAbsoluteTimeGetCurrent` |
/// | `RandomEffects` | `SecRandomCopyBytes` |
/// | `ConsoleEffects` | `os_log` |
/// | `TransportEffects` | `URLSession` |
pub struct IosPresetBuilder {
    app_group: Option<String>,
    keychain_access_group: Option<String>,
    data_protection_class: DataProtectionClass,
    authority_id: Option<AuthorityId>,
    context_id: Option<ContextId>,
    execution_mode: ExecutionMode,
    config: AgentConfig,
}

/// iOS Data Protection classes for file encryption
#[derive(Debug, Clone, Copy, Default)]
pub enum DataProtectionClass {
    /// Files are accessible only while device is unlocked
    #[default]
    CompleteProtection,
    /// Files are accessible after first unlock until reboot
    CompleteUnlessOpen,
    /// Files are accessible after first unlock
    UntilFirstUserAuthentication,
    /// No protection (not recommended)
    None,
}

impl IosPresetBuilder {
    /// Create a new iOS preset builder.
    pub fn new() -> Self {
        Self {
            app_group: None,
            keychain_access_group: None,
            data_protection_class: DataProtectionClass::default(),
            authority_id: None,
            context_id: None,
            execution_mode: ExecutionMode::Production,
            config: AgentConfig::default(),
        }
    }

    /// Set the app group identifier for shared storage.
    ///
    /// This is required for sharing data between the main app and extensions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// .app_group("group.com.example.aura")
    /// ```
    pub fn app_group(mut self, group: impl Into<String>) -> Self {
        self.app_group = Some(group.into());
        self
    }

    /// Set the keychain access group for shared credentials.
    ///
    /// If not set, uses the default keychain access group for the app.
    pub fn keychain_access_group(mut self, group: impl Into<String>) -> Self {
        self.keychain_access_group = Some(group.into());
        self
    }

    /// Set the data protection class for stored files.
    ///
    /// Defaults to `CompleteProtection` (most secure).
    pub fn data_protection(mut self, class: DataProtectionClass) -> Self {
        self.data_protection_class = class;
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
    /// - The `ios` feature is not enabled
    /// - App group is not configured
    /// - Keychain access fails
    /// - Platform APIs are unavailable
    ///
    /// # Platform Note
    ///
    /// This method requires the `ios` feature flag. Without it, this will
    /// return an error indicating that iOS support is not compiled in.
    pub async fn build(self) -> AgentResult<AuraAgent> {
        #[cfg(not(feature = "ios"))]
        {
            return Err(BuildError::EffectInit {
                effect: "ios",
                message: "iOS preset requires the 'ios' feature flag. \
                         Compile with --features ios or use a different preset."
                    .to_string(),
            }
            .into());
        }

        #[cfg(feature = "ios")]
        {
            // Validate required configuration
            let app_group = self.app_group.ok_or_else(|| BuildError::MissingRequired(
                "app_group is required for iOS preset. Call .app_group(\"group.com.example\") before building."
            ))?;

            // Get or generate authority ID
            let authority_id = self.authority_id.unwrap_or_else(|| {
                let id_str = format!("ios:{}", app_group);
                AuthorityId::new_from_entropy(hash::hash(id_str.as_bytes()))
            });

            // Derive context ID if not set
            let context_id = self.context_id.unwrap_or_else(|| {
                let context_entropy = hash::hash(&authority_id.to_bytes());
                ContextId::new_from_entropy(context_entropy)
            });

            // TODO: Wire up iOS-specific handlers when ios feature is implemented
            // - KeychainCryptoHandler for Secure Enclave operations
            // - AppContainerStorageHandler with data protection
            // - IOSTimeHandler
            // - SecRandomHandler
            // - OSLogConsoleHandler
            // - URLSessionTransportHandler

            // For now, fall back to testing mode until iOS handlers are implemented
            let _ = (app_group, authority_id, context_id, self.keychain_access_group, self.data_protection_class, self.execution_mode, self.config);

            Err(BuildError::EffectInit {
                effect: "ios",
                message: "iOS handlers not yet implemented. This is a placeholder for future development.".to_string(),
            }.into())
        }
    }
}

impl Default for IosPresetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the default iOS data directory (app container).
#[allow(dead_code)]
fn default_ios_data_dir() -> PathBuf {
    // On iOS, this would be obtained from:
    // FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup)
    PathBuf::from("~/Library/Application Support/aura")
}

// Extend AgentBuilder with iOS preset entry point
impl AgentBuilder {
    /// Create an iOS preset builder for Apple platform applications.
    ///
    /// This provides sensible defaults for iOS apps:
    /// - Keychain-based crypto storage
    /// - App container storage with data protection
    /// - System time and random sources
    /// - os_log-based console output
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let agent = AgentBuilder::ios()
    ///     .app_group("group.com.example.aura")
    ///     .build()
    ///     .await?;
    /// ```
    ///
    /// # Feature Flag
    ///
    /// Requires the `ios` feature flag for full functionality.
    /// Without it, `build()` will return an error.
    pub fn ios() -> IosPresetBuilder {
        IosPresetBuilder::new()
    }
}
