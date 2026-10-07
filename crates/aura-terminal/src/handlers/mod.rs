//! # CLI Handlers
//!
//! Layer 7 (User Interface).
//!
//! Account commands do not live here: they are typed requests executed by
//! the shared command model (`crate::command`) through
//! `aura_app::ui::workflows`. This module keeps the TUI launcher, the
//! structured output type, and the offline or long-running tools:
//!
//! - `init`, `threshold`: offline device-config tools
//! - `sync`: the foreground sync daemon
//! - `ota`, `budget`: not yet reachable from the parser (work/8.md Task 155)
//! - `scenarios`, `demo`: development builds only

use crate::error::{TerminalError, TerminalResult};
use crate::{OtaAction, SyncDaemonArgs};

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

pub mod budget;
pub mod cli_output;
pub mod config;
pub mod handler_context;
pub mod init;
pub mod ota;
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

/// Runs the offline and long-running CLI tools against an agent.
///
/// Every `handle_*` method returns the command's structured [`CliOutput`];
/// the caller renders it as text or JSON.
pub struct CliHandler {
    /// The portable application core
    app_core: Arc<RwLock<AppCore>>,
    /// The agent providing the effect system
    agent: Arc<AuraAgent>,
    /// The device ID for this handler
    device_id: DeviceId,
    /// Execution context propagated through effect calls
    effect_context: EffectContext,
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
        }
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

    /// Access the AppCore
    #[must_use]
    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    /// Access the agent
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
        let agent_opt = include_agent.then_some(&*self.agent);
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

    /// Handle OTA upgrade commands
    pub async fn handle_ota(&self, action: &OtaAction) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        ota::handle_ota(&self.make_ctx(&effects, false), action).await
    }

    /// Run the foreground sync daemon
    pub async fn handle_sync_daemon(&self, args: &SyncDaemonArgs) -> TerminalResult<CliOutput> {
        let effects = self.agent.runtime().effects();
        sync::handle_daemon_mode(&self.make_ctx(&effects, true), args).await
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
