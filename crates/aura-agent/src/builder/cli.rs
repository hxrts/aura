//! CLI preset builder for terminal applications.
//!
//! Provides sensible defaults for command-line applications:
//! - File-based storage in a configurable data directory
//! - Real cryptographic operations
//! - TCP transport for P2P communication
//! - Stderr logging

use std::path::PathBuf;

use aura_core::effects::ExecutionMode;
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

use crate::builder::BuildError;
use crate::core::config::default_storage_path;
use crate::core::AgentConfig;
use crate::runtime::EffectContext;
use crate::{AgentBuilder, AgentResult, AuraAgent, EffectSystemBuilder};

/// CLI-specific builder with sensible defaults for terminal applications.
///
/// # Example
///
/// ```rust,ignore
/// use aura_agent::AgentBuilder;
///
/// let agent = AgentBuilder::cli()
///     .data_dir("~/.aura")
///     .authority(my_authority_id)
///     .testing_mode()  // Use testing mode for development
///     .build()
///     .await?;
/// ```
pub struct CliPresetBuilder {
    data_dir: Option<PathBuf>,
    authority_id: Option<AuthorityId>,
    context_id: Option<ContextId>,
    execution_mode: ExecutionMode,
    config: AgentConfig,
}

impl CliPresetBuilder {
    /// Create a new CLI preset builder with defaults.
    pub fn new() -> Self {
        Self {
            data_dir: None,
            authority_id: None,
            context_id: None,
            execution_mode: ExecutionMode::Production,
            config: AgentConfig::default(),
        }
    }

    /// Set the data directory for persistent storage.
    ///
    /// Defaults to platform-appropriate location if not set:
    /// - Linux: `~/.local/share/aura`
    /// - macOS: `~/Library/Application Support/aura`
    /// - Windows: `%APPDATA%\aura`
    pub fn data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(path.into());
        self
    }

    /// Set the authority ID for this agent.
    ///
    /// If not set, a default authority ID is generated.
    pub fn authority(mut self, id: AuthorityId) -> Self {
        self.authority_id = Some(id);
        self
    }

    /// Set the default context ID for this agent.
    ///
    /// If not set, derived from the authority ID.
    pub fn context(mut self, id: ContextId) -> Self {
        self.context_id = Some(id);
        self
    }

    /// Use testing execution mode.
    ///
    /// This uses deterministic behavior suitable for testing.
    pub fn testing_mode(mut self) -> Self {
        self.execution_mode = ExecutionMode::Testing;
        self
    }

    /// Use simulation execution mode with a specific seed.
    ///
    /// This uses deterministic behavior controlled by the seed.
    pub fn simulation_mode(mut self, seed: u64) -> Self {
        self.execution_mode = ExecutionMode::Simulation { seed };
        self
    }

    /// Use production execution mode.
    ///
    /// This uses real system operations.
    pub fn production_mode(mut self) -> Self {
        self.execution_mode = ExecutionMode::Production;
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
    /// - The data directory cannot be created or accessed
    /// - Effect handlers fail to initialize
    /// - Runtime construction fails
    pub async fn build(self) -> AgentResult<AuraAgent> {
        let data_dir = self.data_dir.unwrap_or_else(default_storage_path);

        // Ensure data directory exists
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).map_err(|e| BuildError::EffectInit {
                effect: "storage",
                message: format!("failed to create data directory: {}", e),
            })?;
        }

        // Get or generate authority ID
        let authority_id = self.authority_id.unwrap_or_else(|| {
            // Generate a deterministic authority ID from data directory path
            let id_str = format!("cli:{}", data_dir.display());
            AuthorityId::new_from_entropy(hash::hash(id_str.as_bytes()))
        });

        // Derive context ID if not set
        let context_id = self.context_id.unwrap_or_else(|| {
            let context_entropy = hash::hash(&authority_id.to_bytes());
            ContextId::new_from_entropy(context_entropy)
        });

        // Create effect context for building
        let effect_context = EffectContext::new(authority_id, context_id, self.execution_mode);

        // Build the runtime system
        let runtime = match self.execution_mode {
            ExecutionMode::Testing => EffectSystemBuilder::testing()
                .with_config(self.config)
                .with_authority(authority_id)
                .build(&effect_context)
                .await
                .map_err(BuildError::RuntimeConstruction)?,
            ExecutionMode::Production => EffectSystemBuilder::production()
                .with_config(self.config)
                .with_authority(authority_id)
                .build(&effect_context)
                .await
                .map_err(BuildError::RuntimeConstruction)?,
            ExecutionMode::Simulation { seed } => EffectSystemBuilder::simulation(seed)
                .with_config(self.config)
                .with_authority(authority_id)
                .build(&effect_context)
                .await
                .map_err(BuildError::RuntimeConstruction)?,
        };

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build the agent synchronously (for testing).
    ///
    /// This uses `build_sync` internally, which is suitable for tests
    /// that don't have an async runtime available.
    pub fn build_sync(self) -> AgentResult<AuraAgent> {
        let data_dir = self.data_dir.unwrap_or_else(default_storage_path);

        // Ensure data directory exists
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).map_err(|e| BuildError::EffectInit {
                effect: "storage",
                message: format!("failed to create data directory: {}", e),
            })?;
        }

        // Get or generate authority ID
        let authority_id = self.authority_id.unwrap_or_else(|| {
            // Generate a deterministic authority ID from data directory path
            let id_str = format!("cli:{}", data_dir.display());
            AuthorityId::new_from_entropy(hash::hash(id_str.as_bytes()))
        });

        // Build using existing infrastructure
        let runtime = match self.execution_mode {
            ExecutionMode::Testing => EffectSystemBuilder::testing()
                .with_config(self.config)
                .with_authority(authority_id)
                .build_sync()
                .map_err(BuildError::RuntimeConstruction)?,
            ExecutionMode::Production => {
                return Err(BuildError::RuntimeConstruction(
                    "production mode requires async build".to_string(),
                )
                .into());
            }
            ExecutionMode::Simulation { seed } => EffectSystemBuilder::simulation(seed)
                .with_config(self.config)
                .with_authority(authority_id)
                .build_sync()
                .map_err(BuildError::RuntimeConstruction)?,
        };

        Ok(AuraAgent::new(runtime, authority_id))
    }
}

impl Default for CliPresetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// Extend AgentBuilder with CLI preset entry point
impl AgentBuilder {
    /// Create a CLI preset builder for terminal applications.
    ///
    /// This provides sensible defaults for command-line tools:
    /// - File-based storage
    /// - Real cryptographic operations
    /// - TCP transport
    /// - Stderr logging
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let agent = AgentBuilder::cli()
    ///     .data_dir("~/.aura")
    ///     .testing_mode()
    ///     .build()
    ///     .await?;
    /// ```
    pub fn cli() -> CliPresetBuilder {
        CliPresetBuilder::new()
    }
}
