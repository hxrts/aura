use super::*;

impl MaintenanceService {
    /// Check if snapshot is due.
    pub fn is_snapshot_due(&self, current_epoch: Epoch) -> bool {
        if !self.config.auto_snapshot_enabled {
            return false;
        }

        match *self.last_snapshot_epoch.read() {
            None => true,
            Some(last) => {
                current_epoch.value() >= last.value() + self.config.min_snapshot_interval_epochs
            }
        }
    }

    /// Get service uptime.
    pub fn uptime(&self) -> Duration {
        self.started_at
            .read()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    pub(super) async fn flush_pending_operations(&self) -> SyncResult<()> {
        self.cache_manager.write().clear();
        Ok(())
    }
}
