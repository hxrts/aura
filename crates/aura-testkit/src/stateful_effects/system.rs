//! System monitoring and metrics handlers for testing
//!
//! This module contains stateful system handlers that were moved from aura-effects
//! to fix architectural violations. These handlers use Arc<RwLock<>> for shared
//! state in testing and monitoring scenarios.

// Re-export the specific handlers that were moved
pub use logging::*;
pub use metrics::*;
pub use monitoring::*;

/// Metrics system handlers for testing
pub mod metrics {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[derive(Debug, Clone, Default)]
    pub struct MetricsHandler {
        counters: Arc<RwLock<HashMap<String, u64>>>,
    }

    impl MetricsHandler {
        pub fn new() -> Self {
            Self {
                counters: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        pub async fn incr(&self, name: &str) {
            let mut counters = self.counters.write().await;
            *counters.entry(name.to_string()).or_insert(0) += 1;
        }

        pub async fn get(&self, name: &str) -> u64 {
            let counters = self.counters.read().await;
            counters.get(name).copied().unwrap_or(0)
        }
    }
}

/// Logging system handlers for testing
pub mod logging {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[derive(Debug, Clone, Default)]
    pub struct LoggingHandler {
        entries: Arc<RwLock<Vec<String>>>,
    }

    impl LoggingHandler {
        pub fn new() -> Self {
            Self {
                entries: Arc::new(RwLock::new(Vec::new())),
            }
        }

        pub async fn log(&self, line: impl Into<String>) {
            self.entries.write().await.push(line.into());
        }

        pub async fn entries(&self) -> Vec<String> {
            self.entries.read().await.clone()
        }
    }
}

/// Monitoring system handlers for testing
pub mod monitoring {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[derive(Debug, Clone, Default)]
    pub struct MonitoringHandler {
        alerts: Arc<RwLock<Vec<String>>>,
    }

    impl MonitoringHandler {
        pub fn new() -> Self {
            Self {
                alerts: Arc::new(RwLock::new(Vec::new())),
            }
        }

        pub async fn raise(&self, message: impl Into<String>) {
            self.alerts.write().await.push(message.into());
        }

        pub async fn all(&self) -> Vec<String> {
            self.alerts.read().await.clone()
        }
    }
}
