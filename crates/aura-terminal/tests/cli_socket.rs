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
    let data_dir = tempfile::tempdir()?;
    let path = rpc_socket::socket_path(data_dir.path());
    let listener = rpc_socket::bind(&path).await?;
    assert_eq!(
        std::fs::metadata(&path)?.permissions().mode() & 0o777,
        0o600,
        "the socket is owner-only"
    );
    assert!(
        rpc_socket::bind(&path).await.is_err(),
        "a second node must not take the socket"
    );

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = rpc_socket::serve(&peer.ctx, listener, &path, async {
        let _ = stopped.await;
    });
    let dir = data_dir.path().to_path_buf();
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
    Ok(())
}
