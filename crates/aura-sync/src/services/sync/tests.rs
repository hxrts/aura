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

#[tokio::test]
async fn required_sync_exact_session_drop_retires_initializing_target_and_preserves_foreign_session(
) {
    let time = Arc::new(TestTimeEffects::new(1000));
    let service = SyncService::new(
        SyncServiceConfig::default(),
        time.clone(),
        SyncService::monotonic_now(),
    )
    .await
    .expect("actual sync service configured clock");
    let now = time
        .physical_time()
        .await
        .expect("actual session birth observation");
    let foreign = service
        .session_manager
        .write()
        .create_session(vec![DeviceId::new_from_entropy([71; 32])], &now)
        .expect("unrelated real initializing allocation");
    let original = aura_core::time::timeout::TimeoutBudget::from_start_and_timeout(
        &now,
        Duration::from_secs(30),
    )
    .expect("original session resource policy");
    let issued = service
        .create_required_sessions(&[DeviceId::new_from_entropy([72; 32])], &original)
        .await
        .expect("actual required issued target allocation");
    assert_eq!(
        service.session_manager.read().retained_session_ids().len(),
        2
    );
    drop(issued);
    assert_eq!(
        service.session_manager.read().retained_session_ids().len(),
        1
    );
    assert!(
        service
            .session_manager
            .read()
            .get_session(&foreign)
            .is_some(),
        "exact owner must preserve unrelated initializing session"
    );
}

#[tokio::test]
async fn required_sync_partial_session_admission_retires_original_issued_subset() {
    use std::error::Error;
    let time = Arc::new(TestTimeEffects::new(2000));
    let service = SyncService::new(
        SyncServiceConfig::default(),
        time.clone(),
        SyncService::monotonic_now(),
    )
    .await
    .expect("actual sync service fixture");
    let now = time
        .physical_time()
        .await
        .expect("actual original allocation time");
    *service.session_manager.write() = SessionManager::new(
        SessionConfig {
            max_concurrent_sessions: 1,
            ..SessionConfig::default()
        },
        now,
    );
    let peers = [
        DeviceId::new_from_entropy([73; 32]),
        DeviceId::new_from_entropy([74; 32]),
    ];
    let observed = service
        .time_effects
        .physical_time()
        .await
        .expect("actual session test clock");
    let original = aura_core::time::timeout::TimeoutBudget::from_start_and_timeout(
        &observed,
        Duration::from_secs(30),
    )
    .expect("original session resource policy");
    let failure = match service.create_required_sessions(&peers, &original).await {
        Ok(_) => {
            panic!("Initializing allocation must consume the actual concurrent session policy")
        }
        Err(failure) => failure,
    };
    assert!(
        matches!(failure.source().and_then(|source| source.downcast_ref::<RequiredPeerSyncError>()), Some(RequiredPeerSyncError::SessionAdmission { peer, .. }) if *peer == peers[1])
    );
    assert_eq!(
        service.session_manager.read().retained_session_ids().len(),
        0,
        "failed admission retires original subset without fake peer cleanup"
    );
}

#[tokio::test]
async fn required_sync_cancelled_future_retires_its_exact_issued_session() {
    let time = Arc::new(TestTimeEffects::new(3000));
    let service = SyncService::new(
        SyncServiceConfig::default(),
        time,
        SyncService::monotonic_now(),
    )
    .await
    .expect("actual sync service fixture");
    let observed = service
        .time_effects
        .physical_time()
        .await
        .expect("actual session test clock");
    let original = aura_core::time::timeout::TimeoutBudget::from_start_and_timeout(
        &observed,
        Duration::from_secs(30),
    )
    .expect("original session resource policy");
    let mut operation = Box::pin(async {
        let issued = service
            .create_required_sessions(&[DeviceId::new_from_entropy([75; 32])], &original)
            .await
            .expect("actual issued session before suspended operation");
        std::future::pending::<()>().await;
        drop(issued);
    });
    assert!(futures::poll!(operation.as_mut()).is_pending());
    assert_eq!(
        service.session_manager.read().retained_session_ids().len(),
        1
    );
    drop(operation);
    assert_eq!(
        service.session_manager.read().retained_session_ids().len(),
        0,
        "cancelled future synchronously retires original local allocation"
    );
}
