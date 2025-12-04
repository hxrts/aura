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
    OtaAction, RecoveryAction, SnapshotAction, TuiArgs,
};

#[cfg(feature = "development")]
use crate::{DemoCommands, ScenarioAction};
use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_core::identifiers::DeviceId;
use std::path::Path;
use std::sync::Arc;

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
pub mod threshold;
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
pub struct CliHandler {
    /// The Aura effect system instance
    effect_system: Arc<AuraEffectSystem>,
    /// The device ID for this handler
    device_id: DeviceId,
    /// Execution context propagated through effect calls
    effect_context: EffectContext,
}

impl CliHandler {
    /// Create a new CLI handler with the given effect system and device ID
    pub fn new(
        effect_system: Arc<AuraEffectSystem>,
        device_id: DeviceId,
        effect_context: EffectContext,
    ) -> Self {
        Self {
            effect_system,
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

    /// Handle init command through effects
    pub async fn handle_init(&self, num_devices: u32, threshold: u32, output: &Path) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        init::handle_init(&ctx, num_devices, threshold, output).await
    }

    /// Handle status command through effects
    pub async fn handle_status(&self, config_path: &Path) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        status::handle_status(&ctx, config_path).await
    }

    /// Handle node command through effects
    pub async fn handle_node(&self, port: u16, daemon: bool, config_path: &Path) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        node::handle_node(&ctx, port, daemon, config_path).await
    }

    /// Handle threshold command through effects
    pub async fn handle_threshold(&self, configs: &str, threshold: u32, mode: &str) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        threshold::handle_threshold(&ctx, configs, threshold, mode).await
    }

    /// Handle scenarios command through effects (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        scenarios::handle_scenarios(&ctx, action).await
    }

    /// Handle version command through effects
    pub async fn handle_version(&self) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        version::handle_version(&ctx).await
    }

    /// Handle snapshot maintenance commands.
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        snapshot::handle_snapshot(&ctx, action).await
    }

    /// Handle admin maintenance commands.
    pub async fn handle_admin(&self, action: &AdminAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        admin::handle_admin(&ctx, action).await
    }

    /// Handle guardian recovery commands
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        recovery::handle_recovery(&ctx, action).await
    }

    /// Handle invitation commands
    pub async fn handle_invitation(&self, action: &InvitationAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        invite::handle_invitation(&ctx, action).await
    }

    /// Handle authority management commands
    pub async fn handle_authority(&self, command: &AuthorityCommands) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        authority::handle_authority(&ctx, command).await
    }

    /// Handle context inspection commands
    pub async fn handle_context(&self, action: &ContextAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        context::handle_context(&ctx, action).await
    }

    /// Handle OTA upgrade commands
    pub async fn handle_ota(&self, action: &OtaAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        ota::handle_ota(&ctx, action).await
    }

    /// Handle AMP commands routed through the effect system.
    pub async fn handle_amp(&self, action: &AmpAction) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        amp::handle_amp(&ctx, action).await
    }

    /// Handle chat commands
    pub async fn handle_chat(&self, command: &ChatCommands) -> Result<()> {
        let ctx = HandlerContext::new(&self.effect_context, &self.effect_system, self.device_id);
        chat::handle_chat(&ctx, &self.effect_system, command).await
    }

    /// Handle demo commands (requires development feature)
    #[cfg(feature = "development")]
    pub async fn handle_demo(&self, command: &DemoCommands) -> Result<()> {
        demo::DemoHandler::handle_demo_command(command.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Demo command failed: {}", e))
    }

    /// Handle TUI commands for production terminal interface
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
