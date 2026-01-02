//! Layer 3: System Effect Handlers - Logging, Metrics, Monitoring
//!
//! Stateless single-party implementations of system infrastructure effects.
//! Moved from aura-protocol (Layer 4) because they implement basic single-operation
//! handlers with no multi-party coordination.
//!
//! **Handler Types**:
//! - **LoggingSystemHandler**: Console output formatting and log level filtering
//! - **MetricsSystemHandler**: Instrumentation counters and timing aggregation
//! - **MonitoringSystemHandler**: Health checks and anomaly detection
//!
//! **Layer Constraint** (per docs/001_system_architecture.md):
//! Layer 3 implements single-party handlers; multi-party coordination belongs in Layer 4.
//! These handlers have no cross-party dependencies or choreography logic.

pub mod logging;
pub mod metrics;
pub mod monitoring;
pub mod types;

pub use logging::LoggingSystemHandler;
pub use metrics::MetricsSystemHandler;
pub use monitoring::MonitoringSystemHandler;
pub use types::{AuditAction, ComponentId, LogLevel};
