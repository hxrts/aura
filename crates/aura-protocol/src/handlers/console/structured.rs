//! Structured console handler for JSON logging

use crate::effects::{ConsoleEffects, ConsoleEvent};
use chrono;
use serde_json::json;
use std::future::Future;

/// Structured console handler for JSON output
pub struct StructuredConsoleHandler;

impl StructuredConsoleHandler {
    /// Create a new structured console handler
    pub fn new() -> Self {
        Self
    }
}

impl Default for StructuredConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleEffects for StructuredConsoleHandler {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        let field_map: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();

        let log_entry = json!({
            "level": "trace",
            "message": message,
            "fields": field_map,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        let field_map: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();

        let log_entry = json!({
            "level": "debug",
            "message": message,
            "fields": field_map,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        let field_map: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();

        let log_entry = json!({
            "level": "info",
            "message": message,
            "fields": field_map,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        let field_map: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();

        let log_entry = json!({
            "level": "warn",
            "message": message,
            "fields": field_map,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        let field_map: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();

        let log_entry = json!({
            "level": "error",
            "message": message,
            "fields": field_map,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        println!("{}", log_entry);
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let log_entry = json!({
                "type": "console_event",
                "event": format!("{:?}", event),
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            println!("{}", log_entry);
        })
    }
}
