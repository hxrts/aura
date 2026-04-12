#![allow(clippy::disallowed_methods)] // Test code uses monotonic clock for coordination

use super::*;
use crate::services::HealthStatus;

#[derive(Clone)]
struct TestTimeEffects {
    now_ms: u64,
}

impl TestTimeEffects {
    fn new(now_ms: u64) -> Self {
        Self { now_ms }
    }
}

#[async_trait::async_trait]
impl PhysicalTimeEffects for TestTimeEffects {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        Ok(aura_core::time::PhysicalTime {
            ts_ms: self.now_ms,
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, _ms: u64) -> Result<(), TimeError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_sync_service_creation() {
    let config = SyncServiceConfig::default();
    let time_effects = Arc::new(TestTimeEffects::new(0));
    let service = SyncService::new(config, time_effects, SyncService::monotonic_now())
        .await
        .unwrap();

    assert_eq!(service.name(), "SyncService");
    assert!(!service.is_running());
}

#[tokio::test]
async fn test_sync_service_builder() {
    let time_effects = Arc::new(TestTimeEffects::new(0));
    let service = SyncService::builder()
        .with_auto_sync(true)
        .with_sync_interval(Duration::from_secs(30))
        .build(time_effects.clone(), SyncService::monotonic_now())
        .await
        .unwrap();

    assert!(service.config.auto_sync_enabled);
    assert_eq!(service.config.auto_sync_interval, Duration::from_secs(30));
}

#[tokio::test]
async fn test_sync_service_lifecycle() {
    let time_effects = Arc::new(TestTimeEffects::new(0));
    let service = SyncService::builder()
        .build(time_effects.clone(), SyncService::monotonic_now())
        .await
        .unwrap();

    assert!(!service.is_running());
    service
        .start_with_time_effects(time_effects.as_ref(), SyncService::monotonic_now())
        .await
        .unwrap();
    assert!(service.is_running());

    service.stop(SyncService::monotonic_now()).await.unwrap();
    assert!(!service.is_running());
}

#[tokio::test]
async fn test_sync_service_health_check() {
    let time_effects = Arc::new(TestTimeEffects::new(0));
    let service = SyncService::builder()
        .build(time_effects.clone(), SyncService::monotonic_now())
        .await
        .unwrap();
    service
        .start_with_time_effects(time_effects.as_ref(), SyncService::monotonic_now())
        .await
        .unwrap();

    let health = service.health_check().await.unwrap();
    assert_eq!(health.status, HealthStatus::Healthy);
    assert!(health.details.contains_key("active_sessions"));
}
