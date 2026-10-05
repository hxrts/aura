//! Replay AMP lifecycle steps 1–14 against real simulation agents, followed by
//! simulator-only observational transition-policy steps 15–24. The latter
//! issue no native certificates, consensus identities or destruction receipts.
#![allow(clippy::expect_used, clippy::disallowed_methods)]

use std::path::Path;

use aura_simulator::quint::{
    amp_channel_registry, AmpChannelHarness, GenerativeSimulator, GenerativeSimulatorConfig,
    ITFLoader, QuintSimulationState,
};

#[tokio::test]
async fn replay_amp_channel_lifecycle_trace() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("missing manifest ancestors");
    let trace_path = workspace_root.join("verification/quint/traces/amp_channel.itf.json");

    let trace = ITFLoader::load_from_file(&trace_path).expect("failed to load AMP trace");

    let storage = tempfile::tempdir().expect("isolated AMP replay storage");
    let harness = AmpChannelHarness::new(2025, storage.path().to_path_buf())
        .await
        .expect("failed to build AMP harness");
    let registry = amp_channel_registry(harness);

    let simulator = GenerativeSimulator::new(
        registry,
        GenerativeSimulatorConfig {
            max_steps: 200,
            record_trace: true,
            verbose: true,
            exploration_seed: Some(2025),
        },
    );

    let result = simulator
        .replay_trace(&trace, QuintSimulationState::new())
        .await
        .expect("replay failed");

    if !result.success {
        if let Some(step) = result.steps.iter().find(|s| !s.success) {
            eprintln!(
                "AMP replay failed at step {} action {} error {:?}",
                step.index, step.action, step.error
            );
        }
    }

    assert!(
        result.success,
        "AMP channel trace replay failed at step {}",
        result.step_count
    );
}
