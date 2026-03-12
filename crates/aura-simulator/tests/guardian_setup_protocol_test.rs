//! GuardianSetup Protocol Test
//!
//! Tests the guardian setup choreography harness setup.
//! Full protocol execution requires actual guardian setup message types.

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::telltale_choreography::RoleId;
use aura_recovery::setup_runners::GuardianSetupRole;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn guardian_setup_harness_builds() {
    // Test that protocol harness can be built
    let initiator_device = DeviceId::from_uuid(Uuid::from_bytes([1; 16]));
    let guardian1_device = DeviceId::from_uuid(Uuid::from_bytes([2; 16]));
    let guardian2_device = DeviceId::from_uuid(Uuid::from_bytes([3; 16]));
    let guardian3_device = DeviceId::from_uuid(Uuid::from_bytes([4; 16]));

    let test = ProtocolTest::new("GuardianSetup")
        .bind_role("SetupInitiator", initiator_device)
        .bind_role("Guardian1", guardian1_device)
        .bind_role("Guardian2", guardian2_device)
        .bind_role("Guardian3", guardian3_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn guardian_setup_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let initiator_device = DeviceId::from_uuid(Uuid::from_bytes([11; 16]));
    let guardian1_device = DeviceId::from_uuid(Uuid::from_bytes([12; 16]));
    let guardian2_device = DeviceId::from_uuid(Uuid::from_bytes([13; 16]));
    let guardian3_device = DeviceId::from_uuid(Uuid::from_bytes([14; 16]));

    let initiator_auth = AuthorityId::for_device(initiator_device);
    let guardian1_auth = AuthorityId::for_device(guardian1_device);
    let guardian2_auth = AuthorityId::for_device(guardian2_device);
    let guardian3_auth = AuthorityId::for_device(guardian3_device);

    let mut role_map = HashMap::new();
    role_map.insert(GuardianSetupRole::SetupInitiator, initiator_auth);
    role_map.insert(GuardianSetupRole::Guardian1, guardian1_auth);
    role_map.insert(GuardianSetupRole::Guardian2, guardian2_auth);
    role_map.insert(GuardianSetupRole::Guardian3, guardian3_auth);

    let guardian_roles = vec![
        GuardianSetupRole::Guardian1,
        GuardianSetupRole::Guardian2,
        GuardianSetupRole::Guardian3,
    ];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([15; 16]);

    let initiator_effects = TestEffectSystem::new(
        bus.clone(),
        initiator_device,
        GuardianSetupRole::SetupInitiator.role_index().unwrap_or(0),
    )
    .expect("effects");

    let guardian1_effects = TestEffectSystem::new(
        bus.clone(),
        guardian1_device,
        GuardianSetupRole::Guardian1.role_index().unwrap_or(1),
    )
    .expect("guardian1 transport");

    // Verify adapters can be created with role family
    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_effects),
        initiator_auth,
        GuardianSetupRole::SetupInitiator,
        role_map.clone(),
    )
    .with_role_family("Guardian", guardian_roles.clone());

    let mut guardian1_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian1_effects),
        guardian1_auth,
        GuardianSetupRole::Guardian1,
        role_map.clone(),
    )
    .with_role_family("Guardian", guardian_roles.clone());

    // Verify sessions can start
    initiator_adapter
        .start_session(session_id)
        .await
        .expect("initiator session start");
    guardian1_adapter
        .start_session(session_id)
        .await
        .expect("guardian1 session start");

    // Verify sessions can end
    initiator_adapter
        .end_session()
        .await
        .expect("initiator session end");
    guardian1_adapter
        .end_session()
        .await
        .expect("guardian1 session end");
}
