//! Local Unix-socket transport for `aura rpc` sessions.
//!
//! A running node (the TUI, or `aura serve`) listens on `<data-dir>/aura.sock`
//! so several clients can attach to the one runtime that holds the
//! account's profile. The socket is owner-only: its file mode is `0600`, and
//! connections from another user id are refused. There is no network
//! listener. CLI commands try the socket first ([`call`]) and only open the
//! profile themselves when no node answers.

use crate::command::{CommandContext, CommandError, ErrorCode, Request, Response};
use crate::rpc;
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// Socket file name inside the account's data directory.
pub const SOCKET_FILE: &str = "aura.sock";

/// The node socket for the account stored at `base_path`.
#[must_use]
pub fn socket_path(base_path: &Path) -> PathBuf {
    base_path.join(SOCKET_FILE)
}

/// Removes the socket file when the server stops, however it stops.
struct SocketFile(PathBuf);

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn same_user(stream: &UnixStream) -> bool {
    stream
        .peer_cred()
        .is_ok_and(|cred| cred.uid() == nix::unistd::getuid().as_raw())
}

/// Bind the node socket: refuse when another node already answers on it,
/// replace a stale file, and restrict it to the owner.
pub async fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("a node already serves {}", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Serve `aura rpc` sessions on `listener` until `stop` resolves; each
/// connection is one session on the shared runtime.
pub async fn serve(
    ctx: &CommandContext,
    listener: UnixListener,
    path: &Path,
    stop: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let _socket_file = SocketFile(path.to_path_buf());
    let mut sessions = FuturesUnordered::new();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            () = &mut stop => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                if !same_user(&stream) {
                    continue;
                }
                let (reader, writer) = stream.into_split();
                sessions.push(rpc::serve(ctx, BufReader::new(reader), writer));
            }
            Some(_session) = sessions.next(), if !sessions.is_empty() => {}
        }
    }
    Ok(())
}

/// Attach this process's stdin/stdout to the node serving `path`, so
/// `aura rpc` reaches a running node instead of opening the profile.
/// Returns `false` when no node answers there.
pub async fn attach_stdio(path: &Path) -> std::io::Result<bool> {
    let Ok(stream) = UnixStream::connect(path).await else {
        return Ok(false);
    };
    let (mut reader, mut writer) = stream.into_split();
    let upstream = async {
        tokio::io::copy(&mut tokio::io::stdin(), &mut writer).await?;
        // EOF on stdin ends the session on the node.
        writer.shutdown().await
    };
    let mut stdout = tokio::io::stdout();
    let downstream = tokio::io::copy(&mut reader, &mut stdout);
    let (up, down) = tokio::join!(upstream, downstream);
    up?;
    down?;
    Ok(true)
}

/// Run one request on the node serving `path`. `Ok(None)` means no node
/// answers there, so the caller should open the profile itself.
pub async fn call(
    path: &Path,
    request: &Request,
    timeout_secs: Option<u64>,
) -> Result<Option<Result<Response, CommandError>>, CommandError> {
    let unavailable =
        |e: std::io::Error| CommandError::new(ErrorCode::Unavailable, format!("node socket: {e}"));
    let Ok(stream) = UnixStream::connect(path).await else {
        return Ok(None);
    };
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let hello: Value = match lines.next_line().await.map_err(unavailable)? {
        Some(line) => serde_json::from_str(&line)
            .map_err(|e| CommandError::new(ErrorCode::Unavailable, format!("node hello: {e}")))?,
        None => return Ok(None),
    };
    if hello["protocol"] != rpc::PROTOCOL || hello["version"] != json!(rpc::PROTOCOL_VERSION) {
        return Err(CommandError::new(
            ErrorCode::Unavailable,
            format!(
                "the running node speaks {} v{}",
                hello["protocol"], hello["version"]
            ),
        ));
    }
    let mut line = serde_json::to_value(request)
        .map_err(|e| CommandError::invalid(format!("encode request: {e}")))?;
    line["id"] = json!(1);
    if let Some(timeout) = timeout_secs {
        line["timeout_secs"] = json!(timeout);
    }
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .map_err(unavailable)?;
    while let Some(line) = lines.next_line().await.map_err(unavailable)? {
        let value: Value = serde_json::from_str(&line)
            .map_err(|e| CommandError::new(ErrorCode::Unavailable, format!("node reply: {e}")))?;
        if value["type"] != "response" || value["id"] != json!(1) {
            continue;
        }
        let decode = |e: serde_json::Error| {
            CommandError::new(ErrorCode::Unavailable, format!("node reply: {e}"))
        };
        return Ok(Some(if value["ok"] == json!(true) {
            Ok(serde_json::from_value(value["result"].clone()).map_err(decode)?)
        } else {
            Err(serde_json::from_value(value["error"].clone()).map_err(decode)?)
        }));
    }
    Err(CommandError::new(
        ErrorCode::Unavailable,
        "the node closed the session before answering",
    ))
}
