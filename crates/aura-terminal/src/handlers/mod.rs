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
use aura_app::ui::prelude::*;
use aura_core::types::identifiers::DeviceId;
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
/// Every `handle_*` method returns the command's structured [`CliOutput`];
/// the caller renders it as text or JSON.
pub struct CliHandler {
    /// The portable application core (provides intent-based state management)
    app_core: Arc<RwLock<AppCore>>,
    /// The agent for effect system and service access
    agent: Arc<AuraAgent>,
    /// The device ID for this handler
    device_id: DeviceId,
    /// Execution context propagated through effect calls
    effect_context: EffectContext,
    /// Destructive commands run without prompting (`--yes`).
    assume_yes: bool,
}

impl CliHandler {
    /// Create a new CLI handler with AppCore and agent
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
            assume_yes: false,
        }
    }

    /// Confirm destructive commands without prompting (`--yes`).
    #[must_use]
    pub fn assume_yes(mut self, assume_yes: bool) -> Self {
        self.assume_yes = assume_yes;
        self
    }

    /// Get the device ID for this handler
    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Access the effect context for downstream operations
    #[must_use]
    pub fn effect_context(&self) -> &EffectContext {
        &self.effect_context
    }

    /// Access the AppCore (for advanced operations)
    #[must_use]
    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    /// Access the agent (for effect system and service access)
    #[must_use]
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
    pub async fn handle_init(
        &self,
        num_devices: u32,
        threshold: u32,
        output_dir: &Path,
    ) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        init::handle_init(
            &self.make_ctx(&effects, false),
            num_devices,
            threshold,
            output_dir,
        )
        .await
    }

    /// Handle status command for a device config file
    pub async fn handle_status(&self, config_path: &Path) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        let mut output =
            status::handle_status(&self.make_ctx(&effects, false), config_path).await?;
        let current_budget =
            aura_app::ui::workflows::budget::get_current_budget(&self.app_core).await;
        output.section("Home Storage Budget");
        output.println(budget::format_budget_status(&current_budget));
        Ok(output)
    }

    /// Report the loaded account: identity, threshold, devices and contacts.
    pub async fn handle_account_status(&self) -> TerminalResult<CliOutput> {
        let runtime = self.agent.clone().as_runtime_bridge();
        let settings = runtime
            .try_get_settings()
            .await
            .map_err(|e| TerminalError::Operation(e.to_string()))?;
        let mut output = CliOutput::new();
        output.section("Account Status");
        output.kv("Authority", self.agent.authority_id().to_string());
        output.kv("Nickname", settings.nickname_suggestion.clone());
        output.kv(
            "Threshold",
            format!("{} of {}", settings.threshold_k, settings.threshold_n),
        );
        output.kv("Devices", settings.device_count.to_string());
        output.kv("Contacts", settings.contact_count.to_string());
        Ok(output)
    }

    /// Handle node command through effects
    pub async fn handle_node(
        &self,
        port: u16,
        daemon: bool,
        config_path: &Path,
    ) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        node::handle_node(&self.make_ctx(&effects, false), port, daemon, config_path).await
    }

    /// Handle threshold command through effects
    pub async fn handle_threshold(
        &self,
        configs: &str,
        threshold: u32,
        mode: &str,
        message: Option<&str>,
        message_hex: Option<&str>,
        signature: Option<&str>,
    ) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        threshold::handle_threshold(
            &self.make_ctx(&effects, false),
            configs,
            threshold,
            mode,
            message,
            message_hex,
            signature,
        )
        .await
    }

    /// Handle scenarios command through effects (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        scenarios::handle_scenarios(&self.make_ctx(&effects, false), action).await?;
        Ok(CliOutput::new())
    }

    /// Handle snapshot maintenance commands.
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        snapshot::handle_snapshot(&self.make_ctx(&effects, false), action).await
    }

    /// Handle admin maintenance commands.
    pub async fn handle_admin(&self, action: &AdminAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        admin::handle_admin(&self.make_ctx(&effects, false), action).await
    }

    /// Handle guardian recovery commands
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        recovery::handle_recovery(&self.make_ctx(&effects, false), action).await
    }

    /// Handle invitation commands
    pub async fn handle_invitation(&self, action: &InvitationAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        invite::handle_invitation(&self.make_ctx(&effects, true), action).await
    }

    /// Handle authority management commands
    pub async fn handle_authority(&self, command: &AuthorityCommands) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        authority::handle_authority(&self.make_ctx(&effects, false), command).await
    }

    /// Handle context inspection commands
    pub async fn handle_context(&self, action: &ContextAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        context::handle_context(&self.make_ctx(&effects, false), action).await
    }

    /// Handle OTA upgrade commands
    pub async fn handle_ota(&self, action: &OtaAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        ota::handle_ota(&self.make_ctx(&effects, false), action).await
    }

    /// Handle AMP commands routed through the effect system.
    pub async fn handle_amp(&self, action: &AmpAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        amp::handle_amp(&self.make_ctx(&effects, false), action).await
    }

    /// Handle chat commands
    pub async fn handle_chat(&self, command: &ChatCommands) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        chat::handle_chat(&self.make_ctx(&effects, true), command, self.assume_yes).await
    }

    /// Handle sync commands (daemon mode by default)
    pub async fn handle_sync(&self, action: &SyncAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        sync::handle_sync(&self.make_ctx(&effects, true), action).await
    }

    /// Handle demo commands (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_demo(&self, command: &DemoCommands) -> TerminalResult<CliOutput> {
        demo::DemoHandler::handle_demo_command(command.clone())
            .await
            .map_err(|e| TerminalError::Operation(format!("Demo command failed: {e}")))?;
        Ok(CliOutput::new())
    }

    /// Handle TUI commands for production terminal interface
    #[cfg(feature = "terminal")]
    pub async fn handle_tui(&self, args: &TuiArgs) -> TerminalResult<()> {
        tui::handle_tui(args)
            .await
            .map_err(|e| TerminalError::Operation(format!("TUI command failed: {e}")))
    }
}
