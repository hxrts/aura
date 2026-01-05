#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args
)]
//! # Demo Multi-Device Enrollment Flow E2E Test
//!
//! Validates that enrolling a 3rd device does not brick existing devices.
//!
//! Flow:
//! 1) Create a 1-device account (device A)
//! 2) Enroll device B via Settings → Add device
//! 3) Enroll device C via Settings → Add device
//! 4) Assert device B receives/stores the new-epoch key package and acks the ceremony

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
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::threshold::ParticipantIdentity;
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
    let test_dir = std::env::temp_dir().join(format!("aura-multi-device-enroll-test-{unique}"));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    let device_id_str = "test-device-a".to_string();
    let nickname_suggestion = "DemoUser-A".to_string();
    let (authority_id, context_id) = create_account(&test_dir, &device_id_str, &nickname_suggestion)
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
    let device_id = ids::device_id(device_id);
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

/// TODO: This test requires full ceremony commit flow. See demo_device_enrollment_flow.rs.
/// The enrollment ceremony threshold is reached but commit message isn't sent.
#[tokio::test]
#[ignore = "requires full ceremony commit flow - threshold reached but commit message not sent"]
async fn demo_multi_device_enrollment_does_not_brick_existing_devices() {
    let env = setup_test_env().await;

    // Seed SETTINGS_SIGNAL from runtime so device list starts populated.
    aura_app::ui::workflows::settings::refresh_settings_from_runtime(env.ctx_a.app_core_raw())
        .await
        .expect("refresh_settings_from_runtime should succeed with runtime");

    // Enroll device B.
    let start_b = env
        .ctx_a
        .start_device_enrollment("Laptop")
        .await
        .expect("start_device_enrollment should succeed");

    let device_b_id = DeviceId::from_str(&start_b.device_id).expect("device_id should parse");
    let seed_b = 2025u64;
    let effect_ctx_b = EffectContext::new(
        env.authority_id,
        env.context_id,
        ExecutionMode::Simulation { seed: seed_b },
    );

    let storage_base_path = env.test_dir.join("device-b");
    std::fs::create_dir_all(&storage_base_path).expect("Failed to create device-b storage dir");
    let agent_config_b = AgentConfig {
        device_id: device_b_id,
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
        .expect("Failed to build device B agent");
    let agent_b = Arc::new(agent_b);

    let runtime_b = agent_b.clone().as_runtime_bridge();
    let invitation_b = runtime_b
        .import_invitation(&start_b.enrollment_code)
        .await
        .expect("import device enrollment code should succeed");
    runtime_b
        .accept_invitation(&invitation_b.invitation_id)
        .await
        .expect("accept device enrollment invitation should succeed");

    let status_b = aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
        env.ctx_a.app_core_raw(),
        start_b.ceremony_id.clone(),
        Duration::from_millis(50),
        |_| {},
        tokio::time::sleep,
    )
    .await
    .expect("monitor_key_rotation_ceremony should complete");

    assert!(
        status_b.is_complete,
        "device B ceremony should be committed"
    );
    assert_eq!(status_b.accepted_count, 1);
    wait_for_device(&env.app_core_a, &start_b.device_id).await;

    // Enroll device C (now there is an existing non-initiator device B).
    let start_c = env
        .ctx_a
        .start_device_enrollment("Phone")
        .await
        .expect("start_device_enrollment should succeed");

    let device_c_id = DeviceId::from_str(&start_c.device_id).expect("device_id should parse");
    let seed_c = 2026u64;
    let effect_ctx_c = EffectContext::new(
        env.authority_id,
        env.context_id,
        ExecutionMode::Simulation { seed: seed_c },
    );

    let storage_base_path = env.test_dir.join("device-c");
    std::fs::create_dir_all(&storage_base_path).expect("Failed to create device-c storage dir");
    let agent_config_c = AgentConfig {
        device_id: device_c_id,
        storage: StorageConfig {
            base_path: storage_base_path,
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    let agent_c = AgentBuilder::new()
        .with_config(agent_config_c)
        .with_authority(env.authority_id)
        .build_simulation_async_with_shared_transport(
            seed_c,
            &effect_ctx_c,
            env.shared_transport.clone(),
        )
        .await
        .expect("Failed to build device C agent");
    let agent_c = Arc::new(agent_c);

    let runtime_c = agent_c.as_runtime_bridge();
    let invitation_c = runtime_c
        .import_invitation(&start_c.enrollment_code)
        .await
        .expect("import device enrollment code should succeed");
    runtime_c
        .accept_invitation(&invitation_c.invitation_id)
        .await
        .expect("accept device enrollment invitation should succeed");

    // Drive device B runtime a bit (no background tasks) so it processes the key-package envelope.
    let drive_b = {
        let runtime_b = runtime_b.clone();
        let authority_id = env.authority_id;
        tokio::spawn(async move {
            for _ in 0..200u32 {
                let _ = runtime_b.is_peer_online(authority_id).await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    };

    let status_c = aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
        env.ctx_a.app_core_raw(),
        start_c.ceremony_id.clone(),
        Duration::from_millis(50),
        |_| {},
        tokio::time::sleep,
    )
    .await
    .expect("monitor_key_rotation_ceremony should complete");

    drive_b.abort();

    assert!(
        status_c.is_complete,
        "device C ceremony should be committed"
    );
    assert_eq!(status_c.accepted_count, 2, "B + C should ack");

    // Confirm SETTINGS_SIGNAL reflects both new devices.
    wait_for_device(&env.app_core_a, &start_b.device_id).await;
    wait_for_device(&env.app_core_a, &start_c.device_id).await;

    // Confirm device B stored its new-epoch key package (so it is not bricked).
    let effects_b = agent_b.runtime().effects();
    let pending_epoch = start_c.pending_epoch.value();
    let authority_id = env.authority_id;
    let location = SecureStorageLocation::with_sub_key(
        "participant_shares",
        format!("{authority_id}/{pending_epoch}"),
        ParticipantIdentity::device(device_b_id).storage_key(),
    );

    let stored = effects_b
        .secure_retrieve(
            &location,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .expect("device B should have stored its new-epoch key package");

    assert!(
        !stored.is_empty(),
        "stored key package bytes should be non-empty"
    );
}
