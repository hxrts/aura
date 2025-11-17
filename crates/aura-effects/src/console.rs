//! Console effect handlers
//!
//! This module provides standard implementations of the `ConsoleEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_core::{effects::ConsoleEffects, AuraError};
use aura_macros::aura_effect_handlers;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// Generate both mock and real console handlers using the macro
aura_effect_handlers! {
    trait_name: ConsoleEffects,
    mock: {
        struct_name: MockConsoleHandler,
        state: {
            logs: Arc<Mutex<VecDeque<(String, String)>>>,
        },
        features: {
            async_trait: true,
            deterministic: true,
        },
        methods: {
            log_info(message: &str) -> Result<(), AuraError> => {
                self.add_log("INFO", message);
                Ok(())
            },
            log_warn(message: &str) -> Result<(), AuraError> => {
                self.add_log("WARN", message);
                Ok(())
            },
            log_error(message: &str) -> Result<(), AuraError> => {
                self.add_log("ERROR", message);
                Ok(())
            },
            log_debug(message: &str) -> Result<(), AuraError> => {
                self.add_log("DEBUG", message);
                Ok(())
            },
        },
    },
    real: {
        struct_name: RealConsoleHandler,
        features: {
            async_trait: true,
        },
        methods: {
            log_info(message: &str) -> Result<(), AuraError> => {
                tracing::info!("{}", message);
                Ok(())
            },
            log_warn(message: &str) -> Result<(), AuraError> => {
                tracing::warn!("{}", message);
                Ok(())
            },
            log_error(message: &str) -> Result<(), AuraError> => {
                tracing::error!("{}", message);
                Ok(())
            },
            log_debug(message: &str) -> Result<(), AuraError> => {
                tracing::debug!("{}", message);
                Ok(())
            },
        },
    },
}

impl MockConsoleHandler {
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
