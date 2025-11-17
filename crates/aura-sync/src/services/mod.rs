//! Service layer for sync operations
//!
//! This module provides high-level services that orchestrate multiple protocols
//! and infrastructure components to provide complete synchronization functionality.
//!
//! # Architecture
//!
//! Services in this module:
//! - Orchestrate protocols from `protocols/` module
//! - Use infrastructure from `infrastructure/` module
//! - Provide unified interfaces for applications
//! - Handle cross-cutting concerns (health, metrics, lifecycle)
//!
//! # Service Hierarchy
//!
//! ```
//! Services (Layer 5 - Runtime Libraries)
//!   ├── Compose Protocols (anti-entropy, journal sync, snapshots, OTA)
//!   ├── Use Infrastructure (peers, connections, rate limiting)
//!   └── Provide Application APIs
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::services::{SyncService, SyncServiceConfig};
//! use aura_core::effects::{JournalEffects, NetworkEffects};
//!
//! async fn run_sync_service<E>(effects: E) -> Result<(), Box<dyn std::error::Error>>
//! where
//!     E: JournalEffects + NetworkEffects,
//! {
//!     let config = SyncServiceConfig::default();
//!     let service = SyncService::new(effects, config);
//!
//!     // Start service
//!     service.start().await?;
//!
//!     // Service handles periodic sync, maintenance, etc.
//!     Ok(())
//! }
//! ```

pub mod sync;
pub mod maintenance;

// Re-export key service types
pub use sync::{SyncService, SyncServiceConfig, SyncServiceBuilder, SyncServiceHealth};
pub use maintenance::{
    MaintenanceService, MaintenanceServiceConfig,
    MaintenanceEvent, SnapshotProposed, SnapshotCompleted,
    CacheInvalidated, UpgradeActivated, AdminReplaced,
    UpgradeProposal, IdentityEpochFence, CacheKey,
};

use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

use crate::core::{SyncError, SyncResult};

// =============================================================================
// Unified Service Interface
// =============================================================================

/// Health status for a service
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Service is healthy and operational
    Healthy,

    /// Service is degraded but operational
    Degraded,

    /// Service is unhealthy
    Unhealthy,

    /// Service is starting up
    Starting,

    /// Service is shutting down
    Stopping,
}

/// Health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Overall health status
    pub status: HealthStatus,

    /// Detailed health message
    pub message: Option<String>,

    /// Last health check timestamp
    pub checked_at: u64,

    /// Component-specific health details
    pub details: std::collections::HashMap<String, String>,
}

/// Unified service interface
///
/// All services implement this trait to provide consistent
/// lifecycle management and health monitoring.
#[async_trait::async_trait]
pub trait Service: Send + Sync {
    /// Start the service
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    async fn start(&self, now: Instant) -> SyncResult<()>;

    /// Stop the service gracefully
    async fn stop(&self) -> SyncResult<()>;

    /// Check service health
    async fn health_check(&self) -> SyncResult<HealthCheck>;

    /// Get service name
    fn name(&self) -> &str;

    /// Check if service is running
    fn is_running(&self) -> bool;
}

/// Service lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    /// Service not yet started
    Stopped,

    /// Service is starting
    Starting,

    /// Service is running
    Running,

    /// Service is stopping
    Stopping,

    /// Service stopped due to error
    Failed,
}

/// Common service metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceMetrics {
    /// Service uptime in seconds
    pub uptime_seconds: u64,

    /// Total requests processed
    pub requests_processed: u64,

    /// Total errors encountered
    pub errors_encountered: u64,

    /// Average request latency (milliseconds)
    pub avg_latency_ms: f64,

    /// Last operation timestamp
    pub last_operation_at: Option<u64>,
}

/// Service configuration base
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Enable service on startup
    pub enabled: bool,

    /// Health check interval
    pub health_check_interval: Duration,

    /// Enable metrics collection
    pub metrics_enabled: bool,

    /// Graceful shutdown timeout
    pub shutdown_timeout: Duration,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            health_check_interval: Duration::from_secs(30),
            metrics_enabled: true,
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}
