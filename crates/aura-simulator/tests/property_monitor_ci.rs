//! Property monitor CI integration tests.
//!
//! Verifies that property monitors produce deterministic trend reports
//! and can be used in CI pipelines for regression detection.

#![allow(clippy::expect_used)]
#![allow(missing_docs)]

use aura_core::types::identifiers::{AuthorityId, ContextId, SessionId};
use aura_simulator::{
    PropertyMonitoringConfig, PropertyRunReport, PropertyTrendTracker, ProtocolPropertyClass,
    ProtocolPropertySuiteIds, SimulationEffectComposer, SimulationScenarioConfig,
};
use aura_testkit::DeviceTestFixture;
use std::path::Path;
use uuid::Uuid;

#[tokio::test]
async fn property_monitor_ci_gate() {
    let fixture = DeviceTestFixture::new(99);
    let device_id = fixture.device_id();
    let authority_id = AuthorityId::new_from_entropy([99u8; 32]);
    let env = SimulationEffectComposer::for_testing(device_id, authority_id)
        .await
        .expect("create simulation environment");

    let session = SessionId::from_uuid(Uuid::from_u128(9001));
    let context = ContextId::from_uuid(Uuid::from_u128(9002));
    let monitoring = PropertyMonitoringConfig::new(
        ProtocolPropertyClass::Consensus,
        ProtocolPropertySuiteIds { session, context },
    );
    let config = SimulationScenarioConfig {
        max_ticks: 8,
        property_monitoring: Some(monitoring),
        ..SimulationScenarioConfig::default()
    };

    let results = env
        .run_scenario(
            "ci_property_monitor".to_string(),
            "CI gate for online property monitoring".to_string(),
            config,
        )
        .await
        .expect("run scenario");

    let report = results.property_run_report();

    if let Ok(path) = std::env::var("AURA_PROPERTY_MONITOR_REPORT") {
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent).expect("create report directory");
        }
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&report).expect("serialize report"),
        )
        .expect("write property monitor report");
    }

    if let Ok(path) = std::env::var("AURA_PROPERTY_MONITOR_BASELINE") {
        let baseline_path = Path::new(&path);
        if baseline_path.exists() {
            let baseline = std::fs::read(baseline_path).expect("read baseline");
            let baseline_report: PropertyRunReport =
                serde_json::from_slice(&baseline).expect("parse baseline report");
            let regression = report.compare_against(&baseline_report);
            assert!(
                !regression.has_new_violations(),
                "new property-monitor violations detected: {:?}",
                regression.new_violations
            );
        }
    }

    if let Ok(path) = std::env::var("AURA_PROPERTY_MONITOR_TREND") {
        let trend_path = Path::new(&path);
        let mut tracker = if trend_path.exists() {
            let bytes = std::fs::read(trend_path).expect("read trend file");
            serde_json::from_slice::<PropertyTrendTracker>(&bytes).expect("parse trend file")
        } else {
            PropertyTrendTracker::default()
        };
        tracker.record_run(&report);
        std::fs::write(
            trend_path,
            serde_json::to_vec_pretty(&tracker).expect("serialize trend tracker"),
        )
        .expect("write trend tracker");
    }

    assert!(
        report.violations.is_empty(),
        "property monitor CI gate captured violations: {:?}",
        report.violations
    );
}
