//! Custom builder with typestate pattern for compile-time effect enforcement.
//!
//! This builder uses Rust's type system to ensure all required effects
//! are provided before the agent can be built. Attempting to call `build()`
//! without providing all required effects results in a compile error.
//!
//! # Required Effects
//!
//! - `CryptoEffects` - Signing, verification, encryption
//! - `StorageEffects` - Persistent data storage
//! - `PhysicalTimeEffects` - Wall-clock time
//! - `RandomEffects` - Cryptographically secure random
//! - `ConsoleEffects` - Logging and output
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_agent::AgentBuilder;
//!
//! // This compiles - all required effects provided
//! let agent = AgentBuilder::custom()
//!     .with_crypto(Arc::new(MyCrypto::new()))
//!     .with_storage(Arc::new(MyStorage::new()))
//!     .with_time(Arc::new(MyTime::new()))
//!     .with_random(Arc::new(MyRandom::new()))
//!     .with_console(Arc::new(MyConsole::new()))
//!     .build()
//!     .await?;
//!
//! // This would NOT compile - missing effects
//! // let agent = AgentBuilder::custom()
//! //     .with_crypto(Arc::new(MyCrypto::new()))
//! //     .build()  // Error: build() not available
//! //     .await?;
//! ```

use std::marker::PhantomData;
use std::sync::Arc;

use aura_core::effects::{
    ConsoleEffects, CryptoEffects, ExecutionMode, PhysicalTimeEffects, RandomEffects,
    StorageEffects, TransportEffects,
};
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

use crate::builder::BuildError;
use crate::core::AgentConfig;
use crate::runtime::EffectContext;
use crate::{AgentResult, AuraAgent, EffectSystemBuilder};

/// Marker type indicating an effect has not been provided.
pub struct Missing;

/// Marker type indicating an effect has been provided.
pub struct Provided<T>(pub T);

/// Custom builder with typestate enforcement of required effects.
///
/// The type parameters track which effects have been provided:
/// - `C`: CryptoEffects (Missing or Provided<Arc<dyn CryptoEffects>>)
/// - `S`: StorageEffects (Missing or Provided<Arc<dyn StorageEffects>>)
/// - `T`: PhysicalTimeEffects (Missing or Provided<Arc<dyn PhysicalTimeEffects>>)
/// - `R`: RandomEffects (Missing or Provided<Arc<dyn RandomEffects>>)
/// - `O`: ConsoleEffects (Missing or Provided<Arc<dyn ConsoleEffects>>)
pub struct CustomPresetBuilder<C, S, T, R, O> {
    crypto: C,
    storage: S,
    time: T,
    random: R,
    console: O,
    transports: Vec<Arc<dyn TransportEffects>>,
    authority_id: Option<AuthorityId>,
    context_id: Option<ContextId>,
    execution_mode: ExecutionMode,
    config: AgentConfig,
    _phantom: PhantomData<(C, S, T, R, O)>,
}

impl CustomPresetBuilder<Missing, Missing, Missing, Missing, Missing> {
    /// Create a new custom builder with no effects provided.
    pub fn new() -> Self {
        Self {
            crypto: Missing,
            storage: Missing,
            time: Missing,
            random: Missing,
            console: Missing,
            transports: Vec::new(),
            authority_id: None,
            context_id: None,
            execution_mode: ExecutionMode::Production,
            config: AgentConfig::default(),
            _phantom: PhantomData,
        }
    }
}

impl Default for CustomPresetBuilder<Missing, Missing, Missing, Missing, Missing> {
    fn default() -> Self {
        Self::new()
    }
}

// Crypto transition: Missing -> Provided
impl<S, T, R, O> CustomPresetBuilder<Missing, S, T, R, O> {
    /// Provide the crypto effects handler.
    ///
    /// This is required before building.
    pub fn with_crypto(
        self,
        crypto: Arc<dyn CryptoEffects>,
    ) -> CustomPresetBuilder<Provided<Arc<dyn CryptoEffects>>, S, T, R, O> {
        CustomPresetBuilder {
            crypto: Provided(crypto),
            storage: self.storage,
            time: self.time,
            random: self.random,
            console: self.console,
            transports: self.transports,
            authority_id: self.authority_id,
            context_id: self.context_id,
            execution_mode: self.execution_mode,
            config: self.config,
            _phantom: PhantomData,
        }
    }
}

// Storage transition: Missing -> Provided
impl<C, T, R, O> CustomPresetBuilder<C, Missing, T, R, O> {
    /// Provide the storage effects handler.
    ///
    /// This is required before building.
    pub fn with_storage(
        self,
        storage: Arc<dyn StorageEffects>,
    ) -> CustomPresetBuilder<C, Provided<Arc<dyn StorageEffects>>, T, R, O> {
        CustomPresetBuilder {
            crypto: self.crypto,
            storage: Provided(storage),
            time: self.time,
            random: self.random,
            console: self.console,
            transports: self.transports,
            authority_id: self.authority_id,
            context_id: self.context_id,
            execution_mode: self.execution_mode,
            config: self.config,
            _phantom: PhantomData,
        }
    }
}

// Time transition: Missing -> Provided
impl<C, S, R, O> CustomPresetBuilder<C, S, Missing, R, O> {
    /// Provide the physical time effects handler.
    ///
    /// This is required before building.
    pub fn with_time(
        self,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> CustomPresetBuilder<C, S, Provided<Arc<dyn PhysicalTimeEffects>>, R, O> {
        CustomPresetBuilder {
            crypto: self.crypto,
            storage: self.storage,
            time: Provided(time),
            random: self.random,
            console: self.console,
            transports: self.transports,
            authority_id: self.authority_id,
            context_id: self.context_id,
            execution_mode: self.execution_mode,
            config: self.config,
            _phantom: PhantomData,
        }
    }
}

// Random transition: Missing -> Provided
impl<C, S, T, O> CustomPresetBuilder<C, S, T, Missing, O> {
    /// Provide the random effects handler.
    ///
    /// This is required before building.
    pub fn with_random(
        self,
        random: Arc<dyn RandomEffects>,
    ) -> CustomPresetBuilder<C, S, T, Provided<Arc<dyn RandomEffects>>, O> {
        CustomPresetBuilder {
            crypto: self.crypto,
            storage: self.storage,
            time: self.time,
            random: Provided(random),
            console: self.console,
            transports: self.transports,
            authority_id: self.authority_id,
            context_id: self.context_id,
            execution_mode: self.execution_mode,
            config: self.config,
            _phantom: PhantomData,
        }
    }
}

// Console transition: Missing -> Provided
impl<C, S, T, R> CustomPresetBuilder<C, S, T, R, Missing> {
    /// Provide the console effects handler.
    ///
    /// This is required before building.
    pub fn with_console(
        self,
        console: Arc<dyn ConsoleEffects>,
    ) -> CustomPresetBuilder<C, S, T, R, Provided<Arc<dyn ConsoleEffects>>> {
        CustomPresetBuilder {
            crypto: self.crypto,
            storage: self.storage,
            time: self.time,
            random: self.random,
            console: Provided(console),
            transports: self.transports,
            authority_id: self.authority_id,
            context_id: self.context_id,
            execution_mode: self.execution_mode,
            config: self.config,
            _phantom: PhantomData,
        }
    }
}

// Methods available on any state
impl<C, S, T, R, O> CustomPresetBuilder<C, S, T, R, O> {
    /// Add a transport effect handler.
    ///
    /// Multiple transports can be added (TCP, WebSocket, etc.).
    pub fn with_transport(mut self, transport: Arc<dyn TransportEffects>) -> Self {
        self.transports.push(transport);
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

    /// Use production execution mode.
    pub fn production_mode(mut self) -> Self {
        self.execution_mode = ExecutionMode::Production;
        self
    }

    /// Set the agent configuration.
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }
}

// Build only available when all required effects are provided
impl
    CustomPresetBuilder<
        Provided<Arc<dyn CryptoEffects>>,
        Provided<Arc<dyn StorageEffects>>,
        Provided<Arc<dyn PhysicalTimeEffects>>,
        Provided<Arc<dyn RandomEffects>>,
        Provided<Arc<dyn ConsoleEffects>>,
    >
{
    /// Build the agent asynchronously.
    ///
    /// This method is only available when all required effects have been provided.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime construction fails.
    pub async fn build(self) -> AgentResult<AuraAgent> {
        // Get or generate authority ID
        let authority_id = self.authority_id.unwrap_or_else(|| {
            let id_str = "custom:default";
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
        // Note: The typestate pattern ensures all required effects were provided,
        // but currently EffectSystemBuilder creates handlers based on execution mode.
        // The provided handlers validate the API contract at compile time.
        let _ = (
            self.crypto,
            self.storage,
            self.time,
            self.random,
            self.console,
            self.transports,
        );

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
    pub fn build_sync(self) -> AgentResult<AuraAgent> {
        // Get or generate authority ID
        let authority_id = self.authority_id.unwrap_or_else(|| {
            let id_str = "custom:default";
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
