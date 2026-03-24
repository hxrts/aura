//! Test environment setup and teardown helpers.
//!
//! This module provides reusable test infrastructure for aura-terminal tests.
//! There are two main test environment types:
//!
//! - [`SimpleTestEnv`]: Lightweight setup with AppCore only (no agent)
//! - [`FullTestEnv`]: Complete setup with AuraAgent for simulation tests

#![allow(dead_code)]

use async_lock::RwLock;
use std::path::PathBuf;
use std::sync::Arc;

use super::unique_test_dir;
use aura_agent::core::{AgentBuilder, AgentConfig, AuraAgent};
use aura_agent::EffectContext;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::AuthorityId;
use aura_terminal::handlers::tui::{create_account, TuiMode};
use aura_terminal::ids;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};
use aura_testkit::MockRuntimeBridge;

enum TestRuntimeKind {
    AppCoreOnly,
    MockRuntime {
        authority: Option<AuthorityId>,
    },
}

/// Explicit builder for IoContext-based test environments.
///
/// This keeps runtime choice, account creation, device identity, and TUI mode
/// visible at the call site rather than hiding them behind test-only defaults.
pub struct IoContextTestEnvBuilder {
    name: String,
    directory_prefix: String,
    base_path: Option<PathBuf>,
    runtime: TestRuntimeKind,
    existing_account: bool,
    create_account_nickname: Option<String>,
    device_id: Option<String>,
    mode: TuiMode,
}

/// Built IoContext/AppCore pair with owned temp-directory cleanup.
pub struct BuiltIoContextTestEnv {
    pub ctx: Arc<IoContext>,
    pub app_core: Arc<RwLock<AppCore>>,
    pub test_dir: PathBuf,
}

impl IoContextTestEnvBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            directory_prefix: "aura-test".to_string(),
            device_id: Some(format!("test-device-{name}")),
            name,
            base_path: None,
            runtime: TestRuntimeKind::AppCoreOnly,
            existing_account: false,
            create_account_nickname: None,
            mode: TuiMode::Production,
        }
    }

    pub fn with_directory_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.directory_prefix = prefix.into();
        self
    }

    pub fn with_base_path(mut self, base_path: PathBuf) -> Self {
        self.base_path = Some(base_path);
        self
    }

    pub fn with_mock_runtime(mut self) -> Self {
        self.runtime = TestRuntimeKind::MockRuntime { authority: None };
        self
    }

    pub fn with_mock_runtime_authority(mut self, authority: AuthorityId) -> Self {
        self.runtime = TestRuntimeKind::MockRuntime {
            authority: Some(authority),
        };
        self
    }

    pub fn with_existing_account(mut self, existing_account: bool) -> Self {
        self.existing_account = existing_account;
        self
    }

    pub fn with_mode(mut self, mode: TuiMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    pub fn create_account_as(mut self, nickname: impl Into<String>) -> Self {
        self.create_account_nickname = Some(nickname.into());
        self
    }

    pub async fn build(self) -> BuiltIoContextTestEnv {
        let test_dir = if let Some(base_path) = self.base_path {
            let _ = std::fs::remove_dir_all(&base_path);
            std::fs::create_dir_all(&base_path).expect("Failed to create test dir");
            base_path
        } else {
            unique_test_dir(&format!("{}-{}", self.directory_prefix, self.name))
        };

        let app_core = match self.runtime {
            TestRuntimeKind::AppCoreOnly => {
                AppCore::new(AppConfig::default()).expect("Failed to create AppCore")
            }
            TestRuntimeKind::MockRuntime { authority } => {
                let runtime = authority
                    .map(MockRuntimeBridge::with_authority)
                    .unwrap_or_else(MockRuntimeBridge::new);
                AppCore::with_runtime(AppConfig::default(), Arc::new(runtime))
                    .expect("Failed to create AppCore")
            }
        };

        let app_core = Arc::new(RwLock::new(app_core));
        let initialized_app_core = InitializedAppCore::new(app_core.clone())
            .await
            .expect("Failed to init signals");

        let ctx = IoContext::builder()
            .with_app_core(initialized_app_core)
            .with_existing_account(self.existing_account)
            .with_base_path(test_dir.clone())
            .with_device_id(
                self.device_id
                    .unwrap_or_else(|| format!("test-device-{}", self.name)),
            )
            .with_mode(self.mode)
            .build()
            .expect("IoContext builder should succeed for tests");
        let ctx = Arc::new(ctx);

        if let Some(nickname) = self.create_account_nickname {
            ctx.create_account(&nickname)
                .await
                .expect("Failed to create account");
        }

        if let Some(runtime_authority) = {
            let core = app_core.read().await;
            core.runtime().map(|runtime| runtime.authority_id())
        } {
            app_core.write().await.set_authority(runtime_authority);
            aura_app::ui::workflows::settings::refresh_settings_from_runtime(&app_core)
                .await
                .expect("Failed to refresh settings from runtime");
        }

        BuiltIoContextTestEnv {
            ctx,
            app_core,
            test_dir,
        }
    }
}

impl BuiltIoContextTestEnv {
    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }
}

impl Drop for BuiltIoContextTestEnv {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub async fn read_account_config(
    test_dir: &std::path::Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    use std::io;

    use aura_core::effects::StorageCoreEffects;
    use aura_effects::{
        EncryptedStorage, EncryptedStorageConfig, FilesystemStorageHandler, RealCryptoHandler,
        RealSecureStorageHandler,
    };

    let storage = EncryptedStorage::new(
        FilesystemStorageHandler::from_path(test_dir.to_path_buf()),
        Arc::new(RealCryptoHandler::new()),
        Arc::new(RealSecureStorageHandler::with_base_path(
            test_dir.to_path_buf(),
        )),
        EncryptedStorageConfig::default(),
    );
    let bytes = storage
        .retrieve("account.json")
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "account.json missing from storage"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub async fn read_account_authority_id(
    test_dir: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use std::io;

    let config = read_account_config(test_dir).await?;
    Ok(config["authority_id"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "authority_id should be a string"))?
        .to_string())
}

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
        let built = IoContextTestEnvBuilder::new(name)
            .create_account_as(format!("TestUser-{name}"))
            .build()
            .await;

        Self {
            ctx: built.ctx.clone(),
            app_core: built.app_core.clone(),
            test_dir: built.test_dir.clone(),
        }
    }

    /// Clean up the test directory.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }

    /// Clone the commonly used `(ctx, app_core)` pair.
    pub fn clones(&self) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
        (self.ctx.clone(), self.app_core.clone())
    }
}

impl Drop for SimpleTestEnv {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Lightweight test environment backed by `MockRuntimeBridge`.
///
/// Use this for callback wiring and integration-effect tests that need a runtime
/// without booting a full simulation agent.
pub struct MockRuntimeTestEnv {
    pub ctx: Arc<IoContext>,
    pub app_core: Arc<RwLock<AppCore>>,
    pub test_dir: PathBuf,
}

impl MockRuntimeTestEnv {
    /// Create a new mock-runtime test environment under a deterministic prefix.
    pub async fn new(prefix: &str, name: &str) -> Self {
        let test_dir = std::env::temp_dir().join(format!("{prefix}-{name}"));
        let built = IoContextTestEnvBuilder::new(name)
            .with_base_path(test_dir)
            .with_mock_runtime()
            .create_account_as(format!("TestUser-{name}"))
            .build()
            .await;

        Self {
            ctx: built.ctx.clone(),
            app_core: built.app_core.clone(),
            test_dir: built.test_dir.clone(),
        }
    }

    /// Clean up the test directory.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }

    /// Clone the commonly used `(ctx, app_core)` pair.
    pub fn clones(&self) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
        (self.ctx.clone(), self.app_core.clone())
    }
}

impl Drop for MockRuntimeTestEnv {
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
    pub nickname_suggestion: Option<String>,
}

impl Default for FullTestEnvConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            seed: 2024,
            nickname_suggestion: None,
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
            nickname_suggestion,
        } = config;
        let test_dir = unique_test_dir(&format!("aura-full-test-{name}"));

        let device_id_str = format!("test-device-{name}");
        let nickname_suggestion = nickname_suggestion.unwrap_or_else(|| format!("TestUser-{name}"));

        let (authority_id, context_id) = create_account(&test_dir, &nickname_suggestion)
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
// Test Environment Convenience Functions
// ============================================================================

/// Create a simple test environment.
///
/// Returns (ctx, app_core) for tests that use this pattern.
pub async fn setup_test_env(name: &str) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
    let env = SimpleTestEnv::new(name).await;
    // Note: We can't return env directly due to Drop, so we clone the Arcs
    // and leak the cleanup. Tests using this should call cleanup_test_dir.
    (env.ctx.clone(), env.app_core.clone())
}

/// Clean up a test directory.
pub fn cleanup_test_dir(name: &str) {
    let test_dir = std::env::temp_dir().join(format!("aura-test-{name}"));
    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Create a simple AppCore-only test environment under a deterministic prefix.
pub async fn setup_test_env_with_prefix(
    prefix: &str,
    name: &str,
) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
    let test_dir = std::env::temp_dir().join(format!("{prefix}-{name}"));
    let built = IoContextTestEnvBuilder::new(name)
        .with_base_path(test_dir)
        .with_mock_runtime()
        .create_account_as(format!("TestUser-{name}"))
        .build()
        .await;

    (built.ctx.clone(), built.app_core.clone())
}

/// Clean up a deterministic test directory created with a custom prefix.
pub fn cleanup_test_dir_with_prefix(prefix: &str, name: &str) {
    let test_dir = std::env::temp_dir().join(format!("{prefix}-{name}"));
    let _ = std::fs::remove_dir_all(&test_dir);
}
