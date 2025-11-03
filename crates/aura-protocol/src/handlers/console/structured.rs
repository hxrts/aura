//! Structured console handler for JSON logging

use crate::effects::{ConsoleEffects, ConsoleEffect};
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

/// Structured console handler for JSON output
pub struct StructuredConsoleHandler;

impl StructuredConsoleHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StructuredConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConsoleEffects for StructuredConsoleHandler {
    async fn emit_choreo_event(&self, event: ConsoleEffect) {
        let log_entry = json!({
            "type": "choreo_event",
            "event": format!("{:?}", event),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        let log_entry = json!({
            "type": "protocol_started",
            "protocol_id": protocol_id,
            "protocol_type": protocol_type,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn protocol_completed(&self, protocol_id: Uuid, duration_ms: u64) {
        let log_entry = json!({
            "type": "protocol_completed",
            "protocol_id": protocol_id,
            "duration_ms": duration_ms,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn protocol_failed(&self, protocol_id: Uuid, error: &str) {
        let log_entry = json!({
            "type": "protocol_failed",
            "protocol_id": protocol_id,
            "error": error,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn log_info(&self, message: &str) {
        let log_entry = json!({
            "level": "info",
            "message": message,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn log_warning(&self, message: &str) {
        let log_entry = json!({
            "level": "warning",
            "message": message,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn log_error(&self, message: &str) {
        let log_entry = json!({
            "level": "error",
            "message": message,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    async fn flush(&self) {
        // stdout flushes automatically
    }
}