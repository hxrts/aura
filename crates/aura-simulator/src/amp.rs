//! AMP simulation scenarios (placeholder).
use crate::scenario::types::{ByzantineConditions, ExpectedOutcome, NetworkConditions, Scenario, ScenarioAssertion, ScenarioSetup};

/// Out-of-order delivery scenario to validate dual-window tolerance.
pub fn amp_out_of_order_scenario() -> Scenario {
    Scenario {
        id: "amp_out_of_order".into(),
        name: "AMP out-of-order within 2W".into(),
        setup: ScenarioSetup {
            participants: 3,
            threshold: 2,
        },
        network_conditions: Some(NetworkConditions {
            latency_ms: Some(50),
            packet_loss: Some(0.1),
        }),
        byzantine_conditions: Some(ByzantineConditions {
            strategies: vec![],
        }),
        assertions: vec![ScenarioAssertion {
            property: "amp_dual_window_accepts_in_order_and_out_of_order".into(),
            expected: true,
        }],
        expected_outcome: ExpectedOutcome::Success,
    }
}

