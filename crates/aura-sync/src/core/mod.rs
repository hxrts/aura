//! Layer 5: Sync Protocol Foundation - Config, Errors, Metrics, Sessions
//!
//! Foundation types supporting synchronization protocol implementations:
//! **SyncConfig** (protocol configuration), **SyncResult** (unified error handling via `AuraError`),
//! **SyncMetrics** (performance instrumentation), **SessionManager** (session lifecycle).
//!
//! **Integration** (per docs/111_rendezvous.md):
//! All sync protocols (anti-entropy, journal sync, OTA, snapshots) are built atop
//! these foundation types. SessionManager coordinates multi-peer synchronization sessions.

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
    sync_protocol_error, sync_protocol_phase_error, sync_protocol_phase_with_peer,
    sync_protocol_with_peer, sync_resource_exhausted, sync_resource_with_limit,
    sync_serialization_error, sync_session_error, sync_timeout_error, sync_timeout_with_peer,
    sync_validation_error, sync_validation_field_error, SyncPhase, SyncResult,
};
pub use metrics::{MetricsCollector, SyncMetrics};
pub use session::{
    SessionConfig, SessionError, SessionManager, SessionManagerBuilder, SessionManagerStatistics,
    SessionResult, SessionState,
};
