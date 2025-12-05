//! # CLI Command Handlers
//!
//! Layer 7 (User Interface) - Effect-based implementations of user-facing CLI commands.
//!
//! ## Architecture
//!
//! Handlers sit between CLI argument parsing and the effect system:
//!
//! ```text
//! CLI Args → Handlers → Effects → Facts → Views → UI
//! ```
//!
//! ## Responsibilities
//!
//! - Orchestrate effect calls to implement command logic
//! - Validate arguments and business constraints
//! - Translate effect results to user feedback
//! - Handle errors gracefully
//! - Coordinate multi-step operations (e.g., recovery flows)
//!
//! ## Handler Pattern
//!
//! All handlers follow a standard signature using `HandlerContext`:
//!
//! ```ignore
//! use crate::handlers::HandlerContext;
//!
//! pub async fn handle_command(
//!     ctx: &HandlerContext<'_>,
//!     args: &CommandArgs,
//! ) -> Result<()> {
//!     // 1. Validate arguments
//!     // 2. Call effects via ctx.effects()
//!     // 3. Return result
//! }
//! ```
//!
//! ## Handler Modules
//!
//! - **Authority and Context**: `authority`, `context` - Authority/context inspection and management
//! - **Account Administration**: `admin`, `snapshot` - Administrative operations
//! - **Scenario Management**: `scenarios`, `amp` - Demo scenarios and AMP tests
//! - **Recovery Workflows**: `recovery` - Guardian-based recovery coordination
//! - **Invitations**: `invite` - Device onboarding and invitation flows
//! - **OTA Upgrades**: `ota` - Over-the-air update handling
//! - **Status Monitoring**: `status`, `version`, `node`, `threshold`, `init` - System status
//!
//! ## Adding a New Handler
//!
//! 1. Create handler function in appropriate module (or new module)
//! 2. Define command args in `cli_args/`
//! 3. Wire command → handler in main dispatch (handlers are called from main.rs)
//! 4. Add tests in `tests/handlers/`
//!
//! ## See Also
//!
//! - `cli_args/` - Command-line argument definitions (Clap)
//! - `handler_context` - Shared context type for all handlers
//! - `docs/001_system_architecture.md` - Layer 7 architecture

use crate::{
    AdminAction, AmpAction, AuthorityCommands, ChatCommands, ContextAction, InvitationAction,
    OtaAction, RecoveryAction, SnapshotAction, SyncAction,
};

#[cfg(feature = "terminal")]
use crate::cli::tui::TuiArgs;

#[cfg(feature = "development")]
use crate::{DemoCommands, ScenarioAction};
use anyhow::Result;
use aura_app::AppCore;
use aura_core::identifiers::DeviceId;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export agent types through handler_context for backward compatibility
pub use handler_context::{AuraAgent, AuraEffectSystem, EffectContext};

pub mod admin;
pub mod amp;
pub mod authority;
pub mod chat;
pub mod context;
pub mod handler_context;
pub mod init;
pub mod invite;
pub mod node;
pub mod ota;
pub mod recovery;
pub mod recovery_status;
pub mod snapshot;
pub mod status;
pub mod sync;
pub mod threshold;
#[cfg(feature = "terminal")]
pub mod tui;
pub mod version;

// Re-export for convenience
pub use handler_context::HandlerContext;

// Demo and scenarios modules require simulator - only available with development feature
#[cfg(feature = "development")]
pub mod demo;
#[cfg(feature = "development")]
pub mod scenarios;

/// Main CLI handler that coordinates all operations through effects
///
/// Uses `AppCore` as the unified backend, accessing agent and effect system
/// through AppCore's accessors for clean architecture.
pub struct CliHandler {
    /// The portable application core (wraps agent and provides unified API)
    app_core: Arc<RwLock<AppCore>>,
    /// The device ID for this handler
    device_id: DeviceId,
    /// Execution context propagated through effect calls
    effect_context: EffectContext,
}

impl CliHandler {
    /// Create a new CLI handler with AppCore
    ///
    /// This is the preferred constructor that uses the unified AppCore backend.
    pub fn new(
        app_core: Arc<RwLock<AppCore>>,
        device_id: DeviceId,
        effect_context: EffectContext,
    ) -> Self {
        Self {
            app_core,
            device_id,
            effect_context,
        }
    }

    /// Get the device ID for this handler
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Access the effect context for downstream operations
    pub fn effect_context(&self) -> &EffectContext {
        &self.effect_context
    }

    /// Access the AppCore (for advanced operations)
    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    /// Handle init command through effects
    pub async fn handle_init(&self, num_devices: u32, threshold: u32, output: &Path) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        init::handle_init(&ctx, num_devices, threshold, output).await
    }

    /// Handle status command through effects
    pub async fn handle_status(&self, config_path: &Path) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        status::handle_status(&ctx, config_path).await
    }

    /// Handle node command through effects
    pub async fn handle_node(&self, port: u16, daemon: bool, config_path: &Path) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        node::handle_node(&ctx, port, daemon, config_path).await
    }

    /// Handle threshold command through effects
    pub async fn handle_threshold(&self, configs: &str, threshold: u32, mode: &str) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        threshold::handle_threshold(&ctx, configs, threshold, mode).await
    }

    /// Handle scenarios command through effects (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        scenarios::handle_scenarios(&ctx, action).await
    }

    /// Handle version command through effects
    pub async fn handle_version(&self) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        version::handle_version(&ctx).await
    }

    /// Handle snapshot maintenance commands.
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        snapshot::handle_snapshot(&ctx, action).await
    }

    /// Handle admin maintenance commands.
    pub async fn handle_admin(&self, action: &AdminAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        admin::handle_admin(&ctx, action).await
    }

    /// Handle guardian recovery commands
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        recovery::handle_recovery(&ctx, action).await
    }

    /// Handle invitation commands
    pub async fn handle_invitation(&self, action: &InvitationAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        // For invitation, we pass agent reference
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, Some(agent));
        invite::handle_invitation(&ctx, action).await
    }

    /// Handle authority management commands
    pub async fn handle_authority(&self, command: &AuthorityCommands) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        authority::handle_authority(&ctx, command).await
    }

    /// Handle context inspection commands
    pub async fn handle_context(&self, action: &ContextAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        context::handle_context(&ctx, action).await
    }

    /// Handle OTA upgrade commands
    pub async fn handle_ota(&self, action: &OtaAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        ota::handle_ota(&ctx, action).await
    }

    /// Handle AMP commands routed through the effect system.
    pub async fn handle_amp(&self, action: &AmpAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        amp::handle_amp(&ctx, action).await
    }

    /// Handle chat commands
    pub async fn handle_chat(&self, command: &ChatCommands) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        chat::handle_chat(&ctx, &effects, command).await
    }

    /// Handle sync commands (daemon mode by default)
    pub async fn handle_sync(&self, action: &SyncAction) -> Result<()> {
        let app = self.app_core.read().await;
        let agent = app
            .agent()
            .ok_or_else(|| anyhow::anyhow!("No agent available"))?;
        let effects_arc = agent.runtime().effects();
        let effects = effects_arc.read().await;
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, None);
        sync::handle_sync(&ctx, action).await
    }

    /// Handle demo commands (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_demo(&self, command: &DemoCommands) -> Result<()> {
        demo::DemoHandler::handle_demo_command(command.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Demo command failed: {}", e))
    }

    /// Handle TUI commands for production terminal interface
    #[cfg(feature = "terminal")]
    pub async fn handle_tui(&self, args: &TuiArgs) -> Result<()> {
        tui::handle_tui(args)
            .await
            .map_err(|e| anyhow::anyhow!("TUI command failed: {}", e))
    }

    /// Log error message through effects
    pub async fn log_error(&self, message: &str) {
        eprintln!("ERROR: {}", message);
    }

    /// Log info message through effects
    pub async fn log_info(&self, message: &str) {
        println!("INFO: {}", message);
    }
}
