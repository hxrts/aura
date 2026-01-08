//! RecoveryProtocol Test
//!
//! Tests the recovery protocol choreography harness setup.
//! Full protocol execution requires actual recovery message types.

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_recovery::recovery_runners::RecoveryProtocolRole;
use aura_simulator::{SimulatedMessageBus, SimulatedTransport};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn recovery_protocol_harness_builds() {
    // Test that protocol harness can be built
    let account_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian_device = DeviceId::from_uuid(Uuid::new_v4());
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("RecoveryProtocol")
        .bind_role("Account", account_device)
        .bind_role("Guardian", guardian_device)
        .bind_role("Coordinator", coordinator_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn recovery_protocol_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let account_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian_device = DeviceId::from_uuid(Uuid::new_v4());
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());

    let account_auth = AuthorityId::from_uuid(account_device.uuid());
    let guardian_auth = AuthorityId::from_uuid(guardian_device.uuid());
    let coordinator_auth = AuthorityId::from_uuid(coordinator_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(RecoveryProtocolRole::Account, account_auth);
    role_map.insert(RecoveryProtocolRole::Guardian, guardian_auth);
    role_map.insert(RecoveryProtocolRole::Coordinator, coordinator_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let account_transport =
        SimulatedTransport::new(bus.clone(), account_device, RecoveryProtocolRole::Account.role_index().unwrap_or(0))
            .expect("account transport");

    let guardian_transport =
        SimulatedTransport::new(bus.clone(), guardian_device, RecoveryProtocolRole::Guardian.role_index().unwrap_or(1))
            .expect("guardian transport");

    let coordinator_transport =
        SimulatedTransport::new(bus.clone(), coordinator_device, RecoveryProtocolRole::Coordinator.role_index().unwrap_or(2))
            .expect("coordinator transport");

    // Verify adapters can be created
    let mut account_adapter = AuraProtocolAdapter::new(
        Arc::new(account_transport),
        account_auth,
        RecoveryProtocolRole::Account,
        role_map.clone(),
    );

    let mut guardian_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian_transport),
        guardian_auth,
        RecoveryProtocolRole::Guardian,
        role_map.clone(),
    );

    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_transport),
        coordinator_auth,
        RecoveryProtocolRole::Coordinator,
        role_map.clone(),
    );

    // Verify sessions can start
    account_adapter
        .start_session(session_id)
        .await
        .expect("account session start");
    guardian_adapter
        .start_session(session_id)
        .await
        .expect("guardian session start");
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator session start");

    // Verify sessions can end
    account_adapter
        .end_session()
        .await
        .expect("account session end");
    guardian_adapter
        .end_session()
        .await
        .expect("guardian session end");
    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator session end");
}
