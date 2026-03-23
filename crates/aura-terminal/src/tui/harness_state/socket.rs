//! Socket infrastructure for harness command ingress.

use crate::tui::tasks::UiTaskOwner;
use crate::tui::updates::{HarnessCommandSender, HarnessCommandSubmission};
use aura_app::scenario_contract::SemanticCommandValue;
use aura_app::ui::contract::{HarnessUiCommand, HarnessUiCommandReceipt};
use aura_app::ui_contract::{
    ChannelBindingWitness, HarnessUiOperationHandle, OperationInstanceId,
};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::{
    execute_with_timeout_budget, TimeoutBudget, TimeoutBudgetError, TimeoutExecutionProfile,
    TimeoutRunError,
};
use aura_effects::time::PhysicalTimeHandler;
use std::fs;
use std::io;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, watch};

const COMMAND_SOCKET_ENV: &str = "AURA_TUI_COMMAND_SOCKET";

static COMMAND_SOCKET: OnceLock<Option<PathBuf>> = OnceLock::new();
static HARNESS_COMMAND_PLANE_CONTROL: OnceLock<mpsc::Sender<HarnessCommandPlaneControl>> =
    OnceLock::new();
static HARNESS_COMMAND_PLANE_STATE: OnceLock<watch::Sender<HarnessCommandPlaneLifecycle>> =
    OnceLock::new();
static HARNESS_COMMAND_PLANE_STARTED: OnceLock<()> = OnceLock::new();
static HARNESS_COMMAND_PLANE_TASKS: OnceLock<UiTaskOwner> = OnceLock::new();
static HARNESS_COMMAND_LISTENER_STARTED: OnceLock<()> = OnceLock::new();
static HARNESS_COMMAND_LISTENER_TASKS: OnceLock<UiTaskOwner> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarnessCommandPlaneLifecycle {
    Booting,
    Ready { generation: u64 },
    Degraded,
    Stopped,
}

enum HarnessCommandPlaneControl {
    Activate {
        sender: HarnessCommandSender,
        ack: oneshot::Sender<u64>,
    },
    Deactivate {
        ack: oneshot::Sender<()>,
    },
    Submit {
        command: HarnessUiCommand,
        reply: oneshot::Sender<HarnessUiCommandReceipt>,
    },
    Accept {
        submission_id: String,
        operation: Option<HarnessUiOperationHandle>,
        value: Option<SemanticCommandValue>,
    },
    TrackPendingBinding {
        submission_id: String,
        operation: HarnessUiOperationHandle,
    },
    Reject {
        submission_id: String,
        reason: String,
    },
    CompletePendingBinding {
        instance_id: OperationInstanceId,
        binding: ChannelBindingWitness,
    },
    FailPendingBinding {
        instance_id: OperationInstanceId,
        reason: String,
    },
}

struct PendingBindingSubmission {
    reply: oneshot::Sender<HarnessUiCommandReceipt>,
    operation: HarnessUiOperationHandle,
}

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

fn harness_command_plane_tasks() -> &'static UiTaskOwner {
    HARNESS_COMMAND_PLANE_TASKS.get_or_init(UiTaskOwner::new)
}

fn harness_command_listener_tasks() -> &'static UiTaskOwner {
    HARNESS_COMMAND_LISTENER_TASKS.get_or_init(UiTaskOwner::new)
}

fn harness_command_plane_state_sender() -> &'static watch::Sender<HarnessCommandPlaneLifecycle> {
    HARNESS_COMMAND_PLANE_STATE.get_or_init(|| {
        let (state_tx, _state_rx) = watch::channel(HarnessCommandPlaneLifecycle::Booting);
        state_tx
    })
}

fn harness_command_plane_control() -> io::Result<&'static mpsc::Sender<HarnessCommandPlaneControl>>
{
    HARNESS_COMMAND_PLANE_CONTROL
        .get()
        .ok_or_else(|| io::Error::other("harness command plane owner not started"))
}

fn ensure_harness_command_plane_owner_started() {
    if HARNESS_COMMAND_PLANE_STARTED.get().is_some() {
        return;
    }

    let (control_tx, control_rx) = mpsc::channel(128);
    let _ = HARNESS_COMMAND_PLANE_CONTROL.set(control_tx);
    let state_tx = harness_command_plane_state_sender().clone();
    harness_command_plane_tasks().spawn(async move {
        drive_harness_command_plane(control_rx, state_tx).await;
    });
    let _ = HARNESS_COMMAND_PLANE_STARTED.set(());
}

pub(crate) async fn ensure_harness_command_listener() -> io::Result<()> {
    ensure_harness_command_plane_owner_started();
    if HARNESS_COMMAND_LISTENER_STARTED.get().is_some() {
        return Ok(());
    }
    let Some((listener, guard)) = bind_harness_command_listener()? else {
        return Ok(());
    };
    let control_tx = harness_command_plane_control()?.clone();
    harness_command_listener_tasks().spawn(async move {
        let _guard = guard;
        forward_harness_commands_from_listener(listener, control_tx).await;
        tracing::debug!("harness command listener exited");
    });
    let _ = HARNESS_COMMAND_LISTENER_STARTED.set(());
    Ok(())
}

pub(crate) async fn register_harness_command_sender(
    sender: HarnessCommandSender,
) -> io::Result<u64> {
    ensure_harness_command_plane_owner_started();
    let (ack_tx, ack_rx) = oneshot::channel();
    harness_command_plane_control()?
        .send(HarnessCommandPlaneControl::Activate {
            sender,
            ack: ack_tx,
        })
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "failed to activate harness command plane ingress: {error}"
            ))
        })?;
    ack_rx.await.map_err(|error| {
        io::Error::other(format!(
            "harness command plane dropped activation acknowledgement: {error}"
        ))
    })
}

pub(crate) async fn clear_harness_command_sender() -> io::Result<()> {
    if HARNESS_COMMAND_PLANE_STARTED.get().is_none() {
        return Ok(());
    }
    let (ack_tx, ack_rx) = oneshot::channel();
    harness_command_plane_control()?
        .send(HarnessCommandPlaneControl::Deactivate { ack: ack_tx })
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "failed to deactivate harness command plane ingress: {error}"
            ))
        })?;
    ack_rx.await.map_err(|error| {
        io::Error::other(format!(
            "harness command plane dropped deactivation acknowledgement: {error}"
        ))
    })
}

async fn settle_harness_command_plane(
    control: HarnessCommandPlaneControl,
) -> io::Result<()> {
    ensure_harness_command_plane_owner_started();
    harness_command_plane_control()?
        .send(control)
        .await
        .map_err(|error| io::Error::other(format!(
            "failed to reach harness command plane owner: {error}"
        )))
}

pub(crate) async fn accept_harness_command_submission(
    submission_id: String,
    operation: Option<HarnessUiOperationHandle>,
    value: Option<SemanticCommandValue>,
) -> io::Result<()> {
    settle_harness_command_plane(HarnessCommandPlaneControl::Accept {
        submission_id,
        operation,
        value,
    })
    .await
}

pub(crate) async fn track_pending_binding_submission(
    submission_id: String,
    operation: HarnessUiOperationHandle,
) -> io::Result<()> {
    settle_harness_command_plane(HarnessCommandPlaneControl::TrackPendingBinding {
        submission_id,
        operation,
    })
    .await
}

pub(crate) async fn reject_harness_command_submission(
    submission_id: String,
    reason: String,
) -> io::Result<()> {
    settle_harness_command_plane(HarnessCommandPlaneControl::Reject {
        submission_id,
        reason,
    })
    .await
}

pub(crate) async fn complete_pending_binding_submission(
    instance_id: OperationInstanceId,
    binding: ChannelBindingWitness,
) -> io::Result<()> {
    settle_harness_command_plane(HarnessCommandPlaneControl::CompletePendingBinding {
        instance_id,
        binding,
    })
    .await
}

pub(crate) async fn fail_pending_binding_submission(
    instance_id: OperationInstanceId,
    reason: String,
) -> io::Result<()> {
    settle_harness_command_plane(HarnessCommandPlaneControl::FailPendingBinding {
        instance_id,
        reason,
    })
    .await
}

pub(crate) fn authoritative_harness_snapshot_readiness(
    should_exit: bool,
    pending_runtime_bootstrap: bool,
) -> aura_app::ui::contract::UiReadiness {
    if should_exit || pending_runtime_bootstrap {
        aura_app::ui::contract::UiReadiness::Loading
    } else {
        aura_app::ui::contract::UiReadiness::Ready
    }
}

async fn drive_harness_command_plane(
    mut control_rx: mpsc::Receiver<HarnessCommandPlaneControl>,
    state_tx: watch::Sender<HarnessCommandPlaneLifecycle>,
) {
    let mut generation = 0u64;
    let mut next_submission_id = 0u64;
    let mut active_sender: Option<(u64, HarnessCommandSender)> = None;
    let mut pending_submissions: Vec<(HarnessUiCommand, oneshot::Sender<HarnessUiCommandReceipt>)> =
        Vec::new();
    let mut pending_replies: HashMap<String, oneshot::Sender<HarnessUiCommandReceipt>> =
        HashMap::new();
    let mut pending_bindings: HashMap<String, PendingBindingSubmission> = HashMap::new();

    fn reject_reply(
        reply: oneshot::Sender<HarnessUiCommandReceipt>,
        reason: impl Into<String>,
    ) {
        let _ = reply.send(HarnessUiCommandReceipt::Rejected {
            reason: reason.into(),
        });
    }

    let submit_to_active_sender =
        |generation: u64,
         next_submission_id: &mut u64,
         active_sender: &Option<(u64, HarnessCommandSender)>,
         pending_replies: &mut HashMap<String, oneshot::Sender<HarnessUiCommandReceipt>>,
         command: HarnessUiCommand,
         reply: oneshot::Sender<HarnessUiCommandReceipt>| {
            let Some((_, sender)) = active_sender.as_ref().cloned() else {
                return Err((command, reply));
            };
            let submission_id = format!("submission-{generation}-{next_submission_id}");
            *next_submission_id = next_submission_id.saturating_add(1);
            pending_replies.insert(submission_id.clone(), reply);
            Ok((sender, HarnessCommandSubmission { submission_id, command }))
        };

    while let Some(control) = control_rx.recv().await {
        match control {
            HarnessCommandPlaneControl::Activate { sender, ack } => {
                generation = generation.saturating_add(1);
                active_sender = Some((generation, sender));
                let _ = state_tx.send(HarnessCommandPlaneLifecycle::Ready { generation });
                let _ = ack.send(generation);
                while !pending_submissions.is_empty() {
                    let (command, reply) = pending_submissions.remove(0);
                    match submit_to_active_sender(
                        generation,
                        &mut next_submission_id,
                        &active_sender,
                        &mut pending_replies,
                        command,
                        reply,
                    ) {
                        Ok((sender, submission)) => {
                            let submission_id = submission.submission_id.clone();
                            if let Err(error) = sender.send(submission).await {
                                active_sender = None;
                                let _ = state_tx.send(HarnessCommandPlaneLifecycle::Degraded);
                                if let Some(reply) = pending_replies.remove(&submission_id) {
                                    reject_reply(
                                        reply,
                                        format!(
                                            "failed to submit harness command into shell ingress: {error}"
                                        ),
                                    );
                                }
                                break;
                            }
                        }
                        Err((command, reply)) => {
                            pending_submissions.push((command, reply));
                            break;
                        }
                    }
                }
            }
            HarnessCommandPlaneControl::Deactivate { ack } => {
                active_sender = None;
                let _ = state_tx.send(HarnessCommandPlaneLifecycle::Booting);
                let _ = ack.send(());
            }
            HarnessCommandPlaneControl::Submit { command, reply } => {
                match submit_to_active_sender(
                    generation,
                    &mut next_submission_id,
                    &active_sender,
                    &mut pending_replies,
                    command,
                    reply,
                ) {
                    Ok((sender, submission)) => {
                        let submission_id = submission.submission_id.clone();
                        if let Err(error) = sender.send(submission).await {
                            active_sender = None;
                            let _ = state_tx.send(HarnessCommandPlaneLifecycle::Degraded);
                            if let Some(reply) = pending_replies.remove(&submission_id) {
                                reject_reply(
                                    reply,
                                    format!(
                                        "failed to submit harness command into shell ingress: {error}"
                                    ),
                                );
                            }
                        }
                    }
                    Err((command, reply)) => pending_submissions.push((command, reply)),
                }
            }
            HarnessCommandPlaneControl::Accept {
                submission_id,
                operation,
                value,
            } => {
                if let Some(reply) = pending_replies.remove(&submission_id) {
                    let receipt = match operation {
                        Some(operation) => HarnessUiCommandReceipt::AcceptedWithOperation {
                            operation,
                            value,
                        },
                        None => HarnessUiCommandReceipt::Accepted { value },
                    };
                    let _ = reply.send(receipt);
                }
            }
            HarnessCommandPlaneControl::TrackPendingBinding {
                submission_id,
                operation,
            } => {
                if let Some(reply) = pending_replies.remove(&submission_id) {
                    pending_bindings.insert(
                        operation.instance_id().0.clone(),
                        PendingBindingSubmission { reply, operation },
                    );
                }
            }
            HarnessCommandPlaneControl::Reject {
                submission_id,
                reason,
            } => {
                if let Some(reply) = pending_replies.remove(&submission_id) {
                    reject_reply(reply, reason);
                }
            }
            HarnessCommandPlaneControl::CompletePendingBinding {
                instance_id,
                binding,
            } => {
                if let Some(pending) = pending_bindings.remove(&instance_id.0) {
                    let _ = pending.reply.send(HarnessUiCommandReceipt::AcceptedWithOperation {
                        operation: pending.operation,
                        value: Some(binding.semantic_value()),
                    });
                }
            }
            HarnessCommandPlaneControl::FailPendingBinding { instance_id, reason } => {
                if let Some(pending) = pending_bindings.remove(&instance_id.0) {
                    reject_reply(pending.reply, reason);
                }
            }
        }
    }

    for (_command, reply) in pending_submissions.drain(..) {
        reject_reply(
            reply,
            "harness command plane stopped before command ownership resumed",
        );
    }
    for (_submission_id, reply) in pending_replies.drain() {
        reject_reply(
            reply,
            "harness command plane stopped before terminal settlement",
        );
    }
    for (_instance_id, pending) in pending_bindings.drain() {
        reject_reply(
            pending.reply,
            "harness command plane stopped before authoritative binding settlement",
        );
    }
    let _ = state_tx.send(HarnessCommandPlaneLifecycle::Stopped);
}

async fn forward_harness_commands_from_listener(
    listener: UnixListener,
    control_tx: mpsc::Sender<HarnessCommandPlaneControl>,
) {
    loop {
        let Ok((stream, _addr)) = listener.accept().await else {
            break;
        };
        if !process_harness_command_stream(stream, &control_tx).await {
            break;
        }
    }
}

#[cfg(test)]
pub(super) async fn forward_test_harness_commands_from_listener(listener: UnixListener) {
    ensure_harness_command_plane_owner_started();
    let control_tx = harness_command_plane_control()
        .unwrap_or_else(|error| panic!("failed to get harness command plane control: {error}"))
        .clone();
    forward_harness_commands_from_listener(listener, control_tx).await;
}

/// Per-connection timeout for reading a harness command payload.
const HARNESS_COMMAND_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const HARNESS_COMMAND_SUBMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn harness_timeout_profile() -> TimeoutExecutionProfile {
    TimeoutExecutionProfile::harness()
}

fn scaled_harness_duration(base: std::time::Duration) -> io::Result<std::time::Duration> {
    harness_timeout_profile()
        .scale_duration(base)
        .map_err(|error| io::Error::other(format!("invalid harness timeout policy: {error}")))
}

async fn harness_budget(
    time: &PhysicalTimeHandler,
    timeout: std::time::Duration,
) -> io::Result<TimeoutBudget> {
    let started_at = time
        .physical_time()
        .await
        .map_err(|error| io::Error::other(format!("failed to read physical time: {error}")))?;
    TimeoutBudget::from_start_and_timeout(&started_at, scaled_harness_duration(timeout)?)
        .map_err(|error| io::Error::other(format!("failed to create harness budget: {error}")))
}

fn timeout_error_reason(error: TimeoutBudgetError, context: &str) -> String {
    format!("{context} timed out: {error}")
}

async fn process_harness_command_stream(
    mut stream: UnixStream,
    control_tx: &mpsc::Sender<HarnessCommandPlaneControl>,
) -> bool {
    let mut payload = Vec::new();
    let time = PhysicalTimeHandler::new();
    let read_budget = match harness_budget(&time, HARNESS_COMMAND_READ_TIMEOUT).await {
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
    let submit_budget = match harness_budget(&time, HARNESS_COMMAND_SUBMISSION_TIMEOUT).await {
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
    let receipt = match execute_with_timeout_budget(&time, &submit_budget, || {
        let control_tx = control_tx.clone();
        let command = command.clone();
        async move {
            let (reply_tx, reply_rx) = oneshot::channel();
            control_tx
                .send(HarnessCommandPlaneControl::Submit {
                    command,
                    reply: reply_tx,
                })
                .await
                .map_err(|error| {
                    io::Error::other(format!(
                        "failed to reach harness command plane owner: {error}"
                    ))
                })?;
            reply_rx.await.map_err(|error| {
                io::Error::other(format!(
                    "harness command plane dropped submission receipt: {error}"
                ))
            })
        }
    })
    .await
    {
        Ok(receipt) => receipt,
        Err(TimeoutRunError::Timeout(error)) => HarnessUiCommandReceipt::Rejected {
            reason: timeout_error_reason(error, "harness command submission"),
        },
        Err(TimeoutRunError::Operation(error)) => HarnessUiCommandReceipt::Rejected {
            reason: error.to_string(),
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
