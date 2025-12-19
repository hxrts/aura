#![allow(missing_docs)]

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::{signal_defs::ERROR_SIGNAL, AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;

use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::tui::context::IoContext;
use aura_terminal::tui::effects::EffectCommand;

async fn wait_for_error(app_core: &Arc<RwLock<AppCore>>) -> aura_app::signal_defs::AppError {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        {
            let core = app_core.read().await;
            if let Ok(Some(err)) = core.read(&*ERROR_SIGNAL).await {
                return err;
            }
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("Timed out waiting for ERROR_SIGNAL to become Some");
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

async fn test_ctx(
    has_existing_account: bool,
) -> (Arc<RwLock<AppCore>>, IoContext, tempfile::TempDir) {
    let app_core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
    app_core
        .init_signals()
        .await
        .expect("Failed to init signals");
    let app_core = Arc::new(RwLock::new(app_core));

    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let ctx = IoContext::with_account_status(
        app_core.clone(),
        has_existing_account,
        dir.path().to_path_buf(),
        "test-device".to_string(),
        TuiMode::Production,
    );
    (app_core, ctx, dir)
}

#[tokio::test]
async fn capability_denied_emits_error_signal() {
    let (app_core, ctx, _dir) = test_ctx(true).await;

    let _ = ctx
        .dispatch(EffectCommand::KickUser {
            channel: "block:home".to_string(),
            target: "someone".to_string(),
            reason: None,
        })
        .await;
    let err = wait_for_error(&app_core).await;
    assert_eq!(err.code, "CAPABILITY_DENIED");
}

#[tokio::test]
async fn operational_failure_emits_error_signal() {
    let (app_core, ctx, _dir) = test_ctx(true).await;

    let _ = ctx.dispatch(EffectCommand::ForceSync).await;
    let err = wait_for_error(&app_core).await;
    assert_eq!(err.code, "OPERATION_FAILED");
}

#[tokio::test]
async fn unknown_command_emits_error_signal() {
    // NOTE: The "unknown command" path is covered by a unit test because it
    // relies on a test-only `EffectCommand` variant.
    //
    // Keep this integration test as a thin sanity check that error propagation
    // still works at runtime (operational failure already covers it).
    let (app_core, ctx, _dir) = test_ctx(false).await;

    let _ = ctx.dispatch(EffectCommand::ForceSync).await;
    let err = wait_for_error(&app_core).await;
    assert_eq!(err.code, "OPERATION_FAILED");
}
