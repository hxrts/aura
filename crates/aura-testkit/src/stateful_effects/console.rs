//! Mock console effect handlers for testing
//!
//! This module contains the stateful MockConsoleHandler that was moved from aura-effects
//! to fix architectural violations. The handler uses Arc<Mutex<>> to capture log messages
//! for verification in tests.

use aura_core::{effects::ConsoleEffects, AuraError};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Mock console handler for capturing logs in tests
#[derive(Debug, Clone)]
pub struct MockConsoleHandler {
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
}

impl Default for MockConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockConsoleHandler {
    /// Create a new mock console handler
    pub fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Get all captured log messages (for testing)
    pub fn get_logs(&self) -> Vec<(String, String)> {
        let logs = self.logs.lock().unwrap();
        logs.iter().cloned().collect()
    }

    /// Get logs of a specific level (for testing)
    pub fn get_logs_with_level(&self, level: &str) -> Vec<String> {
        let logs = self.logs.lock().unwrap();
        logs.iter()
            .filter(|(l, _)| l == level)
            .map(|(_, m)| m.clone())
            .collect()
    }

    /// Clear all captured logs (for testing)
    pub fn clear_logs(&self) {
        self.logs.lock().unwrap().clear();
    }

    /// Get the number of log entries (for testing)
    pub fn log_count(&self) -> usize {
        self.logs.lock().unwrap().len()
    }

    fn add_log(&self, level: &str, message: &str) {
        let mut logs = self.logs.lock().unwrap();
        logs.push_back((level.to_string(), message.to_string()));
    }
}

#[async_trait]
impl ConsoleEffects for MockConsoleHandler {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        self.add_log("INFO", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        self.add_log("WARN", message);
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        self.add_log("ERROR", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        self.add_log("DEBUG", message);
        Ok(())
    }
}