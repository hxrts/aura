//! Minimal host stub intended for downstream bindings (Swift/Android/etc.).
//! This binary is guarded by the `host` feature and simply boots the headless
//! `aura-app::AppCore` so host integrations can easily reference it from the
//! environment they control (e.g., to bootstrap Swift callbacks or Kotlin wrappers).
//! It is **not** part of the CLI/TUI stack.
use anyhow::Result;
use aura_app::{AppConfig, AppCore};

fn main() -> Result<()> {
    let config = AppConfig::default();
    let app = AppCore::new(config)?;
    println!("App host ready with account {}", app.account_id());
    Ok(())
}
