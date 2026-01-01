//! Runtime-owned OTA update state.

use super::state::with_state_mut_validated;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Update status for the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// No update available.
    UpToDate,
    /// Update available but not yet downloaded.
    Available {
        version: String,
        release_notes: Option<String>,
        size_bytes: u64,
    },
    /// Update is being downloaded.
    Downloading {
        version: String,
        progress_percent: u8,
    },
    /// Update downloaded and verified, ready to install.
    Ready { version: String },
    /// Update is being installed.
    Installing { version: String },
    /// Update failed.
    Failed { reason: String },
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self::UpToDate
    }
}

#[derive(Debug, Default)]
struct OtaState {
    status: UpdateStatus,
}

impl OtaState {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OtaManager {
    state: Arc<RwLock<OtaState>>,
}

impl OtaManager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn status(&self) -> UpdateStatus {
        self.state.read().await.status.clone()
    }

    pub(crate) async fn set_status(&self, status: UpdateStatus) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.status = status;
            },
            |state| state.validate(),
        )
        .await;
    }
}
