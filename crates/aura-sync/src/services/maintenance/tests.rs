use super::*;

#[allow(clippy::disallowed_methods)]
fn monotonic_now() -> MonotonicInstant {
    type MonoClock = MonotonicInstant;
    MonoClock::now()
}

#[test]
fn test_maintenance_service_creation() {
    let config = MaintenanceServiceConfig::default();
    let service = MaintenanceService::new(config).unwrap();

    assert_eq!(service.name(), "MaintenanceService");
    assert!(!service.is_running());
}

#[tokio::test]
async fn test_maintenance_service_lifecycle() {
    let service = MaintenanceService::new(MaintenanceServiceConfig::default()).unwrap();

    let time_effects = aura_effects::time::PhysicalTimeHandler;
    service
        .start_with_time_effects(&time_effects, monotonic_now())
        .await
        .unwrap();
    assert!(service.is_running());

    service.stop(monotonic_now()).await.unwrap();
    assert!(!service.is_running());
}

#[tokio::test]
async fn test_maintenance_service_with_time_effects() {
    let service = MaintenanceService::new(MaintenanceServiceConfig::default()).unwrap();
    let time_effects = aura_testkit::stateful_effects::SimulatedTimeHandler::new();

    service
        .start_with_time_effects(&time_effects, monotonic_now())
        .await
        .unwrap();
    assert!(service.is_running());

    service.stop(monotonic_now()).await.unwrap();
    assert!(!service.is_running());
}

#[tokio::test]
async fn test_propose_upgrade_with_random_effects() {
    let service = MaintenanceService::new(MaintenanceServiceConfig::default()).unwrap();
    let random_effects = aura_testkit::stateful_effects::MockCryptoHandler::new();

    let package_id = Uuid::from_bytes(2u128.to_be_bytes());
    let version = SemanticVersion::new(1, 2, 3);
    let kind = UpgradeKind::SoftFork;
    let package_hash = Hash32::from([1u8; 32]);
    let proposer = AuthorityId::new_from_entropy([3u8; 32]);

    let proposal = service
        .propose_upgrade(
            package_id,
            version,
            kind,
            package_hash,
            proposer,
            &random_effects,
        )
        .await
        .unwrap();

    assert_eq!(proposal.package_id, package_id);
    assert_eq!(proposal.version, version);
    assert_eq!(proposal.kind, kind);
    assert_eq!(proposal.artifact_hash, package_hash);
}

#[test]
fn test_cache_invalidation() {
    let service = MaintenanceService::new(MaintenanceServiceConfig::default()).unwrap();

    let authority_id = AuthorityId::new_from_entropy([5u8; 32]);
    let result = service
        .invalidate_cache(
            authority_id,
            vec!["key1".to_string(), "key2".to_string()],
            Epoch::new(10),
        )
        .unwrap();

    assert_eq!(result.keys.len(), 2);
    assert_eq!(result.epoch_floor, Epoch::new(10));
}

#[test]
fn test_snapshot_due_check() {
    let config = MaintenanceServiceConfig {
        auto_snapshot_enabled: true,
        min_snapshot_interval_epochs: 100,
        ..Default::default()
    };

    let service = MaintenanceService::new(config).unwrap();

    assert!(service.is_snapshot_due(Epoch::new(0)));

    *service.last_snapshot_epoch.write() = Some(Epoch::new(50));
    assert!(!service.is_snapshot_due(Epoch::new(100)));
    assert!(service.is_snapshot_due(Epoch::new(151)));
}
