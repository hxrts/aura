#![allow(missing_docs)]

use std::io::{self, BufRead, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use aura_harness::config::require_existing_file;
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::load_and_validate_run_config;
use aura_harness::scenario::ScenarioRunner;
use aura_harness::scenario_execution::execute_with_run_budgets;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "aura-harness-tool-repl")]
#[command(about = "Persistent ToolApi REPL for manual harness operation")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    /// Owner-only socket used for requests whose responses must not enter the
    /// retained public REPL output.
    #[arg(long, requires = "config")]
    private_socket: Option<PathBuf>,
    /// Send one stdin request to an existing owner's private socket.
    #[arg(long, conflicts_with = "config")]
    private_request_socket: Option<PathBuf>,
    /// Optional compatibility scenario TOML to execute before entering interactive mode.
    #[arg(long)]
    prelude: Option<PathBuf>,
    /// Auto-shutdown after this many milliseconds without incoming requests.
    /// Set to 0 to disable idle timeout.
    #[arg(long, default_value_t = 600_000)]
    idle_timeout_ms: u64,
    /// Require every request line to include an `id` field.
    #[arg(long, default_value_t = false)]
    require_request_id: bool,
    /// Enforce strictly increasing numeric request ids.
    #[arg(long, default_value_t = false)]
    strict_request_id_order: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplRequestEnvelope {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(flatten)]
    request: ToolRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplResponseEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(flatten)]
    response: ToolResponse,
}

enum IncomingRequest {
    Public(Result<String, io::Error>),
    Private(Result<String, io::Error>, UnixStream),
}

fn private_client(path: &PathBuf) -> Result<()> {
    let mut request = String::new();
    io::stdin()
        .lock()
        .take(1_048_577)
        .read_to_string(&mut request)?;
    anyhow::ensure!(request.len() <= 1_048_576, "private request exceeds limit");
    anyhow::ensure!(
        request.lines().count() == 1,
        "private request must be one JSON line"
    );
    let stream = UnixStream::connect(path).context("connect private harness owner")?;
    let response = exchange_private_stream(stream, &request)?;
    io::stdout().write_all(response.as_bytes())?;
    Ok(())
}

fn exchange_private_stream(mut stream: UnixStream, request: &str) -> Result<String> {
    stream.write_all(request.trim_end_matches('\n').as_bytes())?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    io::BufReader::new(stream).read_line(&mut response)?;
    anyhow::ensure!(
        !response.is_empty(),
        "private harness owner closed without a response"
    );
    Ok(response)
}

fn write_response(
    stdout: &mut impl Write,
    private_reply: Option<&mut UnixStream>,
    response: &ReplResponseEnvelope,
) -> Result<()> {
    let encoded = serde_json::to_string(response)?;
    if let Some(reply) = private_reply {
        writeln!(reply, "{encoded}")?;
        reply.flush()?;
    } else {
        writeln!(stdout, "{encoded}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn validate_private_socket_path(path: &PathBuf) -> Result<()> {
    let parent = path
        .parent()
        .context("private socket needs a parent directory")?;
    let metadata = std::fs::symlink_metadata(parent)?;
    anyhow::ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.permissions().mode() & 0o777 == 0o700,
        "private socket parent must be an owner-only directory"
    );
    anyhow::ensure!(
        !path.exists() && !path.is_symlink(),
        "private socket already exists"
    );
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(path) = &cli.private_request_socket {
        return private_client(path);
    }
    let config_path = cli
        .config
        .as_ref()
        .context("--config is required for the REPL owner")?;
    require_existing_file(config_path, "run config")?;
    let config = load_and_validate_run_config(config_path)?;
    if let Some(path) = &cli.private_socket {
        validate_private_socket_path(path)?;
    }
    let prelude = if let Some(path) = &cli.prelude {
        Some(ScenarioRunner::load_and_validate(path)?)
    } else {
        None
    };

    let coordinator = HarnessCoordinator::from_run_config(&config)?;
    let mut tool_api = ToolApi::new(coordinator);
    tool_api.start_all()?;

    if let Some(scenario) = &prelude {
        if let Err(error) = execute_with_run_budgets(&config, scenario, &mut tool_api) {
            let _ = tool_api.stop_all();
            return Err(error);
        }
        eprintln!("prelude_complete scenario_id={}", scenario.id);
    }

    let mut stdout = io::stdout();
    let idle_timeout = if cli.idle_timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(cli.idle_timeout_ms))
    };
    let poll_interval = Duration::from_millis(250);
    let mut last_activity = Instant::now();
    let mut last_request_id: Option<u64> = None;
    let shutdown_requested = Arc::new(AtomicBool::new(false));

    {
        let shutdown_requested = Arc::clone(&shutdown_requested);
        ctrlc::set_handler(move || {
            shutdown_requested.store(true, Ordering::SeqCst);
        })
        .with_context(|| "failed to install signal handler")?;
    }

    let (tx, rx) = mpsc::channel::<IncomingRequest>();
    if let Some(path) = &cli.private_socket {
        validate_private_socket_path(path)?;
        let listener = UnixListener::bind(path).context("bind private harness owner socket")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let tx = tx.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let line = stream.try_clone().and_then(|reader| {
                    let mut line = String::new();
                    io::BufReader::new(reader.take(1_048_577)).read_line(&mut line)?;
                    if line.len() > 1_048_576 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "private request exceeds limit",
                        ));
                    }
                    Ok(line)
                });
                if tx.send(IncomingRequest::Private(line, stream)).is_err() {
                    break;
                }
            }
        });
    }
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            if tx.send(IncomingRequest::Public(line)).is_err() {
                break;
            }
        }
    });

    loop {
        if shutdown_requested.load(Ordering::SeqCst) {
            eprintln!("shutdown signal received; stopping harness instances");
            break;
        }

        let (line, mut private_reply) = match rx.recv_timeout(poll_interval) {
            Ok(IncomingRequest::Public(line)) => {
                last_activity = Instant::now();
                (line.with_context(|| "failed to read stdin line")?, None)
            }
            Ok(IncomingRequest::Private(Ok(line), reply)) => {
                last_activity = Instant::now();
                (line, Some(reply))
            }
            Ok(IncomingRequest::Private(Err(_), mut reply)) => {
                let _ = writeln!(
                    reply,
                    "{{\"status\":\"error\",\"message\":\"invalid private request frame\"}}"
                );
                continue;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(timeout) = idle_timeout {
                    if last_activity.elapsed() >= timeout {
                        eprintln!(
                            "idle timeout reached ({} ms); shutting down harness instances",
                            cli.idle_timeout_ms
                        );
                        break;
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
            break;
        }

        let (request_id, response) = match serde_json::from_str::<ReplRequestEnvelope>(trimmed) {
            Ok(envelope) => {
                if cli.require_request_id && envelope.id.is_none() {
                    (
                        None,
                        ToolResponse::Error {
                            message: "request id is required by --require-request-id".to_string(),
                        },
                    )
                } else if cli.strict_request_id_order {
                    if let Some(raw_id) = envelope.id.clone() {
                        match raw_id.as_u64() {
                            Some(value) => {
                                if last_request_id.is_some_and(|previous| value <= previous) {
                                    (
                                        envelope.id,
                                        ToolResponse::Error {
                                            message: format!(
                                                "request id {value} is not strictly greater than previous id {}",
                                                last_request_id.unwrap_or(0)
                                            ),
                                        },
                                    )
                                } else {
                                    last_request_id = Some(value);
                                    let response = tool_api.handle_request(envelope.request);
                                    (envelope.id, response)
                                }
                            }
                            None => (
                                envelope.id,
                                ToolResponse::Error {
                                    message: "strict request id order requires numeric u64 ids"
                                        .to_string(),
                                },
                            ),
                        }
                    } else {
                        (
                            None,
                            ToolResponse::Error {
                                message: "request id is required by --strict-request-id-order"
                                    .to_string(),
                            },
                        )
                    }
                } else {
                    let response = tool_api.handle_request(envelope.request);
                    (envelope.id, response)
                }
            }
            // Echo the caller's id when the line is JSON at all, so a driver
            // waiting on that id sees the error instead of timing out.
            Err(error) => (
                serde_json::from_str::<serde_json::Value>(trimmed)
                    .ok()
                    .and_then(|value| value.get("id").cloned()),
                ToolResponse::Error {
                    message: format!("invalid ToolRequest JSON: {error}"),
                },
            ),
        };

        let response = ReplResponseEnvelope {
            id: request_id,
            response,
        };

        write_response(&mut stdout, private_reply.as_mut(), &response)?;
    }

    tool_api.stop_all()?;
    if let Some(path) = &cli.private_socket {
        std::fs::remove_file(path).context("remove private harness owner socket")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_response_never_reaches_public_repl_output() {
        let (mut owner, recipient) = UnixStream::pair().expect("private socket pair");
        let response = ReplResponseEnvelope {
            id: Some(serde_json::json!("private-fixture")),
            response: ToolResponse::Error {
                message: "secret-enrollment-code-fixture".to_owned(),
            },
        };
        let mut public = Vec::new();
        write_response(&mut public, Some(&mut owner), &response).expect("private response");
        assert!(public.is_empty());
        let mut line = String::new();
        io::BufReader::new(recipient)
            .read_line(&mut line)
            .expect("read private response");
        assert!(line.contains("secret-enrollment-code-fixture"));
    }

    #[test]
    fn private_client_frame_uses_only_the_socket() {
        let (client, mut owner) = UnixStream::pair().expect("private socket pair");
        let owner_task = thread::spawn(move || {
            let mut request = String::new();
            io::BufReader::new(owner.try_clone().expect("clone owner"))
                .read_line(&mut request)
                .expect("read private request");
            assert_eq!(request, "private-enrollment-code-fixture\n");
            owner
                .write_all(b"{\"status\":\"ok\",\"payload\":{\"setup_code\":\"private-setup-code-fixture\"}}\n")
                .expect("write private response");
        });
        let response = exchange_private_stream(client, "private-enrollment-code-fixture\n")
            .expect("private exchange");
        assert!(response.contains("private-setup-code-fixture"));
        owner_task.join().expect("owner task");
    }

    #[test]
    fn private_socket_requires_unshared_fresh_directory() {
        let root = tempfile::tempdir().expect("temporary root");
        let owner = root.path().join("owner");
        std::fs::create_dir(&owner).expect("owner directory");
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700))
            .expect("owner mode");
        let socket = owner.join("owner.sock");
        validate_private_socket_path(&socket).expect("fresh owner-only socket");
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o755))
            .expect("shared mode");
        assert!(validate_private_socket_path(&socket).is_err());
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700))
            .expect("restore owner mode");
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&owner, &alias).expect("symlink parent");
        assert!(validate_private_socket_path(&alias.join("owner.sock")).is_err());
        std::fs::write(&socket, b"existing").expect("existing destination");
        assert!(validate_private_socket_path(&socket).is_err());
    }
}
