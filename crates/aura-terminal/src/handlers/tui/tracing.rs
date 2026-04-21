use std::io;
use std::sync::{Arc, OnceLock};

use aura_core::effects::StorageCoreEffects;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::env::{tui_allows_stdio, tui_log_path_override};
use crate::tui::tasks::UiTaskOwner;

use super::{TuiMode, MAX_TUI_LOG_BYTES, TUI_LOG_KEY_PREFIX, TUI_LOG_QUEUE_CAPACITY};

static TUI_TRACING_TASKS: OnceLock<UiTaskOwner> = OnceLock::new();

fn tui_tracing_tasks() -> &'static UiTaskOwner {
    TUI_TRACING_TASKS.get_or_init(UiTaskOwner::new)
}

struct StorageLogWriter {
    sender: mpsc::Sender<Vec<u8>>,
}

impl io::Write for StorageLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.sender.try_send(buf.to_vec()) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn init_tui_tracing(storage: Arc<dyn StorageCoreEffects>, mode: TuiMode) {
    if tui_allows_stdio() {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_target(true)
            .with_writer(std::io::stderr)
            .try_init();
        return;
    }

    let default_name = mode.log_filename();
    let log_key =
        tui_log_path_override().unwrap_or_else(|| format!("{TUI_LOG_KEY_PREFIX}/{default_name}"));

    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(TUI_LOG_QUEUE_CAPACITY);
    let storage_task = storage.clone();
    let log_key_task = log_key;

    tui_tracing_tasks().spawn(async move {
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(chunk) = rx.recv().await {
            buffer.extend_from_slice(&chunk);
            if buffer.len() > MAX_TUI_LOG_BYTES {
                let excess = buffer.len() - MAX_TUI_LOG_BYTES;
                buffer.drain(0..excess);
            }
            if let Err(error) = storage_task.store(&log_key_task, buffer.clone()).await {
                tracing::warn!(error = %error, "Failed to persist TUI log chunk");
            }
        }
    });

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let make_writer = {
        let sender = tx;
        move || StorageLogWriter {
            sender: sender.clone(),
        }
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(make_writer)
        .try_init();
}
