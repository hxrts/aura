//! Core abstractions for aura-sync
//!
//! This module provides the foundation types and patterns used throughout
//! the sync protocols. It follows Aura's Layer 5 (Feature/Protocol) guidelines.

pub mod config;
pub mod errors;
pub mod messages;
pub mod metrics;
pub mod session;

// Re-export key types for convenience
pub use config::{
    BatchConfig, NetworkConfig, PeerManagementConfig, PerformanceConfig, ProtocolConfigs,
    RetryConfig, SyncConfig, SyncConfigBuilder,
};
pub use errors::{
    sync_authorization_error, sync_biscuit_authorization_error, sync_biscuit_guard_error,
    sync_config_error, sync_consistency_error, sync_network_error, sync_peer_error,
    sync_protocol_error, sync_protocol_with_peer, sync_resource_exhausted,
    sync_resource_with_limit, sync_serialization_error, sync_session_error, sync_timeout_error,
    sync_timeout_with_peer, sync_validation_error, sync_validation_field_error, SyncError,
    SyncResult,
};
pub use metrics::{MetricsCollector, SyncMetrics};
pub use session::{
    SessionConfig, SessionError, SessionManager, SessionManagerBuilder, SessionManagerStatistics,
    SessionResult, SessionState,
};
