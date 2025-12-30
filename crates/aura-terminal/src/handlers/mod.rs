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
//! ) -> TerminalResult<()> {
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

use crate::error::{TerminalError, TerminalResult};
use crate::{
    AdminAction, AmpAction, AuthorityCommands, ChatCommands, ContextAction, InvitationAction,
    OtaAction, RecoveryAction, SnapshotAction, SyncAction,
};

#[cfg(feature = "terminal")]
use crate::cli::tui::TuiArgs;

#[cfg(feature = "development")]
use crate::{DemoCommands, ScenarioAction};
use async_lock::RwLock;
use aura_app::AppCore;
use aura_core::identifiers::DeviceId;
use std::path::Path;
use std::sync::Arc;

// Re-export agent types through handler_context for convenience
pub use handler_context::{AuraAgent, AuraEffectSystem, EffectContext};

pub mod admin;
pub mod amp;
pub mod authority;
pub mod budget;
pub mod chat;
pub mod cli_output;
pub mod config;
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
pub mod tui_stdio;
pub mod version;

// Re-export CLI output types
pub use cli_output::{CliOutput, CliOutputBuilder, OutputLine};

// Re-export for convenience
pub use handler_context::HandlerContext;

// Demo and scenarios modules require simulator - only available with development feature
#[cfg(feature = "development")]
pub mod demo;
#[cfg(feature = "development")]
pub mod scenarios;

/// Main CLI handler that coordinates all operations through effects
///
/// Uses `AppCore` as the unified backend for intent-based state management,
/// and stores the `AuraAgent` directly for effect system and service access.
/// This follows the dependency inversion pattern where aura-app doesn't
/// depend on aura-agent.
pub struct CliHandler {
    /// The portable application core (provides intent-based state management)
    app_core: Arc<RwLock<AppCore>>,
    /// The agent for effect system and service access
    agent: Arc<AuraAgent>,
    /// The device ID for this handler
    device_id: DeviceId,
    /// Execution context propagated through effect calls
    effect_context: EffectContext,
}

impl CliHandler {
    /// Create a new CLI handler with AppCore and agent
    ///
    /// This constructor uses both AppCore (for intent-based state) and
    /// the agent directly (for effect system and services).
    pub fn with_agent(
        app_core: Arc<RwLock<AppCore>>,
        agent: Arc<AuraAgent>,
        device_id: DeviceId,
        effect_context: EffectContext,
    ) -> Self {
        Self {
            app_core,
            agent,
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

    /// Access the agent (for effect system and service access)
    pub fn agent(&self) -> &Arc<AuraAgent> {
        &self.agent
    }

    /// Build a HandlerContext from an effects guard.
    fn make_ctx<'a>(
        &'a self,
        effects: &'a AuraEffectSystem,
        include_agent: bool,
    ) -> HandlerContext<'a> {
        let agent_opt = if include_agent {
            Some(&*self.agent)
        } else {
            None
        };
        HandlerContext::new(&self.effect_context, effects, self.device_id, agent_opt)
    }

    /// Handle init command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_init(
        &self,
        num_devices: u32,
        threshold: u32,
        output_dir: &Path,
    ) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = init::handle_init(&ctx, num_devices, threshold, output_dir).await?;
        output.render();
        Ok(())
    }

    /// Handle status command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_status(&self, config_path: &Path) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = status::handle_status(&ctx, config_path).await?;

        // Render device status
        output.render();

        // Add budget information using shared handlers
        let current_budget = budget::get_current_budget(&self.app_core).await;
        let budget_status = budget::format_budget_status(&current_budget);

        println!();
        println!("=== Home Storage Budget ===");
        println!("{budget_status}");

        Ok(())
    }

    /// Handle node command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_node(
        &self,
        port: u16,
        daemon: bool,
        config_path: &Path,
    ) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = node::handle_node(&ctx, port, daemon, config_path).await?;
        output.render();
        Ok(())
    }

    /// Handle threshold command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_threshold(
        &self,
        configs: &str,
        threshold: u32,
        mode: &str,
    ) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = threshold::handle_threshold(&ctx, configs, threshold, mode).await?;
        output.render();
        Ok(())
    }

    /// Handle scenarios command through effects (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        scenarios::handle_scenarios(&ctx, action).await
    }

    /// Handle version command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_version(&self) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = version::handle_version(&ctx).await?;
        output.render();
        Ok(())
    }

    /// Handle snapshot maintenance commands.
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = snapshot::handle_snapshot(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle admin maintenance commands.
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_admin(&self, action: &AdminAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = admin::handle_admin(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle guardian recovery commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = recovery::handle_recovery(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle invitation commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_invitation(&self, action: &InvitationAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, true);
        let output = invite::handle_invitation(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle authority management commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_authority(&self, command: &AuthorityCommands) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = authority::handle_authority(&ctx, command).await?;
        output.render();
        Ok(())
    }

    /// Handle context inspection commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_context(&self, action: &ContextAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = context::handle_context(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle OTA upgrade commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_ota(&self, action: &OtaAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = ota::handle_ota(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle AMP commands routed through the effect system.
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_amp(&self, action: &AmpAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = amp::handle_amp(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle chat commands
    pub async fn handle_chat(&self, command: &ChatCommands) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        chat::handle_chat(&ctx, effects, command).await
    }

    /// Handle sync commands (daemon mode by default)
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_sync(&self, action: &SyncAction) -> TerminalResult<()> {
        let effects_arc = self.agent.runtime().effects();
        let effects = &*effects_arc;
        let ctx = self.make_ctx(effects, false);
        let output = sync::handle_sync(&ctx, action).await?;
        output.render();
        Ok(())
    }

    /// Handle demo commands (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_demo(&self, command: &DemoCommands) -> TerminalResult<()> {
        demo::DemoHandler::handle_demo_command(command.clone())
            .await
            .map_err(|e| TerminalError::Operation(format!("Demo command failed: {}", e)))
    }

    /// Handle TUI commands for production terminal interface
    #[cfg(feature = "terminal")]
    pub async fn handle_tui(&self, args: &TuiArgs) -> TerminalResult<()> {
        tui::handle_tui(args)
            .await
            .map_err(|e| TerminalError::Operation(format!("TUI command failed: {e}")))
    }

    /// Log error message through effects
    pub async fn log_error(&self, message: &str) {
        eprintln!("ERROR: {message}");
    }

    /// Log info message through effects
    pub async fn log_info(&self, message: &str) {
        println!("INFO: {message}");
    }
}
