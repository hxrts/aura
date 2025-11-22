//! Compatibility module for legacy handler patterns
//!
//! This module provides minimal compatibility types for handlers that haven't
//! been fully migrated to pure algebraic effects patterns yet.

use crate::types::{Result, SimulatorContext, SimulatorOperation};
use serde_json::Value;

/// Legacy handler trait for backward compatibility
pub trait SimulatorHandler: Send + Sync {
    /// Handle a simulator operation
    fn handle(&self, operation: SimulatorOperation, context: &SimulatorContext) -> Result<Value>;

    /// Get the name of this handler
    fn name(&self) -> &str;

    /// Check if this handler should handle the given operation
    fn handles(&self, _operation: &SimulatorOperation) -> bool {
        true // Default: handle all operations
    }
}

/// Performance metrics for legacy handlers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerformanceMetrics {
    /// Number of effects executed
    pub effects_executed: u64,
    /// Number of devices created
    pub devices_created: u64,
    /// Number of choreographies initialized
    pub choreographies_initialized: u64,
    /// Whether chaos testing is enabled
    pub chaos_enabled: bool,
    /// Whether property checking is enabled
    pub property_checking_enabled: bool,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            effects_executed: 0,
            devices_created: 0,
            choreographies_initialized: 0,
            chaos_enabled: false,
            property_checking_enabled: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}
