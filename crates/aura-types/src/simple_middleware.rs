//! Simple synchronous middleware types for Aura
//!
//! This module provides basic middleware types that can be used across Aura crates
//! without async lifetime complexity.

use crate::errors::AuraError;
use std::collections::HashMap;

/// Result type for middleware operations  
pub type MiddlewareResult<T> = Result<T, AuraError>;

/// Simple middleware context for synchronous operations
#[derive(Debug, Clone)]
pub struct MiddlewareContext {
    /// Operation name being executed
    pub operation_name: String,
    /// Additional metadata  
    pub metadata: HashMap<String, String>,
    /// Timestamp when operation started
    pub start_time: u64,
}

impl MiddlewareContext {
    /// Create a new middleware context
    pub fn new(operation_name: String) -> Self {
        Self {
            operation_name,
            metadata: HashMap::new(),
            start_time: 0, // Will be set by Effects
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set the start time
    pub fn with_start_time(mut self, start_time: u64) -> Self {
        self.start_time = start_time;
        self
    }
}

impl Default for MiddlewareContext {
    fn default() -> Self {
        Self::new("unknown".to_string())
    }
}