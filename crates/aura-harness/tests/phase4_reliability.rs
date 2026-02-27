#![allow(missing_docs)]

use std::fs;
use std::process::Command;

use aura_harness::screen_normalization::normalize_screen;

#[test]
fn screen_normalization_removes_volatile_tokens() {
    let raw = "tick 12:34:56 / #99";
    let normalized = normalize_screen(raw);
    assert!(normalized.contains("<time>"));
    assert!(normalized.contains("<spin>"));
    assert!(normalized.contains("#<n>"));
}

#[test]
fn lint_rejects_unknown_instance_references() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let config_path = temp.path().join("run.toml");
    let scenario_path = temp.path().join("scenario.toml");

    fs::write(
        &config_path,
        format!(
            r#"schema_version = 1

[run]
name = "lint-instance"
pty_rows = 40
pty_cols = 120

[[instances]]
id = "alice"
mode = "local"
data_dir = "{}"
bind_address = "127.0.0.1:47001"
"#,
            temp.path().join("alice-data").display()
        ),
    )
    .unwrap_or_else(|error| panic!("write config failed: {error}"));

    fs::write(
        &scenario_path,
        r#"schema_version = 1
id = "bad-instance"
goal = "lint should fail"
execution_mode = "scripted"

[[steps]]
id = "s1"
action = "wait_for"
instance = "ghost"
expect = "never"
timeout_ms = 10
"#,
    )
    .unwrap_or_else(|error| panic!("write scenario failed: {error}"));

    let binary = env!("CARGO_BIN_EXE_aura-harness");
    let output = Command::new(binary)
        .arg("lint")
        .arg("--config")
        .arg(config_path.as_os_str())
        .arg("--scenario")
        .arg(scenario_path.as_os_str())
        .output()
        .unwrap_or_else(|error| panic!("lint command failed to start: {error}"));

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("scenario lint failed"));
}

#[test]
fn timeout_failures_emit_timeout_diagnostics_bundle() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let config_path = temp.path().join("run.toml");
    let scenario_path = temp.path().join("scenario.toml");
    let artifacts_path = temp.path().join("artifacts");

    fs::write(
        &config_path,
        format!(
            r#"schema_version = 1

[run]
name = "timeout-diagnostics"
pty_rows = 40
pty_cols = 120
global_budget_ms = 100
step_budget_ms = 50

[[instances]]
id = "alice"
mode = "local"
data_dir = "{}"
bind_address = "127.0.0.1:47002"
command = "bash"
args = ["-lc", "cat"]
"#,
            temp.path().join("alice-data").display()
        ),
    )
    .unwrap_or_else(|error| panic!("write config failed: {error}"));

    fs::write(
        &scenario_path,
        r#"schema_version = 1
id = "timeout-case"
goal = "force timeout"
execution_mode = "scripted"

[[steps]]
id = "wait-never"
action = "wait_for"
instance = "alice"
expect = "this-pattern-will-not-appear"
timeout_ms = 10
"#,
    )
    .unwrap_or_else(|error| panic!("write scenario failed: {error}"));

    let binary = env!("CARGO_BIN_EXE_aura-harness");
    let status = Command::new(binary)
        .arg("run")
        .arg("--config")
        .arg(config_path.as_os_str())
        .arg("--scenario")
        .arg(scenario_path.as_os_str())
        .arg("--artifacts-dir")
        .arg(artifacts_path.as_os_str())
        .status()
        .unwrap_or_else(|error| panic!("run command failed to start: {error}"));

    assert!(!status.success());

    let run_dir = artifacts_path.join("harness").join("timeout-diagnostics");
    assert!(run_dir.join("timeout_diagnostics.json").exists());
}
