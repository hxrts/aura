//! Failure attribution for harness execution.
//!
//! Maps raw error text into stable layer/phase diagnostics so real-runtime
//! failures are actionable instead of opaque.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureLayer {
    EnvironmentSetup,
    Startup,
    FrontendInteraction,
    StructuredObservation,
    BackendRpc,
    Runtime,
    Transport,
    Cleanup,
    ScenarioExecution,
    Harness,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailurePhase {
    Provisioning,
    Preflight,
    Startup,
    Interaction,
    Observation,
    Execution,
    Cleanup,
    ArtifactSync,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureAttribution {
    pub layer: FailureLayer,
    pub phase: FailurePhase,
    pub reason: String,
    pub message: String,
}

pub fn attribute_failure(message: &str) -> FailureAttribution {
    let normalized = message.to_ascii_lowercase();
    let (layer, phase, reason) = if contains_any(
        &normalized,
        &[
            "failed to reserve local bind address",
            "ssh probe failed",
            "localstorage unavailable",
            "preflight",
            "duplicate explicit bind_address",
        ],
    ) {
        (
            FailureLayer::EnvironmentSetup,
            FailurePhase::Preflight,
            "environment setup or preflight",
        )
    } else if contains_any(
        &normalized,
        &[
            "failed startup health gate",
            "health gate",
            "did not reach readiness",
            "start_page",
            "failed to spawn process",
            "failed to build web runtime",
        ],
    ) {
        (FailureLayer::Startup, FailurePhase::Startup, "startup/readiness")
    } else if contains_any(
        &normalized,
        &[
            "playwright driver",
            "timed out waiting for driver stdout",
            "closed stdout while awaiting",
            "backend rpc",
        ],
    ) {
        (FailureLayer::BackendRpc, FailurePhase::Interaction, "backend rpc")
    } else if contains_any(
        &normalized,
        &[
            "ui snapshot",
            "structured ui snapshots",
            "wait_for_selector",
            "wait_for timed out",
            "observation",
        ],
    ) {
        (
            FailureLayer::StructuredObservation,
            FailurePhase::Observation,
            "structured observation",
        )
    } else if contains_any(
        &normalized,
        &[
            "click_button",
            "activate_control",
            "fill_field",
            "fill_input",
            "send_keys",
            "send_key",
        ],
    ) {
        (
            FailureLayer::FrontendInteraction,
            FailurePhase::Interaction,
            "frontend interaction",
        )
    } else if contains_any(
        &normalized,
        &[
            "transport",
            "rendezvous",
            "anti-entropy",
            "network monitor",
            "holepunch",
        ],
    ) {
        (FailureLayer::Transport, FailurePhase::Execution, "transport/runtime network")
    } else if contains_any(
        &normalized,
        &[
            "runtime_accept",
            "appcore",
            "failed to initialize app signals",
            "runtime bridge",
            "time not implemented on this platform",
        ],
    ) {
        (FailureLayer::Runtime, FailurePhase::Execution, "runtime")
    } else if contains_any(
        &normalized,
        &[
            "failed teardown health gate",
            "did not release bind address",
            "cleanup_ok",
            "remote artifact sync",
        ],
    ) {
        (FailureLayer::Cleanup, FailurePhase::Cleanup, "cleanup/teardown")
    } else if contains_any(
        &normalized,
        &["scenario execution failed", "scenario lint failed", "step timed out"],
    ) {
        (
            FailureLayer::ScenarioExecution,
            FailurePhase::Execution,
            "scenario execution",
        )
    } else if normalized.contains("artifact") && normalized.contains("sync") {
        (FailureLayer::Harness, FailurePhase::ArtifactSync, "artifact sync")
    } else {
        (FailureLayer::Unknown, FailurePhase::Unknown, "unclassified")
    };

    FailureAttribution {
        layer,
        phase,
        reason: reason.to_string(),
        message: message.to_string(),
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{attribute_failure, FailureLayer, FailurePhase};

    #[test]
    fn attributes_startup_failures() {
        let attribution = attribute_failure("instance alice failed startup health gate");
        assert_eq!(attribution.layer, FailureLayer::Startup);
        assert_eq!(attribution.phase, FailurePhase::Startup);
    }

    #[test]
    fn attributes_cleanup_failures() {
        let attribution =
            attribute_failure("instance alice did not release bind address 127.0.0.1:41001");
        assert_eq!(attribution.layer, FailureLayer::Cleanup);
        assert_eq!(attribution.phase, FailurePhase::Cleanup);
    }
}
