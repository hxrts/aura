//! # CLI Test Harness
//!
//! Runs typed CLI requests in-process against a testing runtime, through the
//! same command model (`crate::command`) as the `aura` binary and `aura rpc`.
//!
//! ```rust,ignore
//! use aura_terminal::command::Request;
//! use aura_terminal::testing::cli::CliTestHarness;
//!
//! let harness = CliTestHarness::new().await?;
//! let response = harness.exec(Request::Status).await?;
//! ```

use crate::command::{execute, render, CommandContext, CommandError, Request, Response};
use crate::ids;

use async_lock::RwLock;
use aura_agent::{AgentBuilder, EffectContext};
use aura_app::ui::prelude::*;
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::DeviceId;
use std::sync::Arc;

/// In-process runner for typed CLI requests.
pub struct CliTestHarness {
    ctx: CommandContext,
}

impl CliTestHarness {
    /// A harness on a fresh testing runtime.
    pub async fn new() -> anyhow::Result<Self> {
        Self::with_device_id(DeviceId::from_bytes([0u8; 32])).await
    }

    /// A harness on a fresh testing runtime for `device_id`.
    pub async fn with_device_id(device_id: DeviceId) -> anyhow::Result<Self> {
        let authority_id = ids::authority_id(&format!("cli:test-authority:{device_id}"));
        let context_id = ids::context_id(&format!("cli:test-context:{device_id}"));
        let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
        let agent = Arc::new(
            AgentBuilder::new()
                .with_authority(authority_id)
                .build_testing_async(&effect_context)
                .await?,
        );
        let app_core = Arc::new(RwLock::new(AppCore::with_runtime(
            AppConfig::default(),
            agent.clone().as_runtime_bridge(),
        )?));
        AppCore::init_signals_with_hooks(&app_core).await?;
        Ok(Self {
            ctx: CommandContext::new(app_core, agent.runtime().effects(), authority_id),
        })
    }

    /// The command context requests run against.
    #[must_use]
    pub fn context(&self) -> &CommandContext {
        &self.ctx
    }

    /// Run one request.
    pub async fn exec(&self, request: Request) -> Result<Response, CommandError> {
        execute(&self.ctx, request).await
    }

    /// Run one request and return its text rendering.
    pub async fn exec_text(&self, request: Request) -> Result<String, CommandError> {
        Ok(render(&self.exec(request).await?).stdout_lines().join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_runtime_has_no_channels() {
        let harness = CliTestHarness::new().await.unwrap();
        assert_eq!(
            harness.exec(Request::ChatList).await.unwrap(),
            Response::Channels(Vec::new())
        );
    }

    #[tokio::test]
    async fn fresh_runtime_has_no_pending_invitations() {
        let harness = CliTestHarness::new().await.unwrap();
        assert_eq!(
            harness.exec(Request::InviteList).await.unwrap(),
            Response::Invitations(Vec::new())
        );
    }

    #[tokio::test]
    async fn unknown_channels_are_not_found() {
        let harness = CliTestHarness::new().await.unwrap();
        let error = harness
            .exec(Request::ChatHistory {
                channel: "no-such-channel".into(),
                limit: None,
                sender: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::command::ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn bad_identifiers_are_invalid_input() {
        let harness = CliTestHarness::new().await.unwrap();
        let error = harness
            .exec(Request::AmpInspect {
                context: "bad".into(),
                channel: "bad".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::command::ErrorCode::InvalidInput);
    }
}
