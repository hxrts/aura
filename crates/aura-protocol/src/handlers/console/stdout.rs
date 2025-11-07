//! Stdout console handler for simple logging

use crate::effects::{ConsoleEffects, ConsoleEvent};
use std::future::Future;

/// Stdout console handler for simple text output
pub struct StdoutConsoleHandler;

impl StdoutConsoleHandler {
    /// Create a new stdout console handler
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdoutConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleEffects for StdoutConsoleHandler {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        let fields_str = format_fields(fields);
        println!("[TRACE] {} {}", message, fields_str);
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        let fields_str = format_fields(fields);
        println!("[DEBUG] {} {}", message, fields_str);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        let fields_str = format_fields(fields);
        println!("[INFO] {} {}", message, fields_str);
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        let fields_str = format_fields(fields);
        println!("[WARN] {} {}", message, fields_str);
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        let fields_str = format_fields(fields);
        println!("[ERROR] {} {}", message, fields_str);
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            println!("[EVENT] {:?}", event);
        })
    }
}

fn format_fields(fields: &[(&str, &str)]) -> String {
    if fields.is_empty() {
        String::new()
    } else {
        let field_strings: Vec<String> =
            fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        format!("[{}]", field_strings.join(" "))
    }
}
