//! Flow Budget Enforcement Tests
//!
//! Tests relay capability-based flow budget enforcement:
//! - RelayCapability flow budget limits
//! - Budget consumption tracking
//! - Budget renewal and decay
//! - Capability restriction via meet-semilattice laws
//! - Budget exhaustion handling
//!
//! Validates that relay flows respect capability constraints
//! and properly enforce privacy-preserving leakage budgets.

use aura_core::{AuraError, DeviceId};
use aura_journal::semilattice::capability::{
    Capability, CapabilitySet, RelayCapability, FlowBudget, BudgetDecayPolicy,
};
use aura_rendezvous::relay_selection::{RelayCandidate, RelayScore, RelaySelectionConfig};
use aura_transport::relay::{RelayStream, RelayStreamConfig, StreamDirection};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

/// Flow budget test configuration
#[derive(Debug, Clone)]
pub struct BudgetTestConfig {
    /// Initial flow budget in bytes
    pub initial_budget: u64,
    /// Budget decay policy
    pub decay_policy: BudgetDecayPolicy,
    /// Test duration
    pub test_duration: Duration,
    /// Flow simulation parameters
    pub bytes_per_message: u64,
    pub messages_per_second: u64,
}

impl Default for BudgetTestConfig {
    fn default() -> Self {
        Self {
            initial_budget: 10_000_000, // 10 MB
            decay_policy: BudgetDecayPolicy::LinearDecay {
                decay_rate: 0.1, // 10% per hour
                decay_interval: Duration::from_secs(3600),
            },
            test_duration: Duration::from_secs(60),
            bytes_per_message: 1024, // 1 KB messages
            messages_per_second: 10,
        }
    }
}

/// Flow budget test result
#[derive(Debug, Clone)]
pub struct BudgetTestResult {
    pub test_name: String,
    pub initial_budget: u64,
    pub final_budget: u64,
    pub bytes_consumed: u64,
    pub messages_processed: usize,
    pub budget_exhausted: bool,
    pub duration: Duration,
    pub throughput_bps: f64,
}

impl BudgetTestResult {
    pub fn budget_utilization(&self) -> f64 {
        if self.initial_budget == 0 {
            0.0
        } else {
            self.bytes_consumed as f64 / self.initial_budget as f64
        }
    }

    pub fn average_message_size(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            self.bytes_consumed as f64 / self.messages_processed as f64
        }
    }
}

/// Create test relay capability with flow budget
fn create_test_relay_capability(
    relay_id: DeviceId,
    initial_budget: u64,
    decay_policy: BudgetDecayPolicy,
) -> RelayCapability {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let flow_budget = FlowBudget {
        bytes_remaining: initial_budget,
        last_updated: current_time,
        decay_policy,
    };

    RelayCapability {
        relay_device_id: relay_id,
        flow_budget,
        max_concurrent_streams: 10,
        allowed_destinations: None, // Allow all destinations for testing
        expires_at: current_time + 3600, // 1 hour
    }
}

/// Simulate message flow through relay with budget tracking
async fn simulate_flow_with_budget(
    mut capability: RelayCapability,
    config: BudgetTestConfig,
) -> BudgetTestResult {
    let test_name = "Flow Budget Simulation".to_string();
    let start_time = Instant::now();
    let initial_budget = capability.flow_budget.bytes_remaining;

    let mut bytes_consumed = 0u64;
    let mut messages_processed = 0usize;
    let mut budget_exhausted = false;

    let message_interval = Duration::from_millis(1000 / config.messages_per_second);
    let mut next_message_time = start_time;

    // Simulate message flow
    while start_time.elapsed() < config.test_duration && !budget_exhausted {
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            std::time::Instant::now()
                + (next_message_time.saturating_duration_since(start_time)),
        ))
        .await;

        // Check if we have budget for this message
        if capability.flow_budget.bytes_remaining >= config.bytes_per_message {
            // Consume budget
            capability.consume_flow_budget(config.bytes_per_message);
            bytes_consumed += config.bytes_per_message;
            messages_processed += 1;

            // Apply decay (simulate time passing)
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            capability.flow_budget.apply_decay(current_time);

            next_message_time += message_interval;
        } else {
            budget_exhausted = true;
            println!(
                "Budget exhausted after {} messages ({} bytes)",
                messages_processed, bytes_consumed
            );
        }
    }

    let final_duration = start_time.elapsed();
    let throughput_bps = if final_duration.as_secs() > 0 {
        bytes_consumed as f64 / final_duration.as_secs_f64()
    } else {
        0.0
    };

    BudgetTestResult {
        test_name,
        initial_budget,
        final_budget: capability.flow_budget.bytes_remaining,
        bytes_consumed,
        messages_processed,
        budget_exhausted,
        duration: final_duration,
        throughput_bps,
    }
}

/// Test basic flow budget consumption
#[tokio::test]
async fn test_basic_budget_consumption() {
    let relay_id = DeviceId("test_relay_1".to_string());
    let config = BudgetTestConfig {
        initial_budget: 5000, // Small budget for quick test
        test_duration: Duration::from_secs(10),
        bytes_per_message: 500,
        messages_per_second: 2, // Should consume budget in ~5 messages
        ..Default::default()
    };

    let capability = create_test_relay_capability(
        relay_id,
        config.initial_budget,
        config.decay_policy.clone(),
    );

    let result = simulate_flow_with_budget(capability, config.clone()).await;
    print_budget_test_result(&result);

    // Verify budget consumption
    assert!(result.bytes_consumed > 0);
    assert_eq!(
        result.initial_budget - result.final_budget,
        result.bytes_consumed
    );
    assert!(result.budget_utilization() > 0.0);

    // Should process around 5 messages before budget exhaustion
    assert!(result.messages_processed >= 3 && result.messages_processed <= 12);
}

/// Test budget decay over time
#[tokio::test]
async fn test_budget_decay() {
    let relay_id = DeviceId("test_relay_2".to_string());
    
    // Test linear decay
    let linear_decay = BudgetDecayPolicy::LinearDecay {
        decay_rate: 0.5, // 50% decay for quick testing
        decay_interval: Duration::from_secs(5), // Every 5 seconds
    };

    let mut capability = create_test_relay_capability(relay_id.clone(), 10000, linear_decay);
    let initial_budget = capability.flow_budget.bytes_remaining;

    // Simulate time passage
    tokio::time::sleep(Duration::from_secs(6)).await;

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    capability.flow_budget.apply_decay(current_time);

    println!("Initial budget: {}", initial_budget);
    println!("Budget after decay: {}", capability.flow_budget.bytes_remaining);
    println!(
        "Decay amount: {}",
        initial_budget - capability.flow_budget.bytes_remaining
    );

    // Should have decayed significantly
    assert!(capability.flow_budget.bytes_remaining < initial_budget);

    // Test exponential decay
    let exponential_decay = BudgetDecayPolicy::ExponentialDecay {
        half_life: Duration::from_secs(5),
    };

    let mut exp_capability = create_test_relay_capability(relay_id, 10000, exponential_decay);
    let exp_initial = exp_capability.flow_budget.bytes_remaining;

    tokio::time::sleep(Duration::from_secs(6)).await;

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    exp_capability.flow_budget.apply_decay(current_time);

    println!("Exponential decay - Initial: {}, After: {}", exp_initial, exp_capability.flow_budget.bytes_remaining);
    assert!(exp_capability.flow_budget.bytes_remaining < exp_initial);
}

/// Test capability restriction via meet-semilattice
#[tokio::test]
async fn test_capability_restriction() {
    let relay_id = DeviceId("test_relay_3".to_string());

    // Create two capabilities with different budgets
    let high_budget_cap = create_test_relay_capability(
        relay_id.clone(),
        10000,
        BudgetDecayPolicy::NoDecay,
    );

    let low_budget_cap = create_test_relay_capability(
        relay_id.clone(),
        5000,
        BudgetDecayPolicy::NoDecay,
    );

    // Meet operation should result in the more restrictive capability
    let restricted_cap = high_budget_cap.meet(&low_budget_cap);

    println!(
        "High budget: {}, Low budget: {}, Restricted: {}",
        high_budget_cap.flow_budget.bytes_remaining,
        low_budget_cap.flow_budget.bytes_remaining,
        restricted_cap.flow_budget.bytes_remaining
    );

    // Meet should take the minimum budget
    assert_eq!(
        restricted_cap.flow_budget.bytes_remaining,
        low_budget_cap.flow_budget.bytes_remaining
    );

    // Meet should take the minimum concurrent streams
    assert_eq!(
        restricted_cap.max_concurrent_streams,
        std::cmp::min(
            high_budget_cap.max_concurrent_streams,
            low_budget_cap.max_concurrent_streams
        )
    );

    // Meet should take the earlier expiration time
    assert_eq!(
        restricted_cap.expires_at,
        std::cmp::min(high_budget_cap.expires_at, low_budget_cap.expires_at)
    );
}

/// Test budget exhaustion handling
#[tokio::test]
async fn test_budget_exhaustion_handling() {
    let relay_id = DeviceId("test_relay_4".to_string());
    let config = BudgetTestConfig {
        initial_budget: 1000, // Very small budget
        test_duration: Duration::from_secs(30), // Long duration
        bytes_per_message: 200, // Should exhaust in ~5 messages
        messages_per_second: 5,
        decay_policy: BudgetDecayPolicy::NoDecay,
    };

    let capability = create_test_relay_capability(
        relay_id,
        config.initial_budget,
        config.decay_policy.clone(),
    );

    let result = simulate_flow_with_budget(capability, config).await;
    print_budget_test_result(&result);

    // Should exhaust budget before test duration ends
    assert!(result.budget_exhausted);
    assert!(result.duration < Duration::from_secs(30));
    assert_eq!(result.final_budget, 0);
    assert!(result.budget_utilization() >= 0.8); // Used most of the budget

    println!(
        "Budget exhausted after {} messages in {:?}",
        result.messages_processed, result.duration
    );
}

/// Test relay selection with budget constraints
#[tokio::test]
async fn test_relay_selection_budget_constraints() {
    // Create relay candidates with different budgets
    let relay_1 = DeviceId("high_budget_relay".to_string());
    let relay_2 = DeviceId("medium_budget_relay".to_string());
    let relay_3 = DeviceId("low_budget_relay".to_string());

    let high_budget_cap = create_test_relay_capability(
        relay_1.clone(),
        100_000,
        BudgetDecayPolicy::NoDecay,
    );

    let medium_budget_cap = create_test_relay_capability(
        relay_2.clone(),
        50_000,
        BudgetDecayPolicy::NoDecay,
    );

    let low_budget_cap = create_test_relay_capability(
        relay_3.clone(),
        10_000,
        BudgetDecayPolicy::NoDecay,
    );

    // Create relay candidates
    let candidates = vec![
        RelayCandidate {
            device_id: relay_1.clone(),
            is_guardian: false,
            is_friend: true,
            capability: Some(high_budget_cap),
            latency: Duration::from_millis(50),
            load_factor: 0.3,
            trust_score: 0.8,
        },
        RelayCandidate {
            device_id: relay_2.clone(),
            is_guardian: true, // Guardian should be preferred
            is_friend: false,
            capability: Some(medium_budget_cap),
            latency: Duration::from_millis(60),
            load_factor: 0.5,
            trust_score: 0.9,
        },
        RelayCandidate {
            device_id: relay_3.clone(),
            is_guardian: false,
            is_friend: true,
            capability: Some(low_budget_cap),
            latency: Duration::from_millis(40),
            load_factor: 0.2,
            trust_score: 0.7,
        },
    ];

    let selection_config = RelaySelectionConfig::default();

    // Calculate scores
    let mut scored_candidates: Vec<_> = candidates
        .into_iter()
        .map(|candidate| {
            let score = candidate.calculate_score(&selection_config);
            (candidate, score)
        })
        .collect();

    // Sort by score (highest first)
    scored_candidates.sort_by(|a, b| b.1.total_score.partial_cmp(&a.1.total_score).unwrap());

    println!("Relay Selection Results:");
    for (candidate, score) in &scored_candidates {
        let budget = candidate
            .capability
            .as_ref()
            .map(|c| c.flow_budget.bytes_remaining)
            .unwrap_or(0);

        println!(
            "  {}: Score={:.3}, Budget={}, Guardian={}, Friend={}",
            candidate.device_id.0,
            score.total_score,
            budget,
            candidate.is_guardian,
            candidate.is_friend
        );
    }

    // Guardian should be preferred despite lower budget
    let best_candidate = &scored_candidates[0].0;
    assert_eq!(best_candidate.device_id, relay_2);
    assert!(best_candidate.is_guardian);

    // Verify budget considerations in scoring
    let high_budget_score = scored_candidates
        .iter()
        .find(|(c, _)| c.device_id == relay_1)
        .unwrap()
        .1;
    let low_budget_score = scored_candidates
        .iter()
        .find(|(c, _)| c.device_id == relay_3)
        .unwrap()
        .1;

    // Higher budget should contribute to better score
    assert!(high_budget_score.total_score > low_budget_score.total_score);
}

/// Test concurrent stream budget tracking
#[tokio::test]
async fn test_concurrent_stream_budget() {
    let relay_id = DeviceId("concurrent_test_relay".to_string());
    let mut capability = create_test_relay_capability(
        relay_id,
        20_000,
        BudgetDecayPolicy::NoDecay,
    );

    let stream_config = RelayStreamConfig {
        max_message_size: 1024,
        buffer_size: 8192,
        timeout: Duration::from_secs(30),
        direction: StreamDirection::Bidirectional,
    };

    // Simulate multiple concurrent streams consuming budget
    let streams = vec![
        ("stream_1", 1000u64),
        ("stream_2", 1500u64),
        ("stream_3", 2000u64),
        ("stream_4", 500u64),
    ];

    let initial_budget = capability.flow_budget.bytes_remaining;
    let mut total_consumed = 0u64;

    for (stream_name, bytes_to_consume) in streams {
        if capability.flow_budget.bytes_remaining >= bytes_to_consume {
            capability.consume_flow_budget(bytes_to_consume);
            total_consumed += bytes_to_consume;
            println!("Stream {} consumed {} bytes", stream_name, bytes_to_consume);
        } else {
            println!(
                "Stream {} denied: insufficient budget ({} remaining, {} needed)",
                stream_name, capability.flow_budget.bytes_remaining, bytes_to_consume
            );
        }
    }

    println!("Initial budget: {}", initial_budget);
    println!("Total consumed: {}", total_consumed);
    println!("Remaining budget: {}", capability.flow_budget.bytes_remaining);

    assert_eq!(
        capability.flow_budget.bytes_remaining,
        initial_budget - total_consumed
    );
    assert!(total_consumed > 0);
}

/// Print budget test results
fn print_budget_test_result(result: &BudgetTestResult) {
    println!("\n=== {} ===", result.test_name);
    println!("Initial Budget: {} bytes", result.initial_budget);
    println!("Final Budget: {} bytes", result.final_budget);
    println!("Bytes Consumed: {} bytes", result.bytes_consumed);
    println!("Messages Processed: {}", result.messages_processed);
    println!("Budget Utilization: {:.1}%", result.budget_utilization() * 100.0);
    println!("Average Message Size: {:.1} bytes", result.average_message_size());
    println!("Throughput: {:.1} bytes/sec", result.throughput_bps);
    println!("Test Duration: {:?}", result.duration);
    println!("Budget Exhausted: {}", result.budget_exhausted);
    println!();
}

/// Comprehensive budget enforcement test suite
#[tokio::test]
async fn test_flow_budget_enforcement_suite() {
    println!("Starting Flow Budget Enforcement Test Suite");
    println!("============================================");
    println!("Testing relay capability flow budget enforcement:");
    println!("- Basic budget consumption tracking");
    println!("- Budget decay policies (linear/exponential)");
    println!("- Capability restriction via meet-semilattice");
    println!("- Budget exhaustion handling");
    println!("- Relay selection with budget constraints");
    println!("- Concurrent stream budget management");
    println!();

    // Run all budget enforcement tests
    test_basic_budget_consumption().await;
    test_budget_decay().await;
    test_capability_restriction().await;
    test_budget_exhaustion_handling().await;
    test_relay_selection_budget_constraints().await;
    test_concurrent_stream_budget().await;

    println!("Flow Budget Enforcement Test Suite Complete");
    println!("============================================");
    println!("Summary: Relay flow budgets properly enforced via capability system.");
    println!("Budget decay: Linear and exponential policies implemented");
    println!("Capability restriction: Meet-semilattice laws correctly applied");
    println!("Budget exhaustion: Properly handled with stream termination");
    println!("Relay selection: Budget constraints integrated into scoring");
    println!("Concurrent tracking: Multiple stream budget consumption verified");
}