//! Every `aura` command emits one valid JSON document under `--json`, and
//! its exit code matches the document (work/8.md Task 148).
//!
//! Account commands run against a node serving the data directory's socket
//! (a virtual-time simulation runtime), the way they reach a running TUI or
//! `aura serve`.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::disallowed_types
)]

#[path = "cli_rpc/simnet.rs"]
#[allow(dead_code)]
mod simnet;

use aura_terminal::rpc_socket;
use serde_json::Value;
use simnet::SimNet;
use std::path::Path;

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

/// The commands, run synchronously against `dir`.
fn exercise(dir: &Path) {
    let ok = |args: &[&str]| {
        let run = aura_json(dir, args);
        assert_eq!(run.code, 0, "aura {args:?} failed: {}", run.doc);
        run.doc
    };
    let fails = |args: &[&str], code: i32| {
        let run = aura_json(dir, args);
        assert_eq!(run.code, code, "aura {args:?}: {}", run.doc);
        run.doc
    };

    let version = ok(&["version"]);
    assert_eq!(
        version["result"]["sections"][0]["fields"]["Package"],
        "aura-terminal"
    );

    // Account commands print their typed response.
    assert_eq!(ok(&["chat", "list"])["result"]["type"], "channels");
    assert_eq!(ok(&["contact", "list"])["result"]["type"], "contacts");
    assert_eq!(
        ok(&["invite", "list"])["result"],
        serde_json::json!({"type": "invitations", "data": []})
    );
    assert_eq!(ok(&["sync", "status"])["result"]["type"], "sync");
    assert_eq!(ok(&["recovery", "status"])["result"]["type"], "recovery");
    assert_eq!(
        ok(&["chat", "search", "anything"])["result"]["type"],
        "messages"
    );

    // The rest of the command surface: valid documents whatever the outcome.
    for args in [
        &["status"][..],
        &["authority", "list"][..],
        &["snapshot"][..],
        &["invite", "accept", "--invitation-id", "missing"][..],
    ] {
        aura_json(dir, args);
    }

    // Failures are documents too, with distinct exit codes.
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
}

#[tokio::test(start_paused = true)]
async fn every_command_emits_valid_json() {
    let net = SimNet::new();
    let peer = net.peer(71).await.unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let hosted = rpc_socket::host(data_dir.path()).await.unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = rpc_socket::serve(&peer.ctx, hosted, async {
        let _ = stopped.await;
    });
    let dir = data_dir.path().to_path_buf();
    let client = async move {
        let result = tokio::task::spawn_blocking(move || exercise(&dir)).await;
        let _ = stop.send(());
        result
    };
    let (served, outcome) = tokio::join!(server, client);
    served.unwrap();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic.into_panic());
    }
}

#[test]
fn missing_account_is_a_not_found_document() {
    let data_dir = tempfile::tempdir().unwrap();
    let run = aura_json(data_dir.path(), &["status"]);
    assert_eq!(run.code, 3, "{}", run.doc);
    assert_eq!(run.doc["error"]["code"], "not_found");
}
