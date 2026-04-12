use super::*;

/// Builder for [`SyncService`].
#[derive(Default)]
pub struct SyncServiceBuilder {
    config: Option<SyncServiceConfig>,
}

impl SyncServiceBuilder {
    /// Set configuration.
    pub fn with_config(mut self, config: SyncServiceConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set auto-sync enabled.
    pub fn with_auto_sync(mut self, enabled: bool) -> Self {
        self.config
            .get_or_insert_with(Default::default)
            .auto_sync_enabled = enabled;
        self
    }

    /// Set auto-sync interval.
    pub fn with_sync_interval(mut self, interval: Duration) -> Self {
        self.config
            .get_or_insert_with(Default::default)
            .auto_sync_interval = interval;
        self
    }

    /// Build the service.
    pub async fn build(
        self,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        now_instant: MonotonicInstant,
    ) -> SyncResult<SyncService> {
        SyncService::new(self.config.unwrap_or_default(), time_effects, now_instant).await
    }
}
