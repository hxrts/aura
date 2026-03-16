//! Socket infrastructure for harness command ingress.

use crate::tui::updates::{
    HarnessCommandReceiptHandle, HarnessCommandSender, HarnessCommandSubmission,
};
use aura_app::ui::contract::{HarnessUiCommand, HarnessUiCommandReceipt};
use parking_lot::Mutex;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

const COMMAND_SOCKET_ENV: &str = "AURA_TUI_COMMAND_SOCKET";

static COMMAND_SOCKET: OnceLock<Option<PathBuf>> = OnceLock::new();
static ACTIVE_HARNESS_COMMAND_SENDER: OnceLock<Mutex<Option<HarnessCommandSender>>> =
    OnceLock::new();
static HARNESS_COMMAND_LISTENER_STARTED: OnceLock<()> = OnceLock::new();

struct HarnessSocketGuard {
    path: PathBuf,
}

impl HarnessSocketGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for HarnessSocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn configured_command_socket() -> Option<&'static PathBuf> {
    COMMAND_SOCKET
        .get_or_init(|| std::env::var_os(COMMAND_SOCKET_ENV).map(PathBuf::from))
        .as_ref()
}

fn bind_harness_command_listener() -> io::Result<Option<(UnixListener, HarnessSocketGuard)>> {
    let Some(path) = configured_command_socket().cloned() else {
        return Ok(None);
    };
    let _ = fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    UnixListener::from_std(listener).map(|listener| Some((listener, HarnessSocketGuard::new(path))))
}

fn active_harness_command_sender() -> &'static Mutex<Option<HarnessCommandSender>> {
    ACTIVE_HARNESS_COMMAND_SENDER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn ensure_harness_command_listener() -> io::Result<()> {
    if HARNESS_COMMAND_LISTENER_STARTED.get().is_some() {
        return Ok(());
    }
    let Some((listener, guard)) = bind_harness_command_listener()? else {
        return Ok(());
    };
    tokio::spawn(async move {
        let _guard = guard;
        forward_harness_commands_from_listener(listener).await;
        tracing::debug!("harness command listener exited");
    });
    let _ = HARNESS_COMMAND_LISTENER_STARTED.set(());
    Ok(())
}

pub(crate) fn register_harness_command_sender(sender: HarnessCommandSender) {
    *active_harness_command_sender().lock() = Some(sender);
}

pub(crate) fn clear_harness_command_sender() {
    *active_harness_command_sender().lock() = None;
}

async fn forward_harness_commands_from_listener(listener: UnixListener) {
    loop {
        let Ok((stream, _addr)) = listener.accept().await else {
            break;
        };
        if !process_harness_command_stream(stream).await {
            break;
        }
    }
}

/// Per-connection timeout for reading a harness command payload.
const HARNESS_COMMAND_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn process_harness_command_stream(mut stream: UnixStream) -> bool {
    let mut payload = Vec::new();
    let read_result = tokio::time::timeout(
        HARNESS_COMMAND_READ_TIMEOUT,
        stream.read_to_end(&mut payload),
    )
    .await;
    let read_result = match read_result {
        Ok(inner) => inner,
        Err(_) => {
            let _ = write_harness_command_receipt(
                &mut stream,
                &HarnessUiCommandReceipt::Rejected {
                    reason: "harness command read timed out".to_string(),
                },
            )
            .await;
            return true;
        }
    };
    if let Err(error) = read_result {
        let _ = write_harness_command_receipt(
            &mut stream,
            &HarnessUiCommandReceipt::Rejected {
                reason: format!("failed to read harness command payload: {error}"),
            },
        )
        .await;
        return true;
    }
    let command = match serde_json::from_slice::<HarnessUiCommand>(&payload) {
        Ok(command) => command,
        Err(error) => {
            let _ = write_harness_command_receipt(
                &mut stream,
                &HarnessUiCommandReceipt::Rejected {
                    reason: format!("failed to decode harness command payload: {error}"),
                },
            )
            .await;
            return true;
        }
    };
    let max_retries: u32 = std::env::var("AURA_HARNESS_COMMAND_max_retries")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200); // 200 × 50ms = 10s budget
    let retry_interval_ms: u64 = std::env::var("AURA_HARNESS_COMMAND_retry_interval_ms")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let mut attempts = 0u32;
    let receipt = loop {
        let command_tx = active_harness_command_sender().lock().clone();
        let Some(command_tx) = command_tx else {
            attempts += 1;
            if attempts >= max_retries {
                break HarnessUiCommandReceipt::Rejected {
                    reason: "TUI harness command plane is temporarily unavailable".to_string(),
                };
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(retry_interval_ms)).await;
            continue;
        };

        let (receipt_tx, receipt_rx) = tokio::sync::oneshot::channel();
        match command_tx
            .send(HarnessCommandSubmission {
                command: command.clone(),
                receipt: HarnessCommandReceiptHandle::new(receipt_tx),
            })
            .await
        {
            Ok(()) => {}
            Err(error) => {
                attempts += 1;
                if attempts >= max_retries {
                    break HarnessUiCommandReceipt::Rejected {
                        reason: format!(
                            "failed to submit harness command into shell ingress: {error}"
                        ),
                    };
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(retry_interval_ms)).await;
                continue;
            }
        }

        match receipt_rx.await {
            Ok(receipt) => break receipt,
            Err(error) => {
                attempts += 1;
                if attempts >= max_retries {
                    break HarnessUiCommandReceipt::Rejected {
                        reason: format!("harness command dropped before application: {error}"),
                    };
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(retry_interval_ms)).await;
            }
        }
    };
    let _ = write_harness_command_receipt(&mut stream, &receipt).await;
    true
}

async fn write_harness_command_receipt(
    stream: &mut UnixStream,
    receipt: &HarnessUiCommandReceipt,
) -> io::Result<()> {
    let payload = serde_json::to_vec(receipt).map_err(|error| {
        io::Error::other(format!("failed to encode harness command receipt: {error}"))
    })?;
    stream.write_all(&payload).await?;
    stream.flush().await
}

// Re-export forward_harness_commands_from_listener for tests in mod.rs.
#[cfg(test)]
pub(super) use forward_harness_commands_from_listener as forward_harness_commands_from_listener_fn;
