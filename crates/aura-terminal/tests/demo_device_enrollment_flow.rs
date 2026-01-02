#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args
)]
//! # Demo Device Enrollment Flow E2E Test
//!
//! Validates that Settings → Add device starts a real device enrollment ceremony,
//! and that accepting the enrollment code on a second simulated device commits
//! the ceremony and updates SETTINGS_SIGNAL device list.

use async_lock::RwLock;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use aura_agent::core::config::StorageConfig;
use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::{EffectContext, SharedTransport};
use aura_app::signal_defs::SETTINGS_SIGNAL;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::DeviceId;
use aura_terminal::handlers::tui::create_account;
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::ids;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};
use uuid::Uuid;

struct TestEnv {
    ctx_a: Arc<IoContext>,
    app_core_a: Arc<RwLock<AppCore>>,
    shared_transport: SharedTransport,
    authority_id: aura_core::AuthorityId,
    context_id: aura_core::ContextId,
    test_dir: std::path::PathBuf,
}

async fn setup_test_env() -> TestEnv {
    let unique = Uuid::new_v4();
    let test_dir = std::env::temp_dir().join(format!("aura-device-enroll-test-{unique}"));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    let device_id_str = "test-device-a".to_string();
    let display_name = "DemoUser-A".to_string();
    let (authority_id, context_id) = create_account(&test_dir, &device_id_str, &display_name)
        .await
        .expect("Failed to create account");

    let shared_transport = SharedTransport::new();
    let seed = 2024u64;
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

    let agent_a = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(authority_id)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
        .await
        .expect("Failed to build initiator agent");
    let agent_a = Arc::new(agent_a);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core_a = AppCore::with_runtime(app_config, agent_a.clone().as_runtime_bridge())
        .expect("Failed to create AppCore with runtime");
    let app_core_a = Arc::new(RwLock::new(app_core_a));
    let initialized = InitializedAppCore::new(app_core_a.clone())
        .await
        .expect("Failed to init signals");

    let ctx_a = IoContext::builder()
        .with_app_core(initialized)
        .with_existing_account(true)
        .with_base_path(test_dir.clone())
        .with_device_id(device_id_str)
        .with_mode(TuiMode::Production)
        .build()
        .expect("IoContext builder should succeed for tests");

    TestEnv {
        ctx_a: Arc::new(ctx_a),
        app_core_a,
        shared_transport,
        authority_id,
        context_id,
        test_dir,
    }
}

async fn wait_for_device(app_core: &Arc<RwLock<AppCore>>, device_id: &str) {
    let device_id = device_id.parse::<DeviceId>().expect("valid device id");
    let start = tokio::time::Instant::now();
    loop {
        let state = {
            let core = app_core.read().await;
            core.read(&*SETTINGS_SIGNAL)
                .await
                .expect("Failed to read SETTINGS_SIGNAL")
        };

        if state.devices.iter().any(|d| d.id == device_id) {
            return;
        }

        if start.elapsed() > Duration::from_secs(3) {
            let device_count = state.devices.len();
            panic!("Timed out waiting for device {device_id} ({device_count} devices present)");
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn demo_device_enrollment_flow_commits_and_updates_settings() {
    let env = setup_test_env().await;

    // Seed SETTINGS_SIGNAL from runtime so device list starts populated.
    aura_app::ui::workflows::settings::refresh_settings_from_runtime(env.ctx_a.app_core_raw())
        .await
        .expect("refresh_settings_from_runtime should succeed with runtime");

    // Start the ceremony via the same path used by Settings → Add device.
    let start = env
        .ctx_a
        .start_device_enrollment("Laptop")
        .await
        .expect("start_device_enrollment should succeed");

    // Build the invited device agent using the device_id from the ceremony start.
    let new_device_id = DeviceId::from_str(&start.device_id).expect("device_id should parse");
    let seed_b = 2025u64;
    let effect_ctx_b = EffectContext::new(
        env.authority_id,
        env.context_id,
        ExecutionMode::Simulation { seed: seed_b },
    );

    let storage_base_path = env.test_dir.join("device-b");
    std::fs::create_dir_all(&storage_base_path).expect("Failed to create device-b storage dir");
    let agent_config_b = AgentConfig {
        device_id: new_device_id,
        storage: StorageConfig {
            base_path: storage_base_path,
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    let agent_b = AgentBuilder::new()
        .with_config(agent_config_b)
        .with_authority(env.authority_id)
        .build_simulation_async_with_shared_transport(
            seed_b,
            &effect_ctx_b,
            env.shared_transport.clone(),
        )
        .await
        .expect("Failed to build invited device agent");
    let agent_b = Arc::new(agent_b);

    // Import and accept the enrollment code on the invited device.
    let runtime_b = agent_b.as_runtime_bridge();
    let invitation = runtime_b
        .import_invitation(&start.enrollment_code)
        .await
        .expect("import device enrollment code should succeed");
    runtime_b
        .accept_invitation(&invitation.invitation_id)
        .await
        .expect("accept device enrollment invitation should succeed");

    // Wait for the initiator to observe completion.
    let status = aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
        env.ctx_a.app_core_raw(),
        start.ceremony_id.clone(),
        Duration::from_millis(50),
        |_| {},
        tokio::time::sleep,
    )
    .await
    .expect("monitor_key_rotation_ceremony should complete");

    assert!(status.is_complete, "ceremony should be committed");
    assert_eq!(
        status.kind,
        aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
    );
    assert_eq!(status.accepted_count, 1);

    // Ensure SETTINGS_SIGNAL device list reflects the committed device membership.
    wait_for_device(&env.app_core_a, &start.device_id).await;
}
