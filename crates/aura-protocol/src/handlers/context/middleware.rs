//! Middleware context for cross-cutting concerns
//!
//! Immutable context for middleware operations, including tracing,
//! metrics, retry configuration, and custom middleware data.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::handlers::AuraHandlerError;

/// Immutable middleware-specific context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareContext {
    /// Tracing information
    pub tracing: TracingContext,
    /// Metrics collection
    pub metrics: MetricsContext,
    /// Retry configuration
    pub retry: RetryContext,
    /// Custom middleware data (immutable)
    pub custom_data: Arc<HashMap<String, Vec<u8>>>,
}

/// Tracing context for observability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TracingContext {
    /// Trace ID for distributed tracing
    pub trace_id: Option<String>,
    /// Span ID for current operation
    pub span_id: Option<String>,
    /// Whether tracing is enabled
    pub enabled: bool,
}

/// Metrics context for monitoring
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsContext {
    /// Whether metrics collection is enabled
    pub enabled: bool,
    /// Custom metrics labels (immutable)
    pub labels: Arc<HashMap<String, String>>,
}

/// Retry context for resilience
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryContext {
    /// Current retry attempt (0-based)
    pub attempt: u32,
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Whether exponential backoff is enabled
    pub exponential_backoff: bool,
}

impl Default for RetryContext {
    fn default() -> Self {
        Self {
            attempt: 0,
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            exponential_backoff: true,
        }
    }
}

impl MiddlewareContext {
    /// Create a new middleware context
    pub fn new() -> Self {
        Self {
            tracing: TracingContext::default(),
            metrics: MetricsContext {
                enabled: false,
                labels: Arc::new(HashMap::new()),
            },
            retry: RetryContext::default(),
            custom_data: Arc::new(HashMap::new()),
        }
    }

    /// Create context with custom data
    pub fn with_custom_data<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<Self, AuraHandlerError> {
        let serialized = aura_core::util::serialization::to_vec(value).map_err(|e| {
            AuraHandlerError::context_error(format!("Failed to serialize custom data: {e}"))
        })?;

        Ok(self.updated(|next| {
            next.custom_data = self.map_custom_data(|data| {
                data.insert(key.to_string(), serialized);
            });
        }))
    }

    /// Get custom middleware data
    pub fn get_custom_data<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, AuraHandlerError> {
        match self.custom_data.get(key) {
            Some(data) => {
                let value = aura_core::util::serialization::from_slice(data).map_err(|e| {
                    AuraHandlerError::context_error(format!(
                        "Failed to deserialize custom data: {e}"
                    ))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Create context with tracing enabled
    pub fn with_tracing(&self, trace_id: String, span_id: String) -> Self {
        self.updated(|next| {
            next.tracing = TracingContext {
                enabled: true,
                trace_id: Some(trace_id),
                span_id: Some(span_id),
            };
        })
    }

    /// Create context with metrics enabled
    pub fn with_metrics(&self) -> Self {
        self.updated(|next| {
            next.metrics = MetricsContext {
                enabled: true,
                labels: self.metrics.labels.clone(),
            };
        })
    }

    /// Add metrics label
    pub fn with_metrics_label(&self, key: &str, value: &str) -> Self {
        self.updated(|next| {
            next.metrics = MetricsContext {
                enabled: self.metrics.enabled,
                labels: self.map_metric_labels(|labels| {
                    labels.insert(key.to_string(), value.to_string());
                }),
            };
        })
    }

    fn updated(&self, update: impl FnOnce(&mut Self)) -> Self {
        let mut next = self.clone();
        update(&mut next);
        next
    }

    fn map_custom_data(
        &self,
        update: impl FnOnce(&mut HashMap<String, Vec<u8>>),
    ) -> Arc<HashMap<String, Vec<u8>>> {
        let mut data = (*self.custom_data).clone();
        update(&mut data);
        Arc::new(data)
    }

    fn map_metric_labels(
        &self,
        update: impl FnOnce(&mut HashMap<String, String>),
    ) -> Arc<HashMap<String, String>> {
        let mut labels = (*self.metrics.labels).clone();
        update(&mut labels);
        Arc::new(labels)
    }
}

impl Default for MiddlewareContext {
    fn default() -> Self {
        Self::new()
    }
}
