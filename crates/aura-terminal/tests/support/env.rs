//! Test environment setup and teardown helpers.
//!
//! This module provides reusable test infrastructure for aura-terminal tests.
//! There are two main test environment types:
//!
//! - [`SimpleTestEnv`]: Lightweight setup with AppCore only (no agent)
//! - [`FullTestEnv`]: Complete setup with AuraAgent for simulation tests

use async_lock::RwLock;
use std::path::PathBuf;
use std::sync::Arc;

use aura_agent::core::{AgentBuilder, AgentConfig, AuraAgent};
use aura_agent::EffectContext;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::ExecutionMode;
use aura_core::identifiers::AuthorityId;
use aura_terminal::handlers::tui::{create_account, TuiMode};
use aura_terminal::ids;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};

use super::unique_test_dir;

// ============================================================================
// Simple Test Environment (no agent)
// ============================================================================

/// Lightweight test environment without an agent.
///
/// Use this for tests that only need IoContext and AppCore,
/// such as callback wiring tests and basic integration tests.
pub struct SimpleTestEnv {
    pub ctx: Arc<IoContext>,
    pub app_core: Arc<RwLock<AppCore>>,
    pub test_dir: PathBuf,
}

impl SimpleTestEnv {
    /// Create a new simple test environment.
    ///
    /// This sets up:
    /// - A unique test directory
    /// - An AppCore with default config
    /// - An IoContext ready for testing
    /// - A test account
    pub async fn new(name: &str) -> Self {
        let test_dir = unique_test_dir(&format!("aura-test-{name}"));

        let app_core = AppCore::new(AppConfig::default()).expect("Failed to create AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let initialized_app_core = InitializedAppCore::new(app_core.clone())
            .await
            .expect("Failed to init signals");

        let ctx = IoContext::builder()
            .with_app_core(initialized_app_core)
            .with_existing_account(false)
            .with_base_path(test_dir.clone())
            .with_device_id(format!("test-device-{name}"))
            .with_mode(TuiMode::Production)
            .build()
            .expect("IoContext builder should succeed for tests");

        // Create account for testing
        ctx.create_account(&format!("TestUser-{name}"))
            .await
            .expect("Failed to create account");

        Self {
            ctx: Arc::new(ctx),
            app_core,
            test_dir,
        }
    }

    /// Clean up the test directory.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }
}

impl Drop for SimpleTestEnv {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ============================================================================
// Full Test Environment (with agent)
// ============================================================================

/// Complete test environment with simulation agent.
///
/// Use this for tests that need full agent capabilities,
/// such as demo flows and end-to-end protocol tests.
pub struct FullTestEnv {
    pub ctx: Arc<IoContext>,
    pub app_core: Arc<RwLock<AppCore>>,
    pub agent: Arc<AuraAgent>,
    pub authority_id: AuthorityId,
    pub test_dir: PathBuf,
}

/// Configuration for creating a full test environment.
pub struct FullTestEnvConfig {
    pub name: String,
    pub seed: u64,
    pub display_name: Option<String>,
}

impl Default for FullTestEnvConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            seed: 2024,
            display_name: None,
        }
    }
}

impl FullTestEnv {
    /// Create a new full test environment with default seed.
    pub async fn new(name: &str) -> Self {
        Self::with_config(FullTestEnvConfig {
            name: name.to_string(),
            ..Default::default()
        })
        .await
    }

    /// Create a new full test environment with custom configuration.
    pub async fn with_config(config: FullTestEnvConfig) -> Self {
        let FullTestEnvConfig {
            name,
            seed,
            display_name,
        } = config;
        let test_dir = unique_test_dir(&format!("aura-full-test-{name}"));

        let device_id_str = format!("test-device-{name}");
        let display_name = display_name.unwrap_or_else(|| format!("TestUser-{name}"));

        let (authority_id, context_id) = create_account(&test_dir, &device_id_str, &display_name)
            .await
            .expect("Failed to create account");

        let agent_config = AgentConfig {
            device_id: ids::device_id(&device_id_str),
            storage: aura_agent::core::config::StorageConfig {
                base_path: test_dir.clone(),
                ..Default::default()
            },
            ..Default::default()
        };

        let effect_ctx =
            EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

        let agent = AgentBuilder::new()
            .with_config(agent_config)
            .with_authority(authority_id)
            .build_simulation_async(config.seed, &effect_ctx)
            .await
            .expect("Failed to build simulation agent");
        let agent = Arc::new(agent);

        let app_config = AppConfig {
            data_dir: test_dir.to_string_lossy().to_string(),
            ..AppConfig::default()
        };
        let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
            .expect("Failed to create AppCore with runtime");
        let app_core = Arc::new(RwLock::new(app_core));
        let initialized_app_core = InitializedAppCore::new(app_core.clone())
            .await
            .expect("Failed to init signals");

        let ctx = IoContext::builder()
            .with_app_core(initialized_app_core)
            .with_existing_account(true)
            .with_base_path(test_dir.clone())
            .with_device_id(device_id_str)
            .with_mode(TuiMode::Production)
            .build()
            .expect("IoContext builder should succeed for tests");

        Self {
            ctx: Arc::new(ctx),
            app_core,
            agent,
            authority_id,
            test_dir,
        }
    }

    /// Clean up the test directory.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }
}

impl Drop for FullTestEnv {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ============================================================================
// Legacy Compatibility Functions
// ============================================================================

/// Create a simple test environment (legacy compatibility).
///
/// Returns (ctx, app_core) for tests that use this pattern.
pub async fn setup_test_env(name: &str) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
    let env = SimpleTestEnv::new(name).await;
    // Note: We can't return env directly due to Drop, so we clone the Arcs
    // and leak the cleanup. Tests using this should call cleanup_test_dir.
    (env.ctx.clone(), env.app_core.clone())
}

/// Clean up a test directory (legacy compatibility).
pub fn cleanup_test_dir(name: &str) {
    let test_dir = std::env::temp_dir().join(format!("aura-test-{name}"));
    let _ = std::fs::remove_dir_all(&test_dir);
}
