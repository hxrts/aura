//! The `aura` binary opens a real account's production profile (Task 22 D15,
//! Task 160): the account is created through the TUI's own creation path
//! (runtime-free staging, then the first production launch reconciles it);
//! with no node running the CLI opens the profile itself and signs, and with
//! `aura serve` running the same commands go through its socket.
//!
//! Test builds keep platform credentials in the file-backed test keyring
//! (`aura-effects` `test-keyring`, see docs/804); `AURA_TEST_KEYRING_DIR`
//! scopes it to this test.

#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

fn aura(keyring: &Path, data_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aura"));
    command
        .env("AURA_TEST_KEYRING_DIR", keyring)
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::null());
    command
}

fn aura_json(keyring: &Path, data_dir: &Path, args: &[&str]) -> Value {
    let output = aura(keyring, data_dir)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "aura {args:?}: {e}\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(doc["ok"], true, "aura {args:?}: {doc}");
    doc["result"].clone()
}

/// A fresh invitee authority for signing commands.
const INVITEE: &str = "authority-0f0f0f0f-0f0f-0f0f-0f0f-0f0f0f0f0f0f";

#[test]
fn cli_opens_a_created_account_in_production_and_routes_to_a_running_node() {
    let keyring = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::env::set_var("AURA_TEST_KEYRING_DIR", keyring.path());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(aura_terminal::handlers::tui::create_account(
            data.path(),
            aura_terminal::handlers::tui::TuiMode::Production,
            "Alex",
        ))
        .unwrap();

    // No node running: the CLI opens the production profile, finishes the
    // new account's bootstrap, and signs.
    let status = aura_json(keyring.path(), data.path(), &["status"]);
    assert_eq!(status["type"], "account");
    assert_eq!(status["data"]["nickname"], "Alex");
    let invited = aura_json(
        keyring.path(),
        data.path(),
        &["invite", "create", "--invitee", INVITEE],
    );
    assert!(
        invited["data"]["code"]
            .as_str()
            .is_some_and(|c| c.starts_with("aura:")),
        "{invited}"
    );

    // `aura serve` holds the profile; commands go through its socket.
    let mut serve = aura(keyring.path(), data.path())
        .arg("serve")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(serve.stderr.take().unwrap());
    let mut line = String::new();
    while stderr.read_line(&mut line).unwrap() > 0 && !line.contains("online at") {
        line.clear();
    }
    assert!(line.contains("online at"), "aura serve did not come online");
    let socket = aura_terminal::rpc_socket::socket_path(data.path());
    assert!(socket.exists());

    assert_eq!(
        aura_json(keyring.path(), data.path(), &["status"])["data"]["nickname"],
        "Alex"
    );
    let routed = aura_json(
        keyring.path(),
        data.path(),
        &["invite", "create", "--invitee", INVITEE],
    );
    assert!(routed["data"]["code"].as_str().is_some(), "{routed}");

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(serve.id()).unwrap()),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    assert!(serve.wait().unwrap().success());
    assert!(
        !socket.exists(),
        "the socket is removed when the node stops"
    );
}

/// `aura account create` takes the same creation path and the account then
/// opens in production; creating over an existing account is refused.
#[test]
fn cli_account_create_opens_in_production_and_refuses_overwrite() {
    let keyring = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let created = aura_json(
        keyring.path(),
        data.path(),
        &["account", "create", "--nickname", "Barbara"],
    );
    assert_eq!(created["data"]["nickname"], "Barbara");
    assert_eq!(
        aura_json(keyring.path(), data.path(), &["status"])["data"]["nickname"],
        "Barbara"
    );
    let again = aura(keyring.path(), data.path())
        .args(["--json", "account", "create", "--nickname", "Other"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(doc["ok"], false, "{doc}");
    assert_eq!(doc["error"]["code"], "invalid_input", "{doc}");

    // An unknown ceremony id is typed not_found end to end, exit code 3
    // (Task 185).
    let unknown = aura(keyring.path(), data.path())
        .args(["--json", "rotation", "status", "no-such-ceremony"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(doc["error"]["code"], "not_found", "{doc}");
    assert_eq!(unknown.status.code(), Some(3), "{doc}");
}
