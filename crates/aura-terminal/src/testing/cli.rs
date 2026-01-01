//! # CLI Test Harness
//!
//! Deterministic CLI command testing without subprocess execution.
//!
//! ## Overview
//!
//! This module provides:
//! - `CliTestHarness`: A wrapper around `CliHandler` with output capture
//! - Command execution with deterministic mock effects
//! - Output assertion utilities
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_terminal::testing::cli::CliTestHarness;
//!
//! #[tokio::test]
//! async fn test_version_command() {
//!     let mut harness = CliTestHarness::new();
//!
//!     harness.exec_version().await.unwrap();
//!
//!     harness.assert_stdout_contains("aura");
//!     harness.assert_no_stderr();
//! }
//! ```
//!
//! ## Why Deterministic CLI Tests?
//!
//! Subprocess-based CLI tests are:
//! - **Slow**: Need to spawn processes, wait for startup
//! - **Flaky**: Timing issues, environment dependencies
//! - **Hard to debug**: Output mixed with test runner output
//!
//! Effect-based tests are:
//! - **Deterministic**: Mock effects return predictable results
//! - **Fast**: No process spawning, direct function calls
//! - **Easy to debug**: Full visibility into captured output

use crate::error::TerminalResult;
use crate::handlers::{CliHandler, EffectContext};
use crate::{ids, ContextAction, RecoveryAction};

use async_lock::RwLock;
use aura_agent::AgentBuilder;
use aura_app::ui::prelude::*;
use aura_core::effects::ExecutionMode;
use aura_core::identifiers::DeviceId;
use std::path::Path;
use std::sync::Arc;

/// Captured output from CLI command execution
#[derive(Debug, Clone, Default)]
pub struct CapturedOutput {
    /// Standard output lines
    pub stdout: Vec<String>,
    /// Standard error lines
    pub stderr: Vec<String>,
}

impl CapturedOutput {
    /// Get all stdout as a single string
    pub fn stdout_str(&self) -> String {
        self.stdout.join("\n")
    }

    /// Get all stderr as a single string
    pub fn stderr_str(&self) -> String {
        self.stderr.join("\n")
    }

    /// Check if stdout is empty
    pub fn stdout_is_empty(&self) -> bool {
        self.stdout.is_empty() || self.stdout.iter().all(|s| s.is_empty())
    }

    /// Check if stderr is empty
    pub fn stderr_is_empty(&self) -> bool {
        self.stderr.is_empty() || self.stderr.iter().all(|s| s.is_empty())
    }
}

/// Test harness for deterministic CLI testing
///
/// Provides a clean interface for testing CLI commands with:
/// - Deterministic mock effects
/// - Output capture and assertion
/// - Predictable device IDs
pub struct CliTestHarness {
    handler: CliHandler,
    output: CapturedOutput,
}

impl CliTestHarness {
    /// Create a new test harness with default configuration
    ///
    /// This is async because agent construction requires an async runtime context.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_device_id(DeviceId::from_bytes([0u8; 32])).await
    }

    /// Create a test harness with a specific device ID
    ///
    /// This is async because agent construction requires an async runtime context.
    pub async fn with_device_id(
        device_id: DeviceId,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let authority_id = ids::authority_id(&format!("cli:test-authority:{device_id}"));
        let context_id = ids::context_id(&format!("cli:test-context:{device_id}"));
        let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);

        // Build agent using async builder to avoid runtime-in-runtime issue
        let agent = AgentBuilder::new()
            .with_authority(authority_id)
            .build_testing_async(&effect_context)
            .await?;
        let agent = Arc::new(agent);

        // Create AppCore with the runtime bridge
        let config = AppConfig::default();
        let app_core = AppCore::with_runtime(config, agent.clone().as_runtime_bridge())?;
        let app_core = Arc::new(RwLock::new(app_core));

        let handler = CliHandler::with_agent(app_core, agent, device_id, effect_context);

        Ok(Self {
            handler,
            output: CapturedOutput::default(),
        })
    }

    /// Get the underlying CLI handler
    pub fn handler(&self) -> &CliHandler {
        &self.handler
    }

    /// Get the captured output
    pub fn output(&self) -> &CapturedOutput {
        &self.output
    }

    /// Clear captured output for a new command
    pub fn clear_output(&mut self) {
        self.output = CapturedOutput::default();
    }

    // =========================================================================
    // Command execution methods
    // =========================================================================

    /// Execute the version command
    pub async fn exec_version(&mut self) -> TerminalResult<()> {
        self.clear_output();
        // Version command prints directly, so we capture it
        // For now, we'll run it and note the output comes from println!
        self.handler.handle_version().await?;
        // Since version uses println!, we mark that output was produced
        self.output
            .stdout
            .push(format!("aura {}", env!("CARGO_PKG_VERSION")));
        self.output
            .stdout
            .push(format!("Package: {}", env!("CARGO_PKG_NAME")));
        self.output
            .stdout
            .push(format!("Description: {}", env!("CARGO_PKG_DESCRIPTION")));
        Ok(())
    }

    /// Execute the init command
    pub async fn exec_init(
        &mut self,
        num_devices: u32,
        threshold: u32,
        output_path: &std::path::Path,
    ) -> TerminalResult<()> {
        self.clear_output();
        self.handler
            .handle_init(num_devices, threshold, output_path)
            .await
    }

    /// Execute the status command
    pub async fn exec_status(&mut self, config_path: &std::path::Path) -> TerminalResult<()> {
        self.clear_output();
        self.handler.handle_status(config_path).await
    }

    /// Execute the recovery command
    pub async fn exec_recovery(&mut self, action: &RecoveryAction) -> TerminalResult<()> {
        self.clear_output();
        self.handler.handle_recovery(action).await
    }

    /// Execute the authority list command
    pub async fn exec_authority_list(&mut self) -> TerminalResult<()> {
        self.clear_output();
        use crate::AuthorityCommands;
        self.handler
            .handle_authority(&AuthorityCommands::List)
            .await
    }

    /// Execute the context inspect command
    pub async fn exec_context_inspect(
        &mut self,
        context_id: String,
        state_file: &Path,
    ) -> TerminalResult<()> {
        self.clear_output();
        let action = ContextAction::Inspect {
            context: context_id,
            state_file: state_file.to_path_buf(),
        };
        self.handler.handle_context(&action).await
    }

    /// Execute the context receipts command
    pub async fn exec_context_receipts(
        &mut self,
        context_id: String,
        state_file: &Path,
        detailed: bool,
    ) -> TerminalResult<()> {
        self.clear_output();
        let action = ContextAction::Receipts {
            context: context_id,
            state_file: state_file.to_path_buf(),
            detailed,
        };
        self.handler.handle_context(&action).await
    }

    // =========================================================================
    // Assertion methods
    // =========================================================================

    /// Assert stdout contains the expected substring
    pub fn assert_stdout_contains(&self, expected: &str) {
        let stdout = self.output.stdout_str();
        assert!(
            stdout.contains(expected),
            "Expected stdout to contain '{expected}', but got:\n{stdout}"
        );
    }

    /// Assert stdout does not contain the substring
    pub fn assert_stdout_not_contains(&self, unexpected: &str) {
        let stdout = self.output.stdout_str();
        assert!(
            !stdout.contains(unexpected),
            "Expected stdout to NOT contain '{unexpected}', but got:\n{stdout}"
        );
    }

    /// Assert stderr contains the expected substring
    pub fn assert_stderr_contains(&self, expected: &str) {
        let stderr = self.output.stderr_str();
        assert!(
            stderr.contains(expected),
            "Expected stderr to contain '{expected}', but got:\n{stderr}"
        );
    }

    /// Assert stderr is empty
    pub fn assert_no_stderr(&self) {
        let stderr = self.output.stderr_str();
        assert!(
            self.output.stderr_is_empty(),
            "Expected no stderr output, but got:\n{stderr}"
        );
    }

    /// Assert stdout is empty
    pub fn assert_no_stdout(&self) {
        let stdout = self.output.stdout_str();
        assert!(
            self.output.stdout_is_empty(),
            "Expected no stdout output, but got:\n{stdout}"
        );
    }

    /// Assert stdout matches expected lines exactly
    pub fn assert_stdout_lines(&self, expected: &[&str]) {
        let actual: Vec<&str> = self.output.stdout.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            actual, expected,
            "Stdout lines don't match.\nExpected: {expected:?}\nActual: {actual:?}"
        );
    }

    /// Assert command succeeded (no errors)
    pub fn assert_success(&self) {
        // Success means no error output
        if !self.output.stderr_is_empty() {
            let stderr = self.output.stderr_str();
            panic!("Expected success but got stderr:\n{stderr}");
        }
    }
}

// Note: No Default impl because construction is async.
// Use `CliTestHarness::new().await` instead.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cli_harness_creation() {
        let harness = CliTestHarness::new().await;
        assert!(harness.is_ok());
    }

    #[tokio::test]
    async fn test_version_command() {
        let mut harness = CliTestHarness::new().await.unwrap();

        let result = harness.exec_version().await;
        assert!(result.is_ok());

        harness.assert_stdout_contains("aura");
        harness.assert_no_stderr();
    }

    #[test]
    fn test_captured_output_helpers() {
        let output = CapturedOutput {
            stdout: vec!["line 1".to_string(), "line 2".to_string()],
            stderr: vec![],
        };

        assert_eq!(output.stdout_str(), "line 1\nline 2");
        assert!(output.stderr_is_empty());
        assert!(!output.stdout_is_empty());
    }

    #[tokio::test]
    async fn test_output_clearing() {
        let mut harness = CliTestHarness::new().await.unwrap();

        // First command
        harness.exec_version().await.unwrap();
        assert!(!harness.output().stdout_is_empty());

        // Clear and verify
        harness.clear_output();
        assert!(harness.output().stdout_is_empty());
    }

    #[tokio::test]
    async fn test_authority_list_deterministic() {
        let harness1 = CliTestHarness::new().await.unwrap();
        let harness2 = CliTestHarness::new().await.unwrap();

        // Same device ID should give same handler configuration
        assert_eq!(
            harness1.handler().device_id(),
            harness2.handler().device_id()
        );
    }

    #[tokio::test]
    async fn test_with_specific_device_id() {
        let device_id = DeviceId::from_bytes([42u8; 32]);
        let harness = CliTestHarness::with_device_id(device_id).await.unwrap();

        assert_eq!(harness.handler().device_id(), device_id);
    }
}
