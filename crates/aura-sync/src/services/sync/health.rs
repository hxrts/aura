use super::*;
use crate::services::{HealthStatus, ServiceMetrics};

/// Sync service health information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncServiceHealth {
    /// Overall health status.
    pub status: HealthStatus,
    /// Number of active sync sessions.
    pub active_sessions: u32,
    /// Number of tracked peers.
    pub tracked_peers: u32,
    /// Number of available peers.
    pub available_peers: u32,
    /// Last sync timestamp.
    pub last_sync: Option<u64>,
    /// Service uptime.
    pub uptime: Duration,
}

impl SyncService {
    /// Get service health.
    pub fn get_health(&self) -> SyncServiceHealth {
        let state = self.state.read().clone();
        let status = match state {
            ServiceState::Running => HealthStatus::Healthy,
            ServiceState::Starting => HealthStatus::Starting,
            ServiceState::Stopping => HealthStatus::Stopping,
            ServiceState::Stopped | ServiceState::Failed(_) => HealthStatus::Unhealthy,
        };

        let session_stats = self.session_manager.read().get_statistics();
        let peer_stats = self.peer_manager.read().statistics();

        SyncServiceHealth {
            status,
            active_sessions: session_stats.active_sessions as u32,
            tracked_peers: peer_stats.total_tracked,
            available_peers: peer_stats.available_peers,
            last_sync: self.metrics.read().get_last_sync_timestamp(),
            uptime: self
                .started_at
                .read()
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO),
        }
    }

    /// Get service metrics.
    pub fn get_metrics(&self) -> ServiceMetrics {
        let metrics = self.metrics.read();

        ServiceMetrics {
            uptime_seconds: self
                .started_at
                .read()
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0),
            requests_processed: metrics.get_total_requests_processed(),
            errors_encountered: metrics.get_total_errors_encountered(),
            avg_latency_ms: metrics.get_average_sync_latency_ms(),
            last_operation_at: metrics.get_last_operation_timestamp(),
        }
    }

    pub(super) async fn build_health_check(&self) -> SyncResult<HealthCheck> {
        let health = self.get_health();
        let mut details = std::collections::HashMap::new();

        details.insert(
            "active_sessions".to_string(),
            health.active_sessions.to_string(),
        );
        details.insert(
            "tracked_peers".to_string(),
            health.tracked_peers.to_string(),
        );
        details.insert(
            "available_peers".to_string(),
            health.available_peers.to_string(),
        );
        details.insert(
            "uptime".to_string(),
            format!("{}s", health.uptime.as_secs()),
        );

        Ok(HealthCheck {
            status: health.status,
            message: Some("Sync service operational".to_string()),
            checked_at: self
                .time_effects
                .physical_time()
                .await
                .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
                .ts_ms
                / 1000,
            details,
        })
    }
}
