//! TUI moderation dispatch smoke tests

#![allow(clippy::expect_used)]

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::{AppConfig, AppCore};
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};
use aura_terminal::tui::effects::EffectCommand;
use tempfile::TempDir;

async fn setup_ctx(name: &str) -> (IoContext, TempDir) {
    let app_core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
    let app_core = Arc::new(RwLock::new(app_core));
    let app_core = InitializedAppCore::new(app_core)
        .await
        .expect("Failed to init signals");

    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let ctx = IoContext::builder()
        .with_app_core(app_core)
        .with_existing_account(false)
        .with_base_path(dir.path().to_path_buf())
        .with_device_id(format!("test-device-{}", name))
        .with_mode(TuiMode::Production)
        .build()
        .expect("IoContext builder should succeed for tests");

    ctx.create_account(&format!("TestUser-{}", name))
        .await
        .expect("Failed to create account");

    (ctx, dir)
}

#[tokio::test]
async fn moderation_commands_are_handled() {
    let (ctx, _dir) = setup_ctx("moderation").await;
    let channel = "block:home".to_string();

    let commands = vec![
        EffectCommand::KickUser {
            channel: channel.clone(),
            target: "user1".to_string(),
            reason: None,
        },
        EffectCommand::BanUser {
            target: "user1".to_string(),
            reason: Some("spam".to_string()),
        },
        EffectCommand::UnbanUser {
            target: "user1".to_string(),
        },
        EffectCommand::MuteUser {
            target: "user1".to_string(),
            duration_secs: Some(30),
        },
        EffectCommand::UnmuteUser {
            target: "user1".to_string(),
        },
        EffectCommand::PinMessage {
            message_id: "msg-1".to_string(),
        },
        EffectCommand::UnpinMessage {
            message_id: "msg-1".to_string(),
        },
    ];

    for cmd in commands {
        if let Err(msg) = ctx.dispatch(cmd).await {
            assert!(
                !msg.contains("Unknown command"),
                "unexpected unknown command error: {msg}"
            );
        }
    }
}
