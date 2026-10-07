//! Every `aura` command emits one valid JSON document under `--json`, and
//! its exit code matches the document (work/8.md Task 148).

#![allow(
    missing_docs,
    dead_code,
    unused,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::all
)]

mod support;

use aura_terminal::handlers::tui::TuiMode;
use serde_json::Value;
use std::path::Path;
use support::IoContextTestEnvBuilder;

struct Run {
    code: i32,
    doc: Value,
}

/// Run `aura --json --data-dir <dir> <args>`; stdout must be exactly one
/// JSON document whose `ok` agrees with the exit code.
fn aura_json(data_dir: &Path, args: &[&str]) -> Run {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aura"))
        .arg("--json")
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("run aura {args:?}: {error}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let doc: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
        panic!("aura {args:?}: stdout is not one JSON document ({error}):\n{stdout}\nstderr:\n{stderr}")
    });
    let code = output.status.code().expect("aura exited by signal");
    assert_eq!(
        doc["ok"].as_bool(),
        Some(code == 0),
        "aura {args:?}: exit code {code} disagrees with {doc}"
    );
    if code != 0 {
        assert!(doc["error"]["code"].is_string(), "{doc}");
        assert!(doc["error"]["message"].is_string(), "{doc}");
    }
    Run { code, doc }
}

#[tokio::test]
async fn every_command_emits_valid_json() {
    let data_dir = std::env::temp_dir().join(format!("aura-cli-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let env = IoContextTestEnvBuilder::new("cli-json")
        .with_base_path(data_dir.clone())
        .with_device_id("test-device-cli-json")
        .with_mode(TuiMode::Production)
        .create_account_as("JsonTester")
        .build()
        .await;
    assert!(env.ctx.has_account());

    let ok = |args: &[&str]| {
        let run = aura_json(&data_dir, args);
        assert_eq!(run.code, 0, "aura {args:?} failed: {}", run.doc);
        run.doc
    };
    let fails = |args: &[&str], code: i32| {
        let run = aura_json(&data_dir, args);
        assert_eq!(run.code, code, "aura {args:?}: {}", run.doc);
        run.doc
    };

    let version = ok(&["version"]);
    assert_eq!(
        version["result"]["sections"][0]["fields"]["Package"],
        "aura-terminal"
    );

    // Account commands print their typed response.
    let status = ok(&["status"]);
    assert_eq!(status["result"]["type"], "account");
    assert_eq!(status["result"]["data"]["nickname"], "JsonTester");
    let authorities = ok(&["authority", "list"]);
    assert_eq!(authorities["result"]["data"][0]["current"], true);
    assert_eq!(ok(&["chat", "list"])["result"]["type"], "channels");
    assert_eq!(
        ok(&["invite", "list"])["result"],
        serde_json::json!({"type": "invitations", "data": []})
    );
    assert_eq!(ok(&["sync", "status"])["result"]["type"], "sync");
    assert_eq!(ok(&["recovery", "status"])["result"]["type"], "recovery");
    ok(&["chat", "search", "anything"]);

    // The rest of the command surface: valid documents whatever the outcome.
    for args in [
        &["snapshot"][..],
        &[
            "threshold",
            "--configs",
            "/nonexistent.toml",
            "--threshold",
            "1",
            "--mode",
            "verify",
        ][..],
        &["invite", "accept", "--invitation-id", "missing"][..],
    ] {
        aura_json(&data_dir, args);
    }

    // Failures are documents too, with distinct exit codes.
    fails(&["node"], 2);
    fails(&["no-such-command"], 2);
    fails(&["replay", "--trace-file", "/nonexistent/trace.json"], 2);
    fails(&["invite", "import", "--code", "not-a-code"], 2);
    fails(
        &["amp", "inspect", "--context", "bad", "--channel", "bad"],
        2,
    );
    fails(&["context", "inspect", "--context", "bad"], 2);
    fails(&["chat", "history", "no-such-channel"], 3);
    // Destructive commands need --yes without a terminal.
    fails(&["chat", "leave", "general"], 2);

    drop(env);
    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn missing_account_is_a_not_found_document() {
    let data_dir = std::env::temp_dir().join(format!("aura-cli-json-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let run = aura_json(&data_dir, &["status"]);
    assert_eq!(run.code, 3, "{}", run.doc);
    assert_eq!(run.doc["error"]["code"], "not_found");
}
