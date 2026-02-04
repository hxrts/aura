#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args
)]
//! # Regression Test: Demo Mode Mobile Device Enrollment
//!
//! This test reproduces a bug where pressing Ctrl+M in the device import modal
//! during demo mode fails with "Internal error: Failed to start devi...".
//!
//! The bug occurs when:
//! 1. User is in demo mode
//! 2. Goes to Settings > Import device enrollment code
//! 3. Presses Ctrl+M to auto-fill the Mobile device enrollment code
//! 4. Since no enrollment code exists yet, the system tries to start a new
//!    device enrollment ceremony with nickname "Mobile"
//! 5. The ceremony fails with an internal error
//!
//! This test verifies that device enrollment works correctly in demo mode.

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use aura_agent::core::config::StorageConfig;
use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::{EffectContext, SharedTransport};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::ExecutionMode;
use aura_terminal::handlers::tui::{create_account, TuiMode};
use aura_terminal::ids;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};

/// Test environment for demo mode device enrollment
struct DemoTestEnv {
    ctx: Arc<IoContext>,
    _app_core: Arc<RwLock<AppCore>>,
    #[allow(dead_code)]
    shared_transport: SharedTransport,
    #[allow(dead_code)]
    authority_id: aura_core::AuthorityId,
    #[allow(dead_code)]
    context_id: aura_core::ContextId,
    test_dir: std::path::PathBuf,
}

impl Drop for DemoTestEnv {
    fn drop(&mut self) {
        // Clean up test directory
        let _ = std::fs::remove_dir_all(&self.test_dir);
    }
}

/// Set up a demo mode test environment that mimics the TUI demo mode flow.
///
/// This creates the same setup as when running `aura --demo`:
/// - Simulation agent for the main user (Bob)
/// - Shared transport for demo peer communication
/// - IoContext configured for demo mode
async fn setup_demo_env(seed: u64) -> DemoTestEnv {
    let unique = Uuid::from_bytes([6; 16]);
    let test_dir = std::env::temp_dir().join(format!("aura-demo-mobile-test-{unique}"));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    let device_id_str = "demo:bob".to_string();
    let nickname_suggestion = "DemoUser".to_string();
    let (authority_id, context_id) =
        create_account(&test_dir, &device_id_str, &nickname_suggestion)
            .await
            .expect("Failed to create account");

    let shared_transport = SharedTransport::new();
    let effect_ctx =
        EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

    let agent_config = AgentConfig {
        device_id: ids::device_id(&device_id_str),
        storage: StorageConfig {
            base_path: test_dir.clone(),
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(authority_id)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
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
    let initialized = InitializedAppCore::new(app_core.clone())
        .await
        .expect("Failed to init signals");

    // Build IoContext with demo mode configuration
    // This is the key difference from production tests - we use TuiMode::Demo
    let ctx = IoContext::builder()
        .with_app_core(initialized)
        .with_existing_account(true)
        .with_base_path(test_dir.clone())
        .with_device_id(device_id_str)
        .with_mode(TuiMode::Demo { seed })
        .build()
        .expect("IoContext builder should succeed for demo mode");

    DemoTestEnv {
        ctx: Arc::new(ctx),
        _app_core: app_core,
        shared_transport,
        authority_id,
        context_id,
        test_dir,
    }
}

/// Regression test: Device enrollment in demo mode should succeed.
///
/// This test reproduces the bug that occurs when pressing Ctrl+M in the
/// device import modal during demo mode. The Ctrl+M handler triggers
/// `DispatchCommand::AddDevice { name: "Mobile" }` which calls
/// `start_device_enrollment()`.
///
/// Expected behavior: The device enrollment ceremony should start successfully
/// and return an enrollment code.
///
/// Actual behavior (with bug): Returns "Internal error: Failed to start devi..."
#[tokio::test]
async fn regression_demo_mode_mobile_device_enrollment_should_start() {
    let seed = 2024u64;
    let env = setup_demo_env(seed).await;

    // Refresh settings to ensure the runtime is properly initialized
    aura_app::ui::workflows::settings::refresh_settings_from_runtime(env.ctx.app_core_raw())
        .await
        .expect("refresh_settings_from_runtime should succeed with runtime");

    // This is the exact flow that happens when pressing Ctrl+M in the
    // device import modal when no enrollment code exists yet:
    // 1. The modal handler detects Ctrl+M
    // 2. Since `last_device_enrollment_code` is empty, it dispatches:
    //    `DispatchCommand::AddDevice { name: "Mobile".to_string(), invitee_authority_id: None }`
    // 3. This calls `ctx.start_device_enrollment("Mobile", None)`
    //
    // The bug causes this to fail with "Internal error: Failed to start devi..."
    let result = env.ctx.start_device_enrollment("Mobile", None).await;

    match result {
        Ok(start) => {
            // Success! The enrollment code should be non-empty
            assert!(
                !start.enrollment_code.is_empty(),
                "Enrollment code should be generated"
            );
            assert!(
                !start.ceremony_id.is_empty(),
                "Ceremony ID should be generated"
            );
            assert!(!start.device_id.is_empty(), "Device ID should be generated");
            println!("Device enrollment started successfully:");
            println!("  ceremony_id: {}", start.ceremony_id);
            println!("  device_id: {}", start.device_id);
            println!(
                "  enrollment_code: {}...",
                &start.enrollment_code[..start.enrollment_code.len().min(20)]
            );
        }
        Err(e) => {
            // This is the bug we're trying to catch!
            // The error message contains "Failed to start devi..." (truncated)
            panic!(
                "BUG REPRODUCED: Device enrollment in demo mode failed with: {e}\n\
                 This is the regression we're testing for. The enrollment ceremony \
                 should start successfully in demo mode."
            );
        }
    }
}

/// Test that multiple device enrollments can be started sequentially.
///
/// This tests a common demo scenario where the user might:
/// 1. Start enrollment for "Mobile"
/// 2. Cancel or complete it
/// 3. Start another enrollment for "Tablet"
#[tokio::test]
async fn demo_mode_sequential_device_enrollments() {
    let seed = 2025u64;
    let env = setup_demo_env(seed).await;

    aura_app::ui::workflows::settings::refresh_settings_from_runtime(env.ctx.app_core_raw())
        .await
        .expect("refresh_settings_from_runtime should succeed");

    // First enrollment
    let result1 = env.ctx.start_device_enrollment("Mobile", None).await;
    assert!(
        result1.is_ok(),
        "First enrollment should succeed: {:?}",
        result1.err()
    );

    // Small delay between ceremonies
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second enrollment (should supersede the first if not completed)
    let result2 = env.ctx.start_device_enrollment("Tablet", None).await;
    assert!(
        result2.is_ok(),
        "Second enrollment should succeed: {:?}",
        result2.err()
    );

    let start1 = result1.unwrap();
    let start2 = result2.unwrap();

    // Different enrollments should have different ceremony IDs and device IDs
    assert_ne!(
        start1.ceremony_id, start2.ceremony_id,
        "Sequential enrollments should have different ceremony IDs"
    );
    assert_ne!(
        start1.device_id, start2.device_id,
        "Sequential enrollments should have different device IDs"
    );
}

/// Test that device enrollment works immediately after account creation.
///
/// This mimics the scenario where a user:
/// 1. Starts the demo
/// 2. Creates their account
/// 3. Immediately tries to add a device via Ctrl+M
#[tokio::test]
async fn demo_mode_enrollment_immediately_after_account_creation() {
    let seed = 2026u64;
    let unique = Uuid::from_bytes([7; 16]);
    let test_dir = std::env::temp_dir().join(format!("aura-demo-immediate-test-{unique}"));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    // Create account (this is what happens in the account setup wizard)
    let device_id_str = "demo:bob".to_string();
    let nickname_suggestion = "NewUser".to_string();
    let (authority_id, context_id) =
        create_account(&test_dir, &device_id_str, &nickname_suggestion)
            .await
            .expect("Failed to create account");

    // Build agent immediately
    let shared_transport = SharedTransport::new();
    let effect_ctx =
        EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

    let agent_config = AgentConfig {
        device_id: ids::device_id(&device_id_str),
        storage: StorageConfig {
            base_path: test_dir.clone(),
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(authority_id)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport)
        .await
        .expect("Failed to build simulation agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.as_runtime_bridge())
        .expect("Failed to create AppCore with runtime");
    let app_core = Arc::new(RwLock::new(app_core));
    let initialized = InitializedAppCore::new(app_core.clone())
        .await
        .expect("Failed to init signals");

    let ctx = IoContext::builder()
        .with_app_core(initialized)
        .with_existing_account(true)
        .with_base_path(test_dir.clone())
        .with_device_id(device_id_str)
        .with_mode(TuiMode::Demo { seed })
        .build()
        .expect("IoContext builder should succeed");

    // Don't refresh settings - try enrollment immediately
    // This is a more aggressive test of the initialization sequence
    let result = ctx.start_device_enrollment("Mobile", None).await;

    // Clean up
    let _ = std::fs::remove_dir_all(&test_dir);

    assert!(
        result.is_ok(),
        "Enrollment immediately after account creation should succeed: {:?}",
        result.err()
    );
}
