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
    // Placeholder for metrics handlers that will be moved from aura-effects/system/metrics.rs
    #[derive(Debug)]
    pub struct MetricsHandler;

    impl Default for MetricsHandler {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MetricsHandler {
        pub fn new() -> Self {
            Self
        }
    }
}

/// Logging system handlers for testing
pub mod logging {
    // Placeholder for logging handlers that will be moved from aura-effects/system/logging.rs
    #[derive(Debug)]
    pub struct LoggingHandler;

    impl Default for LoggingHandler {
        fn default() -> Self {
            Self::new()
        }
    }

    impl LoggingHandler {
        pub fn new() -> Self {
            Self
        }
    }
}

/// Monitoring system handlers for testing
pub mod monitoring {
    // Placeholder for monitoring handlers that will be moved from aura-effects/system/monitoring.rs
    #[derive(Debug)]
    pub struct MonitoringHandler;

    impl Default for MonitoringHandler {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MonitoringHandler {
        pub fn new() -> Self {
            Self
        }
    }
}
