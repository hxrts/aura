//! GuardianAuthRelational Protocol Test
//!
//! Tests the guardian authentication choreography harness setup.
//! Full protocol execution requires actual guardian auth message types.

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::AuraProtocolAdapter;
use aura_authentication::guardian_auth_runners::GuardianAuthRelationalRole;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn guardian_auth_harness_builds() {
    // Test that protocol harness can be built
    let account_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian_device = DeviceId::from_uuid(Uuid::new_v4());
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("GuardianAuthRelational")
        .bind_role("Account", account_device)
        .bind_role("Guardian", guardian_device)
        .bind_role("Coordinator", coordinator_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn guardian_auth_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let account_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian_device = DeviceId::from_uuid(Uuid::new_v4());
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());

    let account_auth = AuthorityId::from_uuid(account_device.uuid());
    let guardian_auth = AuthorityId::from_uuid(guardian_device.uuid());
    let coordinator_auth = AuthorityId::from_uuid(coordinator_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(GuardianAuthRelationalRole::Account, account_auth);
    role_map.insert(GuardianAuthRelationalRole::Guardian, guardian_auth);
    role_map.insert(GuardianAuthRelationalRole::Coordinator, coordinator_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let account_effects = TestEffectSystem::new(
        bus.clone(),
        account_device,
        GuardianAuthRelationalRole::Account
            .role_index()
            .unwrap_or(0),
    )
    .expect("effects");

    let guardian_effects = TestEffectSystem::new(
        bus.clone(),
        guardian_device,
        GuardianAuthRelationalRole::Guardian
            .role_index()
            .unwrap_or(1),
    )
    .expect("effects");

    let coordinator_effects = TestEffectSystem::new(
        bus.clone(),
        coordinator_device,
        GuardianAuthRelationalRole::Coordinator
            .role_index()
            .unwrap_or(2),
    )
    .expect("effects");

    // Verify adapters can be created
    let mut account_adapter = AuraProtocolAdapter::new(
        Arc::new(account_effects),
        account_auth,
        GuardianAuthRelationalRole::Account,
        role_map.clone(),
    );

    let mut guardian_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian_effects),
        guardian_auth,
        GuardianAuthRelationalRole::Guardian,
        role_map.clone(),
    );

    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_effects),
        coordinator_auth,
        GuardianAuthRelationalRole::Coordinator,
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
