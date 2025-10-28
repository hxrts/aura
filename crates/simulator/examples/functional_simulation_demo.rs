//! Functional Simulation Architecture Demonstration
//!
//! This example demonstrates the benefits of the new functional approach
//! to simulation state management, showing the separation of state from logic.

use aura_simulator::{
    tick, ByzantineStrategy, FunctionalRunner, NetworkPartition, QueuedProtocol, WorldState,
};
use std::collections::HashMap;
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Functional Simulation Architecture Demo ===\n");

    // Demonstrate the core architectural benefits
    demonstrate_pure_functions()?;
    println!();

    demonstrate_runner_separation()?;
    println!();

    demonstrate_deterministic_testing()?;
    println!();

    demonstrate_time_travel_debugging()?;
    println!();

    demonstrate_complex_scenario()?;

    Ok(())
}

/// Demonstrate pure functional state transitions
fn demonstrate_pure_functions() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Pure Functional State Transitions");
    println!("=====================================");

    // Create a world state - pure data container
    let mut world = WorldState::new(42);
    println!("[OK] Created pure WorldState (no methods, just data)");

    // Add participants to the state
    world.add_participant(
        "alice".to_string(),
        "device_alice".to_string(),
        "account_1".to_string(),
    )?;

    world.add_participant(
        "bob".to_string(),
        "device_bob".to_string(),
        "account_1".to_string(),
    )?;

    println!("[OK] Added participants to world state");

    // Use pure tick function - takes state, returns events
    let events = tick(&mut world)?;

    println!("[OK] Called pure tick() function");
    println!("  - Input: WorldState + Effects");
    println!("  - Output: Vec<TraceEvent>");
    println!("  - Side effects: Mutates WorldState in place");
    println!("  - Generated {} events", events.len());

    // The function is pure and predictable
    println!("[OK] Benefits of pure approach:");
    println!("  - Easy to test: give it a WorldState, get events");
    println!("  - Deterministic: same input → same output");
    println!("  - No hidden state or coupling");
    println!("  - Clear separation of concerns");

    Ok(())
}

/// Demonstrate separation of runner from core logic
fn demonstrate_runner_separation() -> Result<(), Box<dyn std::error::Error>> {
    println!("2. Runner Separation from Core Logic");
    println!("=====================================");

    // Create functional runner - handles execution harness
    let mut runner = FunctionalRunner::new(42)
        .with_auto_checkpoints(10)
        .with_max_checkpoints(50);

    println!("[OK] Created FunctionalRunner with execution logic");

    // Add participants through runner interface
    runner.add_participant(
        "alice".to_string(),
        "device_alice".to_string(),
        "account_1".to_string(),
    )?;

    runner.add_participant(
        "bob".to_string(),
        "device_bob".to_string(),
        "account_1".to_string(),
    )?;

    println!("[OK] Added participants through runner");

    // Runner handles complex execution patterns
    let result = runner.run_for_ticks(5)?;

    println!("[OK] Runner handled complex execution:");
    println!("  - Looping and iteration");
    println!("  - Automatic checkpointing");
    println!("  - Event collection");
    println!("  - Statistics tracking");
    println!(
        "  - Completed {} ticks with {} events",
        result.final_tick,
        result.event_trace.len()
    );

    // Core logic remains pure and simple
    println!("[OK] Core tick() function stays pure:");
    println!("  - No knowledge of runners or execution patterns");
    println!("  - No checkpointing logic");
    println!("  - No iteration or looping");
    println!("  - Just pure state transformation");

    Ok(())
}

/// Demonstrate deterministic testing with pure functions
fn demonstrate_deterministic_testing() -> Result<(), Box<dyn std::error::Error>> {
    println!("3. Deterministic Testing Benefits");
    println!("==================================");

    // Test the same scenario with two identical setups
    let test_scenario = |seed: u64| -> Result<Vec<u64>, Box<dyn std::error::Error>> {
        let mut world = WorldState::new(seed);

        world.add_participant(
            "alice".to_string(),
            "device_alice".to_string(),
            "account_1".to_string(),
        )?;

        // Queue a protocol for testing
        let protocol = QueuedProtocol {
            protocol_type: "DKD".to_string(),
            participants: vec!["alice".to_string()],
            parameters: HashMap::new(),
            scheduled_time: world.current_time + 100,
            priority: 0,
        };
        world.protocols.execution_queue.push_back(protocol);

        let mut tick_events = Vec::new();

        // Run several ticks and collect event counts
        for _ in 0..5 {
            let events = tick(&mut world)?;
            tick_events.push(events.len() as u64);
        }

        Ok(tick_events)
    };

    // Run same scenario twice
    let result1 = test_scenario(42)?;
    let result2 = test_scenario(42)?;

    println!("[OK] Ran identical scenarios with same seed");
    println!("  Result 1: {:?}", result1);
    println!("  Result 2: {:?}", result2);

    if result1 == result2 {
        println!("[OK] DETERMINISTIC: Same seed → same results");
    } else {
        println!("[ERROR] NON-DETERMINISTIC: Results differ!");
    }

    // Test with different seed
    let result3 = test_scenario(123)?;
    println!("  Result 3 (different seed): {:?}", result3);

    if result1 != result3 {
        println!("[OK] Different seed → different results (as expected)");
    }

    println!("[OK] Testing benefits:");
    println!("  - Pure functions are easy to test");
    println!("  - No setup or teardown needed");
    println!("  - Deterministic results");
    println!("  - Easy to create specific test scenarios");

    Ok(())
}

/// Demonstrate time travel debugging with checkpoints
fn demonstrate_time_travel_debugging() -> Result<(), Box<dyn std::error::Error>> {
    println!("4. Time Travel Debugging");
    println!("========================");

    let mut runner = FunctionalRunner::new(42);

    // Add participants
    runner.add_participant(
        "alice".to_string(),
        "device_alice".to_string(),
        "account_1".to_string(),
    )?;

    runner.add_participant(
        "bob".to_string(),
        "device_bob".to_string(),
        "account_1".to_string(),
    )?;

    println!("[OK] Set up simulation with 2 participants");

    // Run to a specific point
    runner.step_n(3)?;
    let checkpoint_tick = runner.current_tick();
    println!("[OK] Ran to tick {}", checkpoint_tick);

    // Create checkpoint
    let checkpoint_id = runner.create_checkpoint(Some("before_byzantine".to_string()))?;
    println!("[OK] Created checkpoint 'before_byzantine'");

    // Continue and make changes
    runner.step_n(5)?;
    let later_tick = runner.current_tick();
    println!("[OK] Continued to tick {}", later_tick);

    // Time travel back to checkpoint
    runner.restore_checkpoint(&checkpoint_id)?;
    println!(
        "[OK] Restored to checkpoint at tick {}",
        runner.current_tick()
    );

    if runner.current_tick() == checkpoint_tick {
        println!("[OK] TIME TRAVEL SUCCESS: Back to tick {}", checkpoint_tick);
    } else {
        println!("[ERROR] TIME TRAVEL FAILED");
    }

    // List all checkpoints
    let checkpoints = runner.list_checkpoints();
    println!("[OK] Available checkpoints: {}", checkpoints.len());
    for (id, label, tick) in checkpoints {
        println!(
            "  - {} '{}' at tick {}",
            &id[..8],
            label.unwrap_or("unlabeled".to_string()),
            tick
        );
    }

    println!("[OK] Time travel benefits:");
    println!("  - Easy debugging of complex scenarios");
    println!("  - Can explore 'what if' branches");
    println!("  - Simple state restoration");
    println!("  - No complex undo/redo logic needed");

    Ok(())
}

/// Demonstrate complex scenario with network failures and byzantine behavior
fn demonstrate_complex_scenario() -> Result<(), Box<dyn std::error::Error>> {
    println!("5. Complex Scenario: Network Failures + Byzantine Behavior");
    println!("===========================================================");

    let mut runner = FunctionalRunner::new(42).with_auto_checkpoints(10);

    // Set up a 3-participant scenario
    for i in 0..3 {
        runner.add_participant(
            format!("participant_{}", i),
            format!("device_{}", i),
            "shared_account".to_string(),
        )?;
    }

    println!("[OK] Created 3-participant simulation");

    // Directly manipulate world state to set up complex scenario
    {
        let world = runner.world_state_mut();

        // Add network partition
        let partition = NetworkPartition {
            id: Uuid::new_v4().to_string(),
            participants: vec!["participant_0".to_string(), "participant_1".to_string()],
            started_at: world.current_time,
            duration: Some(5000), // 5 second partition
        };
        world.network.partitions.push(partition);

        // Make one participant byzantine
        world
            .byzantine
            .byzantine_participants
            .push("participant_2".to_string());
        world.byzantine.active_strategies.insert(
            "participant_2".to_string(),
            ByzantineStrategy::DropAllMessages,
        );

        // Queue a protocol that will be affected
        let protocol = QueuedProtocol {
            protocol_type: "DKD".to_string(),
            participants: vec![
                "participant_0".to_string(),
                "participant_1".to_string(),
                "participant_2".to_string(),
            ],
            parameters: HashMap::new(),
            scheduled_time: world.current_time + 200,
            priority: 0,
        };
        world.protocols.execution_queue.push_back(protocol);
    }

    println!("[OK] Set up complex scenario:");
    println!("  - Network partition (participants 0,1 isolated from 2)");
    println!("  - Byzantine participant 2 (drops all messages)");
    println!("  - DKD protocol scheduled across all participants");

    // Run simulation and observe behavior
    let result = runner.run_for_ticks(20)?;

    println!("[OK] Simulation completed:");
    println!("  - Final tick: {}", result.final_tick);
    println!("  - Total events: {}", result.event_trace.len());
    println!("  - Stop reason: {:?}", result.stop_reason);

    // Show statistics
    let stats = runner.get_statistics();
    println!("[OK] Final statistics:");
    println!("  - Active sessions: {}", stats.active_sessions);
    println!("  - In-flight messages: {}", stats.in_flight_messages);
    println!(
        "  - Byzantine participants: {}",
        stats.byzantine_participants
    );
    println!("  - Network partitions: {}", stats.network_partitions);
    println!("  - Checkpoints created: {}", stats.checkpoints_created);

    // Export trace for analysis
    let trace = runner.export_trace();
    println!(
        "[OK] Exported complete trace with {} events",
        trace.timeline.len()
    );

    println!("[OK] Complex scenario benefits:");
    println!("  - Easy to set up intricate test conditions");
    println!("  - Pure state makes reasoning about interactions clear");
    println!("  - All behavior is captured in events");
    println!("  - Can replay and analyze any part of execution");

    Ok(())
}

/// Show the architectural comparison
#[allow(dead_code)]
fn show_architectural_comparison() {
    println!("=== Architectural Comparison ===\n");

    println!("BEFORE (Coupled Architecture):");
    println!("- CheckpointSimulation struct contains both state AND logic");
    println!("- tick() method is coupled to the simulation object");
    println!("- Hard to test individual state transitions");
    println!("- Complex object with mixed responsibilities");
    println!("- State and execution logic intertwined");

    println!();
    println!("AFTER (Functional Architecture):");
    println!("- WorldState: Pure data container (state only)");
    println!("- tick(): Pure function (logic only)");
    println!("- FunctionalRunner: Execution harness (control only)");
    println!("- Easy to test: give tick() any WorldState");
    println!("- Clear separation of concerns");
    println!("- Predictable, deterministic execution");

    println!();
    println!("Benefits:");
    println!("[OK] Pure functions are easier to test and reason about");
    println!("[OK] State snapshots are trivial (just clone WorldState)");
    println!("[OK] Time travel debugging is simple");
    println!("[OK] Different execution strategies can use same core logic");
    println!("[OK] Byzantine testing becomes straightforward");
    println!("[OK] Deterministic execution for reproducible tests");
}
