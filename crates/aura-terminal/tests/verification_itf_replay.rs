//! ITF Trace Replay Tests
//!
//! These tests replay Quint-generated ITF traces through the TUI state machine
//! to verify that the Rust implementation matches the formal specification.

use aura_terminal::testing::itf_replay::ITFTraceReplayer;

/// Test replaying the generated TUI trace
#[test]
fn test_replay_tui_trace() {
    let trace_path = "../../verification/traces/tui_trace.itf.json";

    // Skip if trace file doesn't exist
    if !std::path::Path::new(trace_path).exists() {
        eprintln!("Skipping: trace file not found at {}", trace_path);
        return;
    }

    let replayer = ITFTraceReplayer::new();
    let result = replayer
        .replay_trace_file(trace_path)
        .expect("Failed to replay trace");

    println!("Trace replay results:");
    println!("  Total steps: {}", result.total_steps);
    println!("  Matched steps: {}", result.matched_steps);
    println!("  Failed steps: {}", result.failed_steps.len());

    for failed in &result.failed_steps {
        eprintln!("  Step {} failed: {:?}", failed.step_index, failed.diff);
    }

    assert!(
        result.all_states_match,
        "Not all states matched: {} failures",
        result.failed_steps.len()
    );
}

/// Test ITF parsing with inline trace
#[test]
fn test_parse_inline_trace() {
    let trace_json = r##"
    {
      "#meta": {
        "format": "ITF",
        "format-description": "https://apalache-mc.org/docs/adr/015adr-trace.html",
        "source": "test",
        "status": "ok",
        "description": "Test trace",
        "timestamp": 0
      },
      "vars": ["currentScreen", "currentModal", "blockInsertMode", "chatInsertMode", "shouldExit", "terminalWidth", "terminalHeight"],
      "states": [
        {
          "#meta": {"index": 0},
          "currentScreen": {"tag": "Home", "value": {"#tup": []}},
          "currentModal": {"tag": "NoModal", "value": {"#tup": []}},
          "blockInsertMode": false,
          "chatInsertMode": false,
          "shouldExit": false,
          "terminalWidth": {"#bigint": "80"},
          "terminalHeight": {"#bigint": "24"},
          "blockInputBuffer": "",
          "chatInputBuffer": "",
          "modalInputBuffer": "",
          "commandQueue": []
        },
        {
          "#meta": {"index": 1},
          "currentScreen": {"tag": "Chat", "value": {"#tup": []}},
          "currentModal": {"tag": "NoModal", "value": {"#tup": []}},
          "blockInsertMode": false,
          "chatInsertMode": false,
          "shouldExit": false,
          "terminalWidth": {"#bigint": "80"},
          "terminalHeight": {"#bigint": "24"},
          "blockInputBuffer": "",
          "chatInputBuffer": "",
          "modalInputBuffer": "",
          "commandQueue": []
        }
      ]
    }
    "##;

    let trace: aura_terminal::testing::itf_replay::ITFTrace =
        serde_json::from_str(trace_json).expect("Failed to parse trace");

    assert_eq!(trace.states.len(), 2);
    assert_eq!(trace.meta.format, "ITF");

    let replayer = ITFTraceReplayer::new();
    let result = replayer.replay_trace(&trace).expect("Failed to replay");

    assert!(result.all_states_match);
    assert_eq!(result.total_steps, 2);
}

/// Test state invariant validation
#[test]
fn test_invariant_validation() {
    // This trace has an invalid state (insert mode on wrong screen)
    let trace_json = r##"
    {
      "#meta": {
        "format": "ITF",
        "format-description": "https://apalache-mc.org/docs/adr/015adr-trace.html",
        "source": "test",
        "status": "ok",
        "description": "Test trace with invalid state",
        "timestamp": 0
      },
      "vars": ["currentScreen", "currentModal", "blockInsertMode", "chatInsertMode", "shouldExit", "terminalWidth", "terminalHeight"],
      "states": [
        {
          "#meta": {"index": 0},
          "currentScreen": {"tag": "Contacts", "value": {"#tup": []}},
          "currentModal": {"tag": "NoModal", "value": {"#tup": []}},
          "blockInsertMode": true,
          "chatInsertMode": false,
          "shouldExit": false,
          "terminalWidth": {"#bigint": "80"},
          "terminalHeight": {"#bigint": "24"},
          "blockInputBuffer": "",
          "chatInputBuffer": "",
          "modalInputBuffer": "",
          "commandQueue": []
        }
      ]
    }
    "##;

    let trace: aura_terminal::testing::itf_replay::ITFTrace =
        serde_json::from_str(trace_json).expect("Failed to parse trace");

    let replayer = ITFTraceReplayer::new();
    let result = replayer.replay_trace(&trace).expect("Failed to replay");

    // Should detect invariant violation
    assert!(!result.all_states_match);
    assert_eq!(result.failed_steps.len(), 1);
}

/// Generative test: replay multiple random traces
#[test]
#[ignore] // Run with: cargo test --ignored
fn test_generative_trace_replay() {
    use std::process::Command;

    // Generate trace with 100 samples and 50 steps
    let output = Command::new("nix")
        .args([
            "develop",
            "-c",
            "quint",
            "run",
            "--max-samples=100",
            "--max-steps=50",
            "--out-itf=verification/traces/tui_generative.itf.json",
            "verification/quint/tui_state_machine.qnt",
        ])
        .current_dir("../../")
        .output()
        .expect("Failed to run Quint");

    if !output.status.success() {
        eprintln!(
            "Quint run failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let trace_path = "../../verification/traces/tui_generative.itf.json";
    let replayer = ITFTraceReplayer::new();
    let result = replayer
        .replay_trace_file(trace_path)
        .expect("Failed to replay generative trace");

    println!("Generative trace replay results:");
    println!("  Total steps: {}", result.total_steps);
    println!("  Matched steps: {}", result.matched_steps);

    assert!(
        result.all_states_match,
        "Generative trace validation failed"
    );
}

/// Extended generative test: run multiple independent simulations
/// This provides better state space coverage by using different random seeds.
#[test]
#[ignore] // Run with: cargo test --ignored
fn test_multi_seed_generative_replay() {
    use std::process::Command;

    const NUM_RUNS: usize = 10;
    const MAX_SAMPLES: usize = 100;
    const MAX_STEPS: usize = 30;

    let mut total_traces = 0;
    let mut total_steps = 0;
    let mut total_matched = 0;
    let mut failures = Vec::new();

    let replayer = ITFTraceReplayer::new();

    for run in 0..NUM_RUNS {
        let trace_file = format!("verification/traces/tui_gen_{}.itf.json", run);

        // Generate trace with different seed each run
        let output = Command::new("nix")
            .args([
                "develop",
                "-c",
                "quint",
                "run",
                &format!("--max-samples={}", MAX_SAMPLES),
                &format!("--max-steps={}", MAX_STEPS),
                &format!("--seed={}", run * 12345 + 42),
                &format!("--out-itf={}", trace_file),
                "verification/quint/tui_state_machine.qnt",
            ])
            .current_dir("../../")
            .output()
            .expect("Failed to run Quint");

        if !output.status.success() {
            eprintln!(
                "Run {} failed: {}",
                run,
                String::from_utf8_lossy(&output.stderr)
            );
            continue;
        }

        // Replay the trace
        let trace_path = format!("../../{}", trace_file);
        match replayer.replay_trace_file(&trace_path) {
            Ok(result) => {
                total_traces += 1;
                total_steps += result.total_steps;
                total_matched += result.matched_steps;

                if !result.all_states_match {
                    failures.push((run, result.failed_steps.len()));
                }

                println!(
                    "Run {}: {} steps, {} matched",
                    run, result.total_steps, result.matched_steps
                );
            }
            Err(e) => {
                eprintln!("Run {} replay failed: {}", run, e);
            }
        }

        // Clean up trace file
        let _ = std::fs::remove_file(&trace_path);
    }

    println!("\nMulti-seed generative test summary:");
    println!("  Total runs: {}", total_traces);
    println!("  Total steps: {}", total_steps);
    println!("  Total matched: {}", total_matched);
    println!("  Failures: {}", failures.len());

    assert!(
        failures.is_empty(),
        "Generative testing found {} failures: {:?}",
        failures.len(),
        failures
    );
    assert!(
        total_traces >= NUM_RUNS / 2,
        "Not enough successful runs: {} < {}",
        total_traces,
        NUM_RUNS / 2
    );
}

/// Stress test: run high-volume generative testing
/// Use this to find edge cases through exhaustive exploration.
#[test]
#[ignore] // Run with: cargo test test_high_volume_generative -- --ignored --nocapture
fn test_high_volume_generative() {
    use std::process::Command;

    // High-volume settings: 1000 samples, 100 steps
    let output = Command::new("nix")
        .args([
            "develop",
            "-c",
            "quint",
            "run",
            "--max-samples=1000",
            "--max-steps=100",
            "--out-itf=verification/traces/tui_stress.itf.json",
            "verification/quint/tui_state_machine.qnt",
        ])
        .current_dir("../../")
        .output()
        .expect("Failed to run Quint");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Quint high-volume run failed: {}", stderr);
    }

    let trace_path = "../../verification/traces/tui_stress.itf.json";
    let replayer = ITFTraceReplayer::new();
    let result = replayer
        .replay_trace_file(trace_path)
        .expect("Failed to replay stress trace");

    println!("High-volume generative test results:");
    println!("  Total steps: {}", result.total_steps);
    println!("  Matched steps: {}", result.matched_steps);
    println!("  Failed steps: {}", result.failed_steps.len());

    for failed in &result.failed_steps {
        eprintln!("  Step {} failed: {:?}", failed.step_index, failed.diff);
    }

    // Clean up
    let _ = std::fs::remove_file(trace_path);

    assert!(
        result.all_states_match,
        "High-volume generative test failed"
    );
}
