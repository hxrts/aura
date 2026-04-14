use super::*;
use crate::services::HealthStatus;

impl MaintenanceService {
    pub(super) async fn build_health_check(&self) -> SyncResult<HealthCheck> {
        let time_effects = aura_effects::time::PhysicalTimeHandler;
        let state = self.state.read().clone();
        let status = match state {
            ServiceState::Running => HealthStatus::Healthy,
            ServiceState::Starting => HealthStatus::Starting,
            ServiceState::Stopping => HealthStatus::Stopping,
            ServiceState::Stopped | ServiceState::Failed(_) => HealthStatus::Unhealthy,
        };

        let mut details = std::collections::HashMap::new();
        let snapshot_pending = {
            let snapshot_protocol = self.snapshot_protocol.read();
            snapshot_protocol.is_pending()
        };
        details.insert("snapshot_pending".to_string(), snapshot_pending.to_string());

        let ota_pending = {
            let ota_protocol = self.ota_protocol.read();
            ota_protocol.get_pending().is_some()
        };
        details.insert("ota_pending".to_string(), ota_pending.to_string());
        details.insert(
            "uptime".to_string(),
            format!("{}s", self.uptime().as_secs()),
        );

        let checked_at = time_effects
            .physical_time()
            .await
            .map_err(|e| crate::core::errors::sync_validation_error(format!("Time error: {e}")))?
            .ts_ms
            / 1000;

        Ok(HealthCheck {
            status,
            message: Some("Maintenance service operational".to_string()),
            checked_at,
            details,
        })
    }
}
