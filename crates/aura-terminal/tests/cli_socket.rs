//! A running node serves `aura` commands over its owner-only local socket:
//! the CLI binary answers from the node instead of opening the profile
//! (work/8.md Task 160, Task 22 D15).

#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

#[path = "cli_rpc/simnet.rs"]
#[allow(dead_code)]
mod simnet;

use anyhow::{anyhow, Result};
use aura_terminal::rpc_socket;
use serde_json::Value;
use simnet::SimNet;
use std::os::unix::fs::PermissionsExt;

#[tokio::test(start_paused = true)]
async fn cli_commands_reach_a_running_node_over_its_socket() -> Result<()> {
    let net = SimNet::new();
    let peer = net.peer(61).await?;
    // A data directory far deeper than a Unix socket path may be (Task 210:
    // run 174's harness TUIs could not bind `<data-dir>.sock`).
    let root = tempfile::tempdir()?;
    let deep = root
        .path()
        .join("a-rather-long-directory-name-for-the-run-artifacts")
        .join("another-long-component-for-a-scenario-and-instance")
        .join("and-a-third-component-so-any-temp-root-exceeds-the-limit")
        .join("data");
    std::fs::create_dir_all(&deep)?;
    assert!(deep.as_os_str().len() > 150, "{}", deep.display());

    let hosted = rpc_socket::host(&deep).await?;
    let path = hosted.path().to_path_buf();
    assert!(path.as_os_str().len() <= 100, "{}", path.display());
    assert_eq!(
        rpc_socket::socket_path(&deep),
        path,
        "clients find the socket the node recorded"
    );
    assert_eq!(
        std::fs::metadata(&path)?.permissions().mode() & 0o777,
        0o600,
        "the socket is owner-only"
    );
    assert!(
        rpc_socket::host(&deep).await.is_err(),
        "a second node must not take the socket"
    );

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = rpc_socket::serve(&peer.ctx, hosted, async {
        let _ = stopped.await;
    });
    let dir = deep.clone();
    let client = async move {
        // No account exists in `dir`: only the node can answer.
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new(env!("CARGO_BIN_EXE_aura"))
                .arg("--json")
                .arg("--data-dir")
                .arg(&dir)
                .args(["chat", "list"])
                .stdin(std::process::Stdio::null())
                .output()
        })
        .await??;
        let _ = stop.send(());
        let doc: Value = serde_json::from_slice(&output.stdout).map_err(|e| {
            anyhow!(
                "{e}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        assert_eq!(doc["ok"], true, "{doc}");
        assert_eq!(doc["result"]["type"], "channels", "{doc}");
        Ok::<(), anyhow::Error>(())
    };
    let (served, outcome) = tokio::join!(server, client);
    served?;
    outcome?;
    assert!(
        !path.exists(),
        "the socket file is removed when the node stops"
    );
    assert!(
        !deep.with_file_name("data.sock-path").exists(),
        "the recorded socket path is removed when the node stops"
    );
    Ok(())
}
