//! Socket infrastructure for harness command ingress.

use crate::tui::updates::{
    HarnessCommandReceiptHandle, HarnessCommandSender, HarnessCommandSubmission,
};
use aura_app::ui::contract::{HarnessUiCommand, HarnessUiCommandReceipt};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::{
    execute_with_retry_budget, execute_with_timeout_budget, ExponentialBackoffPolicy,
    RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutBudgetError,
    TimeoutExecutionProfile, TimeoutRunError,
};
use aura_effects::time::PhysicalTimeHandler;
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
const HARNESS_COMMAND_RETRY_INITIAL_DELAY: std::time::Duration =
    std::time::Duration::from_millis(50);
const HARNESS_COMMAND_RETRY_MAX_DELAY: std::time::Duration =
    std::time::Duration::from_millis(500);
const HARNESS_COMMAND_RETRY_ATTEMPTS: u32 = 200;

fn harness_timeout_profile() -> TimeoutExecutionProfile {
    TimeoutExecutionProfile::harness()
}

fn scaled_harness_duration(base: std::time::Duration) -> io::Result<std::time::Duration> {
    harness_timeout_profile()
        .scale_duration(base)
        .map_err(|error| io::Error::other(format!("invalid harness timeout policy: {error}")))
}

async fn harness_read_budget(time: &PhysicalTimeHandler) -> io::Result<TimeoutBudget> {
    let started_at = time
        .physical_time()
        .await
        .map_err(|error| io::Error::other(format!("failed to read physical time: {error}")))?;
    TimeoutBudget::from_start_and_timeout(&started_at, scaled_harness_duration(HARNESS_COMMAND_READ_TIMEOUT)?)
        .map_err(|error| io::Error::other(format!("failed to create harness read budget: {error}")))
}

fn harness_command_retry_policy() -> io::Result<RetryBudgetPolicy> {
    let base = RetryBudgetPolicy::new(
        HARNESS_COMMAND_RETRY_ATTEMPTS,
        ExponentialBackoffPolicy::new(
            HARNESS_COMMAND_RETRY_INITIAL_DELAY,
            HARNESS_COMMAND_RETRY_MAX_DELAY,
            harness_timeout_profile().jitter(),
        )
        .map_err(|error| io::Error::other(format!("invalid harness backoff policy: {error}")))?,
    );
    harness_timeout_profile()
        .apply_retry_policy(&base)
        .map_err(|error| io::Error::other(format!("invalid harness retry policy: {error}")))
}

fn timeout_error_reason(error: TimeoutBudgetError, context: &str) -> String {
    format!("{context} timed out: {error}")
}

fn retry_error_reason(error: RetryRunError<io::Error>, unavailable_reason: &str) -> String {
    match error {
        RetryRunError::Timeout(error) => timeout_error_reason(error, unavailable_reason),
        RetryRunError::AttemptsExhausted {
            attempts_used: _,
            last_error,
        } => last_error.to_string(),
    }
}

async fn process_harness_command_stream(mut stream: UnixStream) -> bool {
    let mut payload = Vec::new();
    let time = PhysicalTimeHandler::new();
    let read_budget = match harness_read_budget(&time).await {
        Ok(budget) => budget,
        Err(error) => {
            let _ = write_harness_command_receipt(
                &mut stream,
                &HarnessUiCommandReceipt::Rejected {
                    reason: error.to_string(),
                },
            )
            .await;
            return true;
        }
    };
    let read_result = execute_with_timeout_budget(&time, &read_budget, || async {
        stream.read_to_end(&mut payload).await
    })
    .await;
    let read_result = match read_result {
        Ok(_bytes_read) => Ok(()),
        Err(TimeoutRunError::Timeout(error)) => {
            let _ = write_harness_command_receipt(
                &mut stream,
                &HarnessUiCommandReceipt::Rejected {
                    reason: timeout_error_reason(error, "harness command read"),
                },
            )
            .await;
            return true;
        }
        Err(TimeoutRunError::Operation(error)) => Err(error),
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
    let retry_policy = match harness_command_retry_policy() {
        Ok(policy) => policy,
        Err(error) => {
            let _ = write_harness_command_receipt(
                &mut stream,
                &HarnessUiCommandReceipt::Rejected {
                    reason: error.to_string(),
                },
            )
            .await;
            return true;
        }
    };
    let receipt = match execute_with_retry_budget(&time, &retry_policy, |_| {
        let command = command.clone();
        async move {
            let command_tx =
                active_harness_command_sender()
                    .lock()
                    .clone()
                    .ok_or_else(|| {
                        io::Error::other("TUI harness command plane is temporarily unavailable")
                    })?;

            let (receipt_tx, receipt_rx) = tokio::sync::oneshot::channel();
            command_tx
                .send(HarnessCommandSubmission {
                    command,
                    receipt: HarnessCommandReceiptHandle::new(receipt_tx),
                })
                .await
                .map_err(|error| {
                    io::Error::other(format!(
                        "failed to submit harness command into shell ingress: {error}"
                    ))
                })?;

            receipt_rx.await.map_err(|error| {
                io::Error::other(format!("harness command dropped before application: {error}"))
            })
        }
    })
    .await
    {
        Ok(receipt) => receipt,
        Err(error) => HarnessUiCommandReceipt::Rejected {
            reason: retry_error_reason(error, "harness command submission"),
        },
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
