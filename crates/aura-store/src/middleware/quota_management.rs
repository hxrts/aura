//! Quota Management Middleware
//!
//! Enforces storage quotas and limits.

use super::handler::{StorageError, StorageHandler, StorageOperation, StorageResult};
use super::stack::StorageMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_protocol::middleware::{MiddlewareContext, MiddlewareError, MiddlewareResult};
use aura_types::AuraError;
use std::collections::HashMap;

/// Quota configuration
#[derive(Debug, Clone)]
pub struct QuotaConfig {
    pub max_total_size: u64,
    pub max_chunk_count: u64,
    pub max_chunk_size: usize,
    pub warn_threshold: f32, // Warn when usage exceeds this percentage
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            max_total_size: 1024 * 1024 * 1024, // 1GB
            max_chunk_count: 10000,
            max_chunk_size: 64 * 1024 * 1024, // 64MB
            warn_threshold: 0.8,              // 80%
        }
    }
}

/// Quota management middleware
pub struct QuotaMiddleware {
    config: QuotaConfig,
    current_size: u64,
    current_count: u64,
}

impl QuotaMiddleware {
    pub fn new() -> Self {
        Self {
            config: QuotaConfig::default(),
            current_size: 0,
            current_count: 0,
        }
    }

    pub fn with_config(config: QuotaConfig) -> Self {
        Self {
            config,
            current_size: 0,
            current_count: 0,
        }
    }

    fn check_quota(&self, additional_size: usize) -> Result<(), StorageError> {
        // Check chunk size limit
        if additional_size > self.config.max_chunk_size {
            return Err(StorageError::QuotaExceeded);
        }

        // Check total size limit
        if self.current_size + additional_size as u64 > self.config.max_total_size {
            return Err(StorageError::QuotaExceeded);
        }

        // Check chunk count limit
        if self.current_count + 1 > self.config.max_chunk_count {
            return Err(StorageError::QuotaExceeded);
        }

        Ok(())
    }

    pub fn get_usage_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("current_size".to_string(), self.current_size.to_string());
        info.insert("current_count".to_string(), self.current_count.to_string());
        info.insert(
            "max_total_size".to_string(),
            self.config.max_total_size.to_string(),
        );
        info.insert(
            "max_chunk_count".to_string(),
            self.config.max_chunk_count.to_string(),
        );

        let size_percentage =
            (self.current_size as f32 / self.config.max_total_size as f32) * 100.0;
        let count_percentage =
            (self.current_count as f32 / self.config.max_chunk_count as f32) * 100.0;

        info.insert(
            "size_usage_percent".to_string(),
            format!("{:.1}", size_percentage),
        );
        info.insert(
            "count_usage_percent".to_string(),
            format!("{:.1}", count_percentage),
        );

        info
    }
}

impl Default for QuotaMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageMiddleware for QuotaMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        match operation {
            StorageOperation::Store { ref data, .. } => {
                // Check quota before storing
                self.check_quota(data.len())
                    .map_err(|e| MiddlewareError::General {
                        message: format!("Quota exceeded: {}", e),
                    })?;

                // Execute the store operation
                let result = next.execute(operation, effects)?;

                // Update usage tracking on successful store
                if let StorageResult::Stored { size, .. } = &result {
                    self.current_size += *size as u64;
                    self.current_count += 1;
                }

                Ok(result)
            }

            StorageOperation::Delete { .. } => {
                // Execute the delete operation
                let result = next.execute(operation, effects)?;

                // Update usage tracking on successful delete
                if let StorageResult::Deleted { .. } = &result {
                    // Note: In a real implementation, we'd need to track chunk sizes
                    // to properly decrement the current_size
                    self.current_count = self.current_count.saturating_sub(1);
                }

                Ok(result)
            }

            _ => next.execute(operation, effects),
        }
    }

    fn middleware_name(&self) -> &'static str {
        "QuotaMiddleware"
    }

    fn middleware_info(&self) -> HashMap<String, String> {
        self.get_usage_info()
    }
}
