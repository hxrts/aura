#![allow(clippy::expect_used)]
#![allow(missing_docs)]

use aura_core::identifiers::{AuthorityId, ContextId, SessionId};
use aura_simulator::{
    PropertyEvent, PropertyMonitoringConfig, PropertyStateSnapshot, ProtocolPropertyClass,
    ProtocolPropertySuiteIds, SimulationEffectComposer, SimulationScenarioConfig,
};
use aura_testkit::DeviceTestFixture;
use uuid::Uuid;

fn injected_fault_provider(
    session: SessionId,
    tick_to_fault: u64,
) -> impl Fn(u64) -> PropertyStateSnapshot {
    move |tick| {
        if tick == tick_to_fault {
            PropertyStateSnapshot {
                events: vec![PropertyEvent::Faulted {
                    session,
                    reason: "injected message drop fault".to_string(),
                }],
                ..PropertyStateSnapshot::default()
            }
        } else {
            PropertyStateSnapshot::default()
        }
    }
}

#[tokio::test]
async fn fault_injection_is_reported_by_property_monitor() {
    let fixture = DeviceTestFixture::new(55);
    let authority_id = AuthorityId::new_from_entropy([55u8; 32]);
    let env = SimulationEffectComposer::for_testing(fixture.device_id(), authority_id)
        .await
        .expect("create simulation environment");

    let session = SessionId::from_uuid(Uuid::from_u128(7001));
    let context = ContextId::from_uuid(Uuid::from_u128(7002));

    let monitoring = PropertyMonitoringConfig::new(
        ProtocolPropertyClass::Consensus,
        ProtocolPropertySuiteIds { session, context },
    )
    .with_snapshot_provider(injected_fault_provider(session, 1));

    let config = SimulationScenarioConfig {
        max_ticks: 3,
        property_monitoring: Some(monitoring),
        ..SimulationScenarioConfig::default()
    };

    let results = env
        .run_scenario(
            "fault_monitoring".to_string(),
            "fault should violate NoFaults".to_string(),
            config,
        )
        .await
        .expect("run scenario");

    assert!(
        results
            .property_violations
            .iter()
            .any(|violation| violation.property == "NoFaults"),
        "expected NoFaults violation, got: {:?}",
        results.property_violations
    );
}

#[tokio::test]
#[should_panic(expected = "property monitor gate expected clean run")]
async fn monitor_gate_fails_when_fault_violation_exists() {
    let fixture = DeviceTestFixture::new(56);
    let authority_id = AuthorityId::new_from_entropy([56u8; 32]);
    let env = SimulationEffectComposer::for_testing(fixture.device_id(), authority_id)
        .await
        .expect("create simulation environment");

    let session = SessionId::from_uuid(Uuid::from_u128(7101));
    let context = ContextId::from_uuid(Uuid::from_u128(7102));

    let monitoring = PropertyMonitoringConfig::new(
        ProtocolPropertyClass::Consensus,
        ProtocolPropertySuiteIds { session, context },
    )
    .with_snapshot_provider(injected_fault_provider(session, 1));

    let config = SimulationScenarioConfig {
        max_ticks: 3,
        property_monitoring: Some(monitoring),
        ..SimulationScenarioConfig::default()
    };

    let results = env
        .run_scenario(
            "fault_monitoring_gate".to_string(),
            "monitor gate should panic on injected fault".to_string(),
            config,
        )
        .await
        .expect("run scenario");

    assert!(
        results.property_violations.is_empty(),
        "property monitor gate expected clean run, found violations: {:?}",
        results.property_violations
    );
}
