use super::{MetricsCollector, SessionConfig, SessionManager};
use aura_core::time::PhysicalTime;
use serde::{Deserialize, Serialize};

/// Session manager builder for easy configuration.
pub struct SessionManagerBuilder<T> {
    config: SessionConfig,
    metrics: Option<MetricsCollector>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SessionManagerBuilder<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    /// Create new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: SessionConfig::default(),
            metrics: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set custom configuration.
    pub fn config(mut self, config: SessionConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable metrics collection.
    pub fn with_metrics(mut self, metrics: MetricsCollector) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Build the session manager.
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider and pass it to this method.
    pub fn build(self, now: PhysicalTime) -> SessionManager<T> {
        if let Some(metrics) = self.metrics {
            SessionManager::with_metrics(self.config, metrics, now)
        } else {
            SessionManager::new(self.config, now)
        }
    }

    /// Build the session manager (from milliseconds).
    ///
    /// Convenience method for backward compatibility.
    pub fn build_ms(self, now_ms: u64) -> SessionManager<T> {
        self.build(PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        })
    }
}

impl<T> Default for SessionManagerBuilder<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    fn default() -> Self {
        Self::new()
    }
}
