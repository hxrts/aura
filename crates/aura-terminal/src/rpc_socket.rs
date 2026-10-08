//! Local Unix-socket transport for `aura rpc` sessions.
//!
//! A running node (the TUI, or `aura serve`) listens on a node socket so
//! several clients can attach to the one runtime that holds the account's
//! profile. Unix socket paths are short (about 104 bytes on macOS), so the
//! socket lives in the user's runtime directory (`$XDG_RUNTIME_DIR`, else
//! `$TMPDIR`, else `/tmp`) as `aura-<hash of the data dir>.sock`, and the
//! node records that path in `<data-dir>.sock-path`, beside the data
//! directory (outside the owned profile), where clients look it up. The
//! socket is owner-only: its file mode is `0600`, and connections from
//! another user id are refused. There is no network listener. CLI commands
//! try the socket first ([`call`]) and only open the profile themselves when
//! no node answers.

use crate::command::{CommandContext, CommandError, ErrorCode, Request, Response};
use crate::rpc;
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// Longest socket path this module binds: under every platform's
/// `sun_path` limit (104 bytes on macOS, 108 on Linux), with the NUL byte.
const MAX_SOCKET_PATH: usize = 100;

/// Where the node serving `base_path` records its socket path: a sibling of
/// the data directory (`~/.aura` → `~/.aura.sock-path`), outside the owned
/// profile directory, whose storage owns every entry inside it.
fn pointer_path(base_path: &Path) -> PathBuf {
    let name = base_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "aura".to_string());
    base_path.with_file_name(format!("{name}.sock-path"))
}

/// The short socket path a node for `base_path` binds: the user runtime
/// directory and a hash of the data directory's absolute path, so any data
/// directory depth fits. `/tmp` is used when the runtime directory itself
/// is too deep.
fn hosted_socket_path(base_path: &Path) -> PathBuf {
    let absolute = std::fs::canonicalize(base_path).unwrap_or_else(|_| base_path.to_path_buf());
    let digest = aura_core::hash::hash(absolute.as_os_str().as_encoded_bytes());
    let name = format!("aura-{}.sock", hex::encode(&digest[..8]));
    ["XDG_RUNTIME_DIR", "TMPDIR"]
        .iter()
        .filter_map(|var| std::env::var_os(var).map(PathBuf::from))
        .map(|dir| dir.join(&name))
        .find(|path| path.as_os_str().len() <= MAX_SOCKET_PATH)
        .unwrap_or_else(|| Path::new("/tmp").join(name))
}

/// The node socket for the account stored at `base_path`: the path its node
/// recorded, else the path a node for it would bind.
#[must_use]
pub fn socket_path(base_path: &Path) -> PathBuf {
    std::fs::read_to_string(pointer_path(base_path))
        .ok()
        .map(|recorded| PathBuf::from(recorded.trim_end()))
        .filter(|recorded| !recorded.as_os_str().is_empty())
        .unwrap_or_else(|| hosted_socket_path(base_path))
}

/// A node socket bound for one data directory. Dropping it removes the
/// socket file and the recorded path, however the server stops.
pub struct HostedSocket {
    listener: UnixListener,
    path: PathBuf,
    pointer: PathBuf,
}

impl HostedSocket {
    /// The socket's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for HostedSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(&self.pointer);
    }
}

/// Host the node socket for the account at `base_path`: bind the short
/// socket path and record it beside the data directory. Fails (rather than
/// running without a socket) when another node already answers, the path
/// cannot be bound, or the record cannot be written.
pub async fn host(base_path: &Path) -> std::io::Result<HostedSocket> {
    let path = hosted_socket_path(base_path);
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("node socket path {} is too long", path.display()),
        ));
    }
    let listener = bind(&path).await?;
    let pointer = pointer_path(base_path);
    let hosted = HostedSocket {
        listener,
        path,
        pointer,
    };
    std::fs::write(&hosted.pointer, format!("{}\n", hosted.path.display()))?;
    Ok(hosted)
}

fn same_user(stream: &UnixStream) -> bool {
    stream
        .peer_cred()
        .is_ok_and(|cred| cred.uid() == nix::unistd::getuid().as_raw())
}

/// Bind the node socket: refuse when another node already answers on it,
/// replace a stale file, and restrict it to the owner.
async fn bind(path: &Path) -> std::io::Result<UnixListener> {
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

/// Serve `aura rpc` sessions on the hosted socket until `stop` resolves;
/// each connection is one session on the shared runtime. The socket and its
/// record are removed when serving ends.
pub async fn serve(
    ctx: &CommandContext,
    hosted: HostedSocket,
    stop: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let listener = &hosted.listener;
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
