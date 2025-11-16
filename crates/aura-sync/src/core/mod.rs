//! Core abstractions for aura-sync
//!
//! This module provides the foundation types and patterns used throughout
//! the sync protocols. It follows Aura's Layer 5 (Feature/Protocol) guidelines.

pub mod errors;
pub mod messages;
pub mod config;
pub mod metrics;
pub mod session;

// Re-export key types for convenience
pub use errors::{SyncError, SyncResult};
pub use config::{SyncConfig, NetworkConfig, RetryConfig, BatchConfig, PeerManagementConfig, ProtocolConfigs, PerformanceConfig, SyncConfigBuilder};
pub use metrics::{SyncMetrics, MetricsCollector};
pub use session::{SessionManager, SessionState, SessionResult, SessionError, SessionConfig, SessionManagerStatistics, SessionManagerBuilder};