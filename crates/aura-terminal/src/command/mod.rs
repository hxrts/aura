//! Command execution scaffolding shared by the `aura` CLI.
//!
//! - [`error`]: typed failures ([`CommandError`]) and their exit codes.
//! - [`OutputMode`]: text or `--json` rendering of a command's outcome.
//! - [`confirm`]: the `--yes` / interactive confirmation for destructive
//!   commands.
//! - [`with_timeout`]: the `--timeout` deadline, measured on the runtime's
//!   own clock.

pub mod error;

pub use error::{CommandError, ErrorCode};

use crate::handlers::CliOutput;
use async_lock::RwLock;
use aura_app::ui::types::AppCore;
use aura_app::ui::workflows::runtime::{
    execute_with_runtime_timeout_budget, require_runtime, workflow_timeout_budget,
};
use aura_core::TimeoutRunError;
use serde_json::json;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// How a command reports its outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable text on stdout, errors on stderr.
    Text,
    /// One JSON document on stdout: `{"ok":true,"result":..}` or
    /// `{"ok":false,"error":{"code":..,"message":..}}`.
    Json,
}

impl OutputMode {
    /// Pick the mode from the global `--json` flag.
    #[must_use]
    pub fn from_json_flag(json: bool) -> Self {
        if json {
            Self::Json
        } else {
            Self::Text
        }
    }

    /// The success document for `output` under `--json`.
    #[must_use]
    pub fn success_document(output: &CliOutput) -> serde_json::Value {
        json!({ "ok": true, "result": output.to_json() })
    }

    /// The failure document for `error` under `--json`.
    #[must_use]
    pub fn failure_document(error: &CommandError) -> serde_json::Value {
        json!({ "ok": false, "error": error })
    }

    /// Print a successful command's output.
    pub fn emit_success(self, output: &CliOutput) {
        match self {
            Self::Text => output.render(),
            Self::Json => println!("{}", Self::success_document(output)),
        }
    }

    /// Print a failure; `verbose` adds the raw error chain in text mode.
    pub fn emit_failure(self, error: &CommandError, verbose: bool) {
        match self {
            Self::Text => {
                eprintln!("error: {}", error.message);
                if let (true, Some(detail)) = (verbose, &error.detail) {
                    eprintln!("detail: {detail}");
                }
            }
            Self::Json => println!("{}", Self::failure_document(error)),
        }
    }
}

/// Confirm a destructive action: `--yes` confirms; otherwise ask on an
/// interactive terminal, and refuse when stdin is not a terminal.
pub fn confirm(prompt: &str, assume_yes: bool) -> Result<(), CommandError> {
    use std::io::{BufRead, IsTerminal, Write};
    if assume_yes {
        return Ok(());
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(CommandError::invalid(format!(
            "{prompt}: confirmation required; pass --yes to run non-interactively"
        )));
    }
    eprint!("{prompt} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    stdin
        .lock()
        .read_line(&mut answer)
        .map_err(|e| CommandError::invalid(format!("read confirmation: {e}")))?;
    if matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
        Ok(())
    } else {
        Err(CommandError::invalid("Cancelled"))
    }
}

/// Run `operation`, failing with [`ErrorCode::Timeout`] when it does not
/// finish within `timeout` on the runtime's clock. `None` waits without a
/// deadline.
pub async fn with_timeout<T, F, Fut>(
    app_core: &Arc<RwLock<AppCore>>,
    timeout: Option<Duration>,
    operation: F,
) -> Result<T, CommandError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, CommandError>>,
{
    let Some(timeout) = timeout else {
        return operation().await;
    };
    let runtime = require_runtime(app_core).await?;
    let budget = workflow_timeout_budget(&runtime, timeout)
        .await
        .map_err(|e| CommandError::from(aura_core::AuraError::from(e)))?;
    match execute_with_runtime_timeout_budget(&runtime, &budget, operation).await {
        Ok(value) => Ok(value),
        Err(TimeoutRunError::Operation(error)) => Err(error),
        Err(TimeoutRunError::Timeout(_)) => Err(CommandError::timeout(timeout.as_secs())),
    }
}
