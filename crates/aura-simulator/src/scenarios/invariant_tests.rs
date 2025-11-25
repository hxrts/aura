//! Simulator invariant assertion scenarios
//!
//! These smoke tests exercise basic invariant expectations in a controlled,
//! deterministic simulator context. They are intentionally lightweight to keep
//! runtime low while still catching obvious regressions.

use crate::handlers::fault_simulation::SimulationFaultHandler;
use crate::handlers::time_control::SimulationTimeHandler;
use aura_core::effects::{ChaosEffects, PhysicalTimeEffects};
use std::time::Duration;

#[tokio::test]
async fn simulation_time_handler_deterministic_start() {
    let handler_a = SimulationTimeHandler::with_start_ms(1_000);
    let handler_b = SimulationTimeHandler::with_start_ms(1_000);

    let ta = handler_a.physical_time().await.unwrap().ts_ms;
    let tb = handler_b.physical_time().await.unwrap().ts_ms;

    assert!(
        (ta as i128 - tb as i128).abs() < 5,
        "handlers with same start should report similar timestamps"
    );
}

#[tokio::test]
async fn paused_time_does_not_advance() {
    let mut handler = SimulationTimeHandler::with_start_ms(1_000);
    let before = handler.physical_time().await.unwrap().ts_ms;
    handler.pause();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let after = handler.physical_time().await.unwrap().ts_ms;
    assert_eq!(before, after, "paused time must not advance");
    handler.resume();
    handler.sleep_ms(10).await.unwrap();
    let resumed = handler.physical_time().await.unwrap().ts_ms;
    assert!(resumed > after, "resumed time must advance");
}

#[tokio::test]
async fn fault_injection_is_tracked() {
    let fault_handler = SimulationFaultHandler::new(42);

    fault_handler
        .inject_network_delay((Duration::from_millis(10), Duration::from_millis(20)), None)
        .await
        .unwrap_or_else(|_| panic!("inject network delay"));

    let active = fault_handler.get_active_faults();
    assert!(
        !active.is_empty(),
        "fault injection should register active faults"
    );
}
