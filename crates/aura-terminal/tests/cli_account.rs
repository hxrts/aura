//! The `aura` CLI binds to an account created by the terminal app through
//! `--data-dir` (work/8.md Task 22).

#![allow(missing_docs)]

mod support;

use aura_terminal::handlers::tui::TuiMode;
use support::IoContextTestEnvBuilder;

fn aura(data_dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aura"))
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run aura {args:?}: {error}"));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "aura {args:?} failed: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

#[tokio::test]
async fn cli_reads_an_account_created_by_the_terminal() {
    let data_dir = std::env::temp_dir().join(format!("aura-cli-account-{}", std::process::id()));
    let env = IoContextTestEnvBuilder::new("cli-account")
        .with_base_path(data_dir.clone())
        .with_device_id("test-device-cli-account")
        .with_mode(TuiMode::Production)
        .create_account_as("CliTester")
        .build()
        .await;
    assert!(env.ctx.has_account());

    let status = aura(&data_dir, &["status"]);
    assert!(status.contains("Nickname: CliTester"), "{status}");

    let authorities = aura(&data_dir, &["authority", "list"]);
    assert!(authorities.contains("(current account)"), "{authorities}");

    drop(env);
    let _ = std::fs::remove_dir_all(&data_dir);
}
