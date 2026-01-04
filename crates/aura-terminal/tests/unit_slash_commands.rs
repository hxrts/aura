//! TUI command toast wiring tests

#![allow(clippy::expect_used)]

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::{AppConfig, AppCore};
use aura_app::ui::workflows::messaging::create_channel;
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::tui::callbacks::ChatCallbacks;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};
use aura_terminal::tui::updates::{ui_update_channel, UiUpdate, UiUpdateReceiver, UiUpdateSender};
use tempfile::TempDir;

async fn setup_ctx(name: &str) -> (Arc<IoContext>, UiUpdateSender, UiUpdateReceiver, TempDir) {
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
        .with_device_id(format!("test-device-{name}"))
        .with_mode(TuiMode::Production)
        .build()
        .expect("IoContext builder should succeed for tests");

    ctx.create_account(&format!("TestUser-{name}"))
        .await
        .expect("Failed to create account");

    let (tx, rx) = ui_update_channel();
    (Arc::new(ctx), tx, rx, dir)
}

async fn next_toast(rx: &mut UiUpdateReceiver) -> aura_terminal::tui::components::ToastMessage {
    tokio::time::timeout(std::time::Duration::from_millis(500), async {
        loop {
            let update = rx
                .recv()
                .await
                .expect("UiUpdate channel closed unexpectedly");
            if let UiUpdate::ToastAdded(toast) = update {
                return toast;
            }
        }
    })
    .await
    .expect("Timed out waiting for toast")
}

async fn ensure_chat_channel(ctx: &Arc<IoContext>) {
    create_channel(ctx.app_core_raw(), "channel:general", None, &[], 0, 0)
        .await
        .expect("Failed to create chat channel for tests");
}

#[tokio::test]
async fn slash_who_emits_participants_toast() {
    let (ctx, tx, mut rx, _dir) = setup_ctx("who").await;
    let callbacks = ChatCallbacks::new(ctx.clone(), tx, ctx.app_core_raw().clone());
    ensure_chat_channel(&ctx).await;

    let on_send = callbacks.on_send.clone();
    on_send("channel:general".to_string(), "/who".to_string());

    let toast = next_toast(&mut rx).await;
    assert_eq!(toast.id, "participants");
}

#[tokio::test]
async fn slash_whois_emits_whois_toast() {
    let (ctx, tx, mut rx, _dir) = setup_ctx("whois").await;
    let callbacks = ChatCallbacks::new(ctx.clone(), tx, ctx.app_core_raw().clone());
    ensure_chat_channel(&ctx).await;

    let on_send = callbacks.on_send.clone();
    on_send("channel:general".to_string(), "/whois test-user".to_string());

    let toast = next_toast(&mut rx).await;
    assert_eq!(toast.id, "whois");
}
