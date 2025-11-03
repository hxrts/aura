//! Silent console handler for testing

use crate::effects::{ConsoleEffects, ConsoleEffect};
use async_trait::async_trait;
use uuid::Uuid;

/// Silent console handler that discards all output
pub struct SilentConsoleHandler;

impl SilentConsoleHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SilentConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConsoleEffects for SilentConsoleHandler {
    async fn emit_choreo_event(&self, _event: ConsoleEffect) {
        // Silent - do nothing
    }

    async fn protocol_started(&self, _protocol_id: Uuid, _protocol_type: &str) {
        // Silent - do nothing
    }

    async fn protocol_completed(&self, _protocol_id: Uuid, _duration_ms: u64) {
        // Silent - do nothing
    }

    async fn protocol_failed(&self, _protocol_id: Uuid, _error: &str) {
        // Silent - do nothing
    }

    async fn log_info(&self, _message: &str) {
        // Silent - do nothing
    }

    async fn log_warning(&self, _message: &str) {
        // Silent - do nothing
    }

    async fn log_error(&self, _message: &str) {
        // Silent - do nothing
    }

    async fn flush(&self) {
        // Silent - do nothing
    }
}