//! Layer 7: CLI Command Handlers
//!
//! Effect-based implementations of user-facing CLI commands for:
//! - Authority and context inspection/creation (authority, context)
//! - Account administration (admin, snapshot)
//! - Scenario management (scenarios, amp)
//! - Recovery workflows (recovery)
//! - Invitations and device onboarding (invite)
//! - OTA upgrades (ota)
//! - Status monitoring (status, version, node, threshold, init)

use crate::{
    AdminAction, AmpAction, AuthorityCommands, ChatCommands, ContextAction, DemoCommands,
    InvitationAction, OtaAction, RecoveryAction, ScenarioAction, SnapshotAction,
};
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
pub mod demo;
pub mod init;
pub mod invite;
pub mod node;
pub mod ota;
pub mod recovery;
pub mod scenarios;
pub mod snapshot;
pub mod status;
pub mod threshold;
pub mod version;

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
        init::handle_init(
            &self.effect_context,
            &self.effect_system,
            num_devices,
            threshold,
            output,
        )
        .await
    }

    /// Handle status command through effects
    pub async fn handle_status(&self, config_path: &Path) -> Result<()> {
        status::handle_status(&self.effect_context, &self.effect_system, config_path).await
    }

    /// Handle node command through effects
    pub async fn handle_node(&self, port: u16, daemon: bool, config_path: &Path) -> Result<()> {
        node::handle_node(
            &self.effect_context,
            &self.effect_system,
            port,
            daemon,
            config_path,
        )
        .await
    }

    /// Handle threshold command through effects
    pub async fn handle_threshold(&self, configs: &str, threshold: u32, mode: &str) -> Result<()> {
        threshold::handle_threshold(
            &self.effect_context,
            &self.effect_system,
            configs,
            threshold,
            mode,
        )
        .await
    }

    /// Handle scenarios command through effects
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> Result<()> {
        scenarios::handle_scenarios(&self.effect_context, self.effect_system.clone(), action).await
    }

    /// Handle version command through effects
    pub async fn handle_version(&self) -> Result<()> {
        version::handle_version(&self.effect_context, &self.effect_system).await
    }

    /// Handle snapshot maintenance commands.
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> Result<()> {
        snapshot::handle_snapshot(&self.effect_context, self.device_id, action).await
    }

    /// Handle admin maintenance commands.
    pub async fn handle_admin(&self, action: &AdminAction) -> Result<()> {
        admin::handle_admin(&self.effect_context, self.device_id, action).await
    }

    /// Handle guardian recovery commands
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> Result<()> {
        recovery::handle_recovery(&self.effect_context, &self.effect_system, action).await
    }

    /// Handle invitation commands
    pub async fn handle_invitation(&self, action: &InvitationAction) -> Result<()> {
        invite::handle_invitation(&self.effect_context, &self.effect_system, action).await
    }

    /// Handle authority management commands (placeholder)
    pub async fn handle_authority(&self, command: &AuthorityCommands) -> Result<()> {
        authority::handle_authority(&self.effect_context, &self.effect_system, command).await
    }

    /// Handle context inspection commands
    pub async fn handle_context(&self, action: &ContextAction) -> Result<()> {
        context::handle_context(&self.effect_context, action).await
    }

    /// Handle OTA upgrade commands
    pub async fn handle_ota(&self, action: &OtaAction) -> Result<()> {
        ota::handle_ota(&self.effect_context, &self.effect_system, action).await
    }

    /// Handle AMP commands (placeholder wiring).
    pub async fn handle_amp(&self, action: &AmpAction) -> Result<()> {
        amp::handle_amp(&self.effect_context, &self.effect_system, action).await
    }

    /// Handle chat commands
    pub async fn handle_chat(&self, command: &ChatCommands) -> Result<()> {
        chat::handle_chat(&self.effect_context, &self.effect_system, command).await
    }

    /// Handle demo commands
    pub async fn handle_demo(&self, command: &DemoCommands) -> Result<()> {
        demo::DemoHandler::handle_demo_command(command.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Demo command failed: {}", e))
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
