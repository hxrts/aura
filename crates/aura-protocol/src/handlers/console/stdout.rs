//! Stdout console handler for simple logging

use crate::effects::{ConsoleEffects, ConsoleEffect};
use async_trait::async_trait;
use uuid::Uuid;

/// Stdout console handler for simple text output
pub struct StdoutConsoleHandler;

impl StdoutConsoleHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdoutConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConsoleEffects for StdoutConsoleHandler {
    async fn emit_choreo_event(&self, event: ConsoleEffect) {
        println!("[CHOREO] {:?}", event);
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        println!("[PROTOCOL] Started {} ({})", protocol_type, protocol_id);
    }

    async fn protocol_completed(&self, protocol_id: Uuid, duration_ms: u64) {
        println!("[PROTOCOL] Completed {} in {}ms", protocol_id, duration_ms);
    }

    async fn protocol_failed(&self, protocol_id: Uuid, error: &str) {
        println!("[PROTOCOL] Failed {} - {}", protocol_id, error);
    }

    async fn log_info(&self, message: &str) {
        println!("[INFO] {}", message);
    }

    async fn log_warning(&self, message: &str) {
        println!("[WARN] {}", message);
    }

    async fn log_error(&self, message: &str) {
        println!("[ERROR] {}", message);
    }

    async fn flush(&self) {
        // stdout flushes automatically
    }
}