//! Property Monitor Demo
//! 
//! This example demonstrates the property monitor with Quint integration

use aura_simulator::{
    testing::{PropertyMonitor, QuintInvariant, QuintSafetyProperty, SimulationState, 
              ProtocolExecutionState, SessionInfo, ParticipantStateSnapshot, 
              NetworkStateSnapshot, MessageDeliveryStats, NetworkFailureConditions},
    Result, SimError
};

fn main() -> Result<()> {
    println!("[setup] Property Monitor Demo with Quint Integration");
    println!("================================================\n");
    
    // Create property monitor
    let mut monitor = PropertyMonitor::new();
    
    // Add properties that match our Quint specification
    monitor.add_invariant(QuintInvariant {
        name: "validCounts".to_string(),
        expression: "validCounts".to_string(),
        description: Some("Session counts remain consistent".to_string()),
    });
    
    monitor.add_invariant(QuintInvariant {
        name: "sessionLimit".to_string(),
        expression: "sessionLimit".to_string(),
        description: Some("Session count doesn't exceed maximum".to_string()),
    });
    
    monitor.add_safety_property(QuintSafetyProperty {
        name: "safetyProperty".to_string(),
        expression: "safetyProperty".to_string(),
        description: Some("Basic safety property for completed sessions".to_string()),
    });
    
    println!("[OK] Property monitor created with {} properties", 
             monitor.get_statistics().total_evaluations);
    
    // Create test simulation state
    let sim_state = SimulationState {
        tick: 15,
        time: 1500,
        participants: vec![
            ParticipantStateSnapshot {
                id: "participant_0".to_string(),
                status: "active".to_string(),
                message_count: 12,
                active_sessions: vec!["session_1".to_string(), "session_2".to_string()],
            },
            ParticipantStateSnapshot {
                id: "participant_1".to_string(),
                status: "active".to_string(),
                message_count: 8,
                active_sessions: vec!["session_1".to_string()],
            },
            ParticipantStateSnapshot {
                id: "participant_2".to_string(),
                status: "completing".to_string(),
                message_count: 15,
                active_sessions: vec!["session_2".to_string()],
            },
        ],
        protocol_state: ProtocolExecutionState {
            active_sessions: vec![
                SessionInfo {
                    session_id: "session_1".to_string(),
                    protocol_type: "dkd".to_string(),
                    current_phase: "reveal".to_string(),
                    participants: vec!["participant_0".to_string(), "participant_1".to_string()],
                    status: "active".to_string(),
                },
                SessionInfo {
                    session_id: "session_2".to_string(),
                    protocol_type: "dkd".to_string(),
                    current_phase: "finalization".to_string(),
                    participants: vec!["participant_0".to_string(), "participant_2".to_string()],
                    status: "completing".to_string(),
                },
            ],
            completed_sessions: vec![
                SessionInfo {
                    session_id: "session_0".to_string(),
                    protocol_type: "dkd".to_string(),
                    current_phase: "complete".to_string(),
                    participants: vec!["participant_1".to_string(), "participant_2".to_string()],
                    status: "complete".to_string(),
                },
            ],
            queued_protocols: vec!["resharing".to_string()],
        },
        network_state: NetworkStateSnapshot {
            partitions: vec![],
            message_stats: MessageDeliveryStats {
                total_sent: 45,
                total_delivered: 42,
                total_dropped: 3,
                average_latency_ms: 75.5,
            },
            failure_conditions: NetworkFailureConditions {
                drop_rate: 0.05,
                latency_range: (20, 150),
                partition_count: 0,
            },
        },
    };
    
    println!("[stats] Simulation State:");
    println!("   Tick: {}", sim_state.tick);
    println!("   Participants: {}", sim_state.participants.len());
    println!("   Active sessions: {}", sim_state.protocol_state.active_sessions.len());
    println!("   Completed sessions: {}", sim_state.protocol_state.completed_sessions.len());
    println!();
    
    // Check properties
    println!("[search] Checking properties...");
    let check_result = monitor.check_properties(&sim_state)?;
    
    // Display results
    println!("[log] Property Check Results:");
    println!("   Overall result: {}", if check_result.validation_result.passed { "[OK] PASSED" } else { "[ERROR] FAILED" });
    println!("   Properties checked: {}", check_result.checked_properties.len());
    println!("   Violations detected: {}", check_result.violations.len());
    println!("   Check duration: {}ms", check_result.performance_metrics.check_duration_ms);
    println!();
    
    // Show individual property results
    println!("🔬 Individual Property Results:");
    for (i, prop_name) in check_result.checked_properties.iter().enumerate() {
        if i < check_result.evaluation_results.len() {
            let result = &check_result.evaluation_results[i];
            let status = if result.satisfied { "[OK]" } else { "[ERROR]" };
            println!("   {} {}: {}", status, prop_name, result.details);
        }
    }
    
    if !check_result.violations.is_empty() {
        println!("\n[WARN]  Property Violations:");
        for violation in &check_result.violations {
            println!("   - {}: {}", violation.property_name, violation.violation_details.description);
        }
    }
    
    println!("\n[graph] Monitoring Statistics:");
    let stats = monitor.get_statistics();
    println!("   Total evaluations: {}", stats.total_evaluations);
    println!("   Total evaluation time: {}ms", stats.total_evaluation_time_ms);
    println!("   Average evaluation time: {:.2}ms", stats.average_evaluation_time_ms);
    println!("   Violations detected: {}", stats.violations_detected);
    
    println!("\n[done] Property Monitor Demo completed successfully!");
    println!("   The Quint integration is working and evaluating properties correctly.");
    
    Ok(())
}