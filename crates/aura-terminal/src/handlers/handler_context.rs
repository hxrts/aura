//! Handler Context Types for Standardized Handler Signatures (Task 2.2)
//!
//! This module provides standardized types for CLI handler functions:
//! - `HandlerContext`: Wraps EffectContext + AuraEffectSystem for unified parameter passing
//! - `HandlerResult`: Standardized return type for reactive view integration
//!
//! Handlers use `HandlerContext` for consistent signatures and return
//! `Result<HandlerResult, CliError>` to drive view deltas.

// Import types from aura-app (pure app types) and aura-agent (runtime types)
use aura_app::core::ViewDelta;
use aura_core::identifiers::{ContextId, DeviceId};

// Import agent types from aura-agent (runtime layer)
pub use aura_agent::{AuraAgent, AuraEffectSystem, EffectContext};

/// Unified context for CLI handler functions
///
/// Wraps the effect context and effect system to provide a single
/// parameter for all handlers, enabling consistent signatures.
///
/// **Usage Pattern:**
/// ```rust,ignore
/// pub async fn handle_command(
///     ctx: &HandlerContext,
///     args: CommandArgs,
/// ) -> Result<(), CliError> {
///     ctx.effects().some_effect_call().await?;
///     Ok(())
/// }
/// ```
pub struct HandlerContext<'a> {
    effect_ctx: &'a EffectContext,
    effect_system: &'a AuraEffectSystem,
    device_id: DeviceId,
    agent: Option<&'a AuraAgent>,
}

impl<'a> HandlerContext<'a> {
    /// Create a new handler context
    pub fn new(
        effect_ctx: &'a EffectContext,
        effect_system: &'a AuraEffectSystem,
        device_id: DeviceId,
        agent: Option<&'a AuraAgent>,
    ) -> Self {
        Self {
            effect_ctx,
            effect_system,
            device_id,
            agent,
        }
    }

    /// Access the effect context for propagation through async calls
    pub fn effect_context(&self) -> &EffectContext {
        self.effect_ctx
    }

    /// Access the effect system for effect calls
    pub fn effects(&self) -> &AuraEffectSystem {
        self.effect_system
    }

    /// Get the device ID for this handler context
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the context ID from the effect context
    pub fn context_id(&self) -> ContextId {
        self.effect_ctx.context_id()
    }

    /// Access the agent if available (for higher-level services)
    pub fn agent(&self) -> Option<&'a AuraAgent> {
        self.agent
    }
}

/// Standardized result type for CLI handler functions
///
/// Enables handlers to communicate their outcome in a way that
/// integrates with the reactive view system instead of printing directly.
///
/// # Variants
///
/// - `Success`: Handler completed with a message to display
/// - `Silent`: Handler completed with no output needed
/// - `ViewUpdate`: Handler triggered a view delta for reactive updates
/// - `Multiple`: Handler produced multiple view deltas
#[derive(Debug, Clone)]
pub enum HandlerResult {
    /// Handler completed successfully with a message to display
    Success {
        /// Message to show to the user
        message: String,
    },
    /// Handler completed with no output needed
    Silent,
    /// Handler triggered a single view delta for reactive updates
    ViewUpdate {
        /// The view delta to apply
        delta: ViewDelta,
    },
    /// Handler triggered multiple view deltas
    Multiple {
        /// All view deltas to apply in order
        deltas: Vec<ViewDelta>,
    },
}

impl HandlerResult {
    /// Create a success result with a message
    pub fn success(message: impl Into<String>) -> Self {
        Self::Success {
            message: message.into(),
        }
    }

    /// Create a silent result
    pub fn silent() -> Self {
        Self::Silent
    }

    /// Create a view update result with a single delta
    pub fn view_update(delta: ViewDelta) -> Self {
        Self::ViewUpdate { delta }
    }

    /// Create a result with multiple deltas
    pub fn multiple(deltas: Vec<ViewDelta>) -> Self {
        Self::Multiple { deltas }
    }
}
