//! Silent console handler for testing

use crate::effects::{ConsoleEffects, ConsoleEvent};
use std::future::Future;

/// Silent console handler that discards all output
pub struct SilentConsoleHandler;

impl SilentConsoleHandler {
    /// Create a new silent console handler
    pub fn new() -> Self {
        Self
    }
}

impl Default for SilentConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleEffects for SilentConsoleHandler {
    fn log_trace(&self, _message: &str, _fields: &[(&str, &str)]) {
        // Silent - do nothing
    }

    fn log_debug(&self, _message: &str, _fields: &[(&str, &str)]) {
        // Silent - do nothing
    }

    fn log_info(&self, _message: &str, _fields: &[(&str, &str)]) {
        // Silent - do nothing
    }

    fn log_warn(&self, _message: &str, _fields: &[(&str, &str)]) {
        // Silent - do nothing
    }

    fn log_error(&self, _message: &str, _fields: &[(&str, &str)]) {
        // Silent - do nothing
    }

    fn emit_event(
        &self,
        _event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            // Silent - do nothing
        })
    }
}
