//! Replay AMP channel lifecycle traces against real simulation agents.
#![allow(clippy::expect_used)]

use std::path::Path;

use aura_simulator::quint::{
    amp_channel_registry, AmpChannelHarness, GenerativeSimulator, GenerativeSimulatorConfig,
    ITFLoader, QuintSimulationState,
};

#[tokio::test]
#[ignore]
async fn replay_amp_channel_lifecycle_trace() {
    let trace_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("missing manifest ancestors")
        .join("traces/amp_channel.itf.json");

    if !trace_path.exists() {
        println!("Skipping: AMP channel trace not found at {trace_path:?}");
        return;
    }

    let trace = ITFLoader::load_from_file(&trace_path).expect("failed to load AMP trace");

    let base_dir =
        std::env::temp_dir().join(format!("aura-amp-channel-itf-{}", std::process::id()));
    let harness = AmpChannelHarness::new(2025, base_dir)
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
