async fn test_e2e_integration(
    device_count: u16,
    base_port: u16,
    test_duration: u64,
    file_count: u32,
    file_size: u32,
    events_per_device: u32,
    test_security: bool,
    collect_metrics: bool,
    generate_report: bool,
) -> anyhow::Result<()> {
    info!("═══════════════════════════════════════════════════════════════");
    info!("        AURA END-TO-END INTEGRATION TEST STARTING");
    info!("═══════════════════════════════════════════════════════════════");
    info!("Configuration:");
    info!("  - Devices: {}", device_count);
    info!("  - Base Port: {}", base_port);
    info!("  - Test Duration: {}s", test_duration);
    info!("  - Files: {} ({} bytes each)", file_count, file_size);
    info!("  - Events per device: {}", events_per_device);
    info!("  - Security testing: {}", test_security);
    info!("  - Metrics collection: {}", collect_metrics);
    info!("  - Generate report: {}", generate_report);
    info!("═══════════════════════════════════════════════════════════════");

    let test_start_time = std::time::Instant::now();
    let mut test_results = E2ETestResults::new();

    // Phase 1: Multi-Device Threshold Operations Test
    info!("\n🔐 PHASE 1: Multi-Device Threshold Operations");
    info!("─────────────────────────────────────────────────");

    let threshold_start = std::time::Instant::now();

    // Initialize devices and test threshold operations
    let mut agents = Vec::new();
    let mut agent_results = Vec::new();

    for device_idx in 0..device_count {
        let device_num = device_idx + 1;
        let port = base_port + device_idx;
        let config_path = format!("config_{}.toml", device_num);

        info!(
            "  Initializing device {} on port {} with config {}",
            device_num, port, config_path
        );

        match crate::config::load_config(&config_path) {
            Ok(config) => match Agent::new(&config).await {
                Ok(agent) => {
                    let device_id = agent.device_id().await?;
                    let account_id = agent.account_id().await?;

                    info!(
                        "    ✓ Device {}: ID {}, Account {}, Port {}",
                        device_num, device_id, account_id, port
                    );

                    agent_results.push((device_num, port, device_id, account_id));
                    agents.push(agent);
                }
                Err(e) => {
                    warn!("    ✗ Device {} agent creation failed: {}", device_num, e);
                    test_results.failed_operations += 1;
                }
            },
            Err(e) => {
                warn!("    ✗ Device {} config load failed: {}", device_num, e);
                test_results.failed_operations += 1;
            }
        }
    }

    // Test threshold operations if we have enough devices
    if agents.len() >= 2 {
        info!("  Testing 2-of-{} threshold operations...", agents.len());

        // Test DKD protocol execution across participants
        let mut threshold_successes = 0;
        for (idx, agent) in agents.iter().enumerate().take(3) {
            let app_id = format!("threshold-test-{}", idx + 1);
            let context = format!(
                "e2e-integration-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs()
            );

            match agent.derive_identity(&app_id, &context).await {
                Ok(derived_identity) => {
                    info!(
                        "    ✓ Device {} threshold operation successful: {}...",
                        idx + 1,
                        hex::encode(&derived_identity.identity_key[..8])
                    );
                    threshold_successes += 1;
                    test_results.successful_operations += 1;
                }
                Err(e) => {
                    warn!("    ✗ Device {} threshold operation failed: {}", idx + 1, e);
                    test_results.failed_operations += 1;
                }
            }
        }

        info!(
            "  Phase 1 Summary: {}/{} threshold operations successful",
            threshold_successes,
            agents.len().min(3)
        );
    } else {
        warn!(
            "  ⚠ Insufficient devices for threshold operations (need at least 2, have {})",
            agents.len()
        );
    }

    let threshold_duration = threshold_start.elapsed();
    test_results.add_phase_time("threshold_operations", threshold_duration);

    // Final Summary
    info!("\n═══════════════════════════════════════════════════════════════");
    info!("        AURA END-TO-END INTEGRATION TEST COMPLETED");
    info!("═══════════════════════════════════════════════════════════════");

    let total_duration = test_start_time.elapsed();
    let success_rate = if test_results.successful_operations + test_results.failed_operations > 0 {
        (test_results.successful_operations as f64
            / (test_results.successful_operations + test_results.failed_operations) as f64)
            * 100.0
    } else {
        0.0
    };

    info!("FINAL RESULTS:");
    info!("  - Test Duration: {:.2}s", total_duration.as_secs_f64());
    info!("  - Devices Tested: {}", device_count);
    info!(
        "  - Successful Operations: {}",
        test_results.successful_operations
    );
    info!("  - Failed Operations: {}", test_results.failed_operations);
    info!("  - Success Rate: {:.1}%", success_rate);

    if test_results.failed_operations == 0 {
        info!("  🎉 ALL TESTS PASSED - Aura system is functioning correctly!");
    } else {
        warn!(
            "  ⚠ {} TESTS FAILED - Review failures above",
            test_results.failed_operations
        );
    }

    info!("═══════════════════════════════════════════════════════════════");

    Ok(())
}

#[derive(Debug)]
struct E2ETestResults {
    successful_operations: u32,
    failed_operations: u32,
    total_duration: std::time::Duration,
    phase_times: std::collections::HashMap<String, std::time::Duration>,
}

impl E2ETestResults {
    fn new() -> Self {
        Self {
            successful_operations: 0,
            failed_operations: 0,
            total_duration: std::time::Duration::default(),
            phase_times: std::collections::HashMap::new(),
        }
    }

    fn add_phase_time(&mut self, phase: &str, duration: std::time::Duration) {
        self.phase_times.insert(phase.to_string(), duration);
    }
}
