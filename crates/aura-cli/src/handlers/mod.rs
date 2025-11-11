//! CLI Command Handlers
//!
//! Effect-based implementations of CLI commands following the unified effect system.

use crate::{AdminAction, InvitationAction, OtaAction, RecoveryAction, ScenarioAction, SnapshotAction};
use anyhow::Result;
use aura_protocol::{AuraEffectSystem, ConsoleEffects};
use std::path::PathBuf;

pub mod admin;
pub mod init;
pub mod invite;
pub mod node;
pub mod ota;
pub mod recovery;
pub mod scenarios;
pub mod snapshot;
pub mod status;
pub mod test_dkd;
pub mod threshold;
pub mod version;

/// Main CLI handler that coordinates all operations through effects
pub struct CliHandler {
    effect_system: AuraEffectSystem,
}

impl CliHandler {
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self { effect_system }
    }

    /// Handle init command through effects
    pub async fn handle_init(
        &self,
        num_devices: u32,
        threshold: u32,
        output: &PathBuf,
    ) -> Result<()> {
        init::handle_init(&self.effect_system, num_devices, threshold, output).await
    }

    /// Handle status command through effects
    pub async fn handle_status(&self, config_path: &PathBuf) -> Result<()> {
        status::handle_status(&self.effect_system, config_path).await
    }

    /// Handle node command through effects
    pub async fn handle_node(&self, port: u16, daemon: bool, config_path: &PathBuf) -> Result<()> {
        node::handle_node(&self.effect_system, port, daemon, config_path).await
    }

    /// Handle threshold command through effects
    pub async fn handle_threshold(&self, configs: &str, threshold: u32, mode: &str) -> Result<()> {
        threshold::handle_threshold(&self.effect_system, configs, threshold, mode).await
    }

    /// Handle scenarios command through effects
    pub async fn handle_scenarios(&self, action: &ScenarioAction) -> Result<()> {
        scenarios::handle_scenarios(&self.effect_system, action).await
    }

    /// Handle test-dkd command through effects
    pub async fn handle_test_dkd(&self, app_id: &str, context: &str, file: &PathBuf) -> Result<()> {
        test_dkd::handle_test_dkd(&self.effect_system, app_id, context, file).await
    }

    /// Handle version command through effects
    pub async fn handle_version(&self) -> Result<()> {
        version::handle_version(&self.effect_system).await
    }

    /// Handle snapshot maintenance commands.
    pub async fn handle_snapshot(&self, action: &SnapshotAction) -> Result<()> {
        snapshot::handle_snapshot(self.effect_system.clone(), action).await
    }

    /// Handle admin maintenance commands.
    pub async fn handle_admin(&self, action: &AdminAction) -> Result<()> {
        admin::handle_admin(self.effect_system.clone(), action).await
    }

    /// Handle guardian recovery commands
    pub async fn handle_recovery(&self, action: &RecoveryAction) -> Result<()> {
        recovery::handle_recovery(&self.effect_system, action).await
    }

    /// Handle invitation commands
    pub async fn handle_invitation(&self, action: &InvitationAction) -> Result<()> {
        invite::handle_invitation(&self.effect_system, action).await
    }

    /// Handle OTA upgrade commands
    pub async fn handle_ota(&self, action: &OtaAction) -> Result<()> {
        ota::handle_ota(&self.effect_system, action).await
    }

    /// Log error message through effects
    pub async fn log_error(&self, message: &str) {
        self.effect_system.log_error(message, &[]);
    }

    /// Log info message through effects
    pub async fn log_info(&self, message: &str) {
        self.effect_system.log_info(message, &[]);
    }
}
