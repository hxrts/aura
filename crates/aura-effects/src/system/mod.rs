//! System Effect Handlers
//!
//! **Layer 3 (aura-effects)**: Basic system operation handlers.
//!
//! This module contains handlers for system-level effects like logging, metrics,
//! and monitoring. These handlers were moved from aura-protocol (Layer 4) as they
//! implement basic single-operation handlers with no coordination logic.

pub mod logging;
pub mod metrics;
pub mod monitoring;

pub use logging::LoggingSystemHandler;
pub use metrics::MetricsSystemHandler;
pub use monitoring::MonitoringSystemHandler;
