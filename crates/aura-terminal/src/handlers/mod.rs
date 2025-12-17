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

// Re-export agent types through handler_context for convenience
pub use handler_context::{AuraAgent, AuraEffectSystem, EffectContext};

pub mod admin;
pub mod amp;
pub mod authority;
pub mod chat;
pub mod cli_output;
pub mod context;
pub mod config;
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

// Re-export CLI output types
pub use cli_output::{CliOutput, CliOutputBuilder, OutputLine};

// Re-export for convenience
pub use handler_context::HandlerContext;

// Demo and scenarios modules require simulator - only available with development feature
#[cfg(feature = "development")]
pub mod demo;
#[cfg(feature = "development")]
pub mod scenarios;

use std::future::Future;

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

    /// Helper to build a HandlerContext and run an async block with it.
    async fn with_ctx<F, Fut, R>(&self, include_agent: bool, f: F) -> R
    where
        F: for<'a> FnOnce(HandlerContext<'a>, &'a AuraEffectSystem) -> Fut,
        Fut: Future<Output = R>,
    {
        let effects_arc = self.agent.runtime().effects();
        let effects = effects_arc.read().await;
        let agent_opt = if include_agent { Some(&*self.agent) } else { None };
        let ctx = HandlerContext::new(&self.effect_context, &effects, self.device_id, agent_opt);
        f(ctx, &effects).await
    }

    /// Handle init command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_init(
        &self,
        num_devices: u32,
        threshold: u32,
        output_dir: &Path,
    ) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = init::handle_init(&ctx, num_devices, threshold, output_dir).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle status command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_status(&self, config_path: &Path) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = status::handle_status(&ctx, config_path).await?;
            output.render();
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))
    }

    /// Handle node command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_node(&self, port: u16, daemon: bool, config_path: &Path) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = node::handle_node(&ctx, port, daemon, config_path).await?;
            output.render();
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))
    }

    /// Handle threshold command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_threshold(&self, configs: &str, threshold: u32, mode: &str) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = threshold::handle_threshold(&ctx, configs, threshold, mode).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle scenarios command through effects (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move { scenarios::handle_scenarios(&ctx, action).await })
            .await
    }

    /// Handle version command through effects
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_version(&self) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = version::handle_version(&ctx).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle snapshot maintenance commands.
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = snapshot::handle_snapshot(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle admin maintenance commands.
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_admin(&self, action: &AdminAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = admin::handle_admin(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle guardian recovery commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = recovery::handle_recovery(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle invitation commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_invitation(&self, action: &InvitationAction) -> Result<()> {
        self.with_ctx(true, |ctx, _| async move {
            let output = invite::handle_invitation(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle authority management commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_authority(&self, command: &AuthorityCommands) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = authority::handle_authority(&ctx, command).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle context inspection commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_context(&self, action: &ContextAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = context::handle_context(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle OTA upgrade commands
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_ota(&self, action: &OtaAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = ota::handle_ota(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle AMP commands routed through the effect system.
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_amp(&self, action: &AmpAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = amp::handle_amp(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
    }

    /// Handle chat commands
    pub async fn handle_chat(&self, command: &ChatCommands) -> Result<()> {
        self.with_ctx(false, |ctx, effects| async move {
            chat::handle_chat(&ctx, effects, command).await
        })
        .await
    }

    /// Handle sync commands (daemon mode by default)
    ///
    /// Returns structured output that is rendered to stdout/stderr
    pub async fn handle_sync(&self, action: &SyncAction) -> Result<()> {
        self.with_ctx(false, |ctx, _| async move {
            let output = sync::handle_sync(&ctx, action).await?;
            output.render();
            Ok(())
        })
        .await
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
