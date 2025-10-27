//! Decoupled Debugging Architecture Demonstration
//!
//! This example demonstrates the power of the new decoupled debugging architecture,
//! showing how debugging tools act as passive observers rather than wrappers.

use aura_simulator::{
    WorldState, tick,
    decoupled::{
        ScenarioEngine, ScenarioSetup, ParticipantSetup, NetworkCondition,
        TimeTravelDebugger, PassiveTraceRecorder, CheckpointManager,
        ReplayConfig, PropertyChecker
    },
    QueuedProtocol, NetworkPartition
};
use aura_console_types::{TraceEvent, trace::PropertyViolation};
use std::collections::HashMap;
use std::error::Error;
use tempfile::TempDir;
use uuid::Uuid;

/// Custom property checker that ensures participants stay active
struct ParticipantActivityChecker;

impl PropertyChecker for ParticipantActivityChecker {
    fn property_name(&self) -> &str {
        "participant_activity"
    }
    
    fn check_property(&self, world_state: &WorldState, _events: &[TraceEvent]) -> aura_simulator::Result<bool> {
        // Check that all participants are still active
        for participant in world_state.participants.values() {
            if participant.status != aura_console_types::SessionStatus::Active {
                return Ok(false);
            }
        }
        Ok(true)
    }
    
    fn description(&self) -> &str {
        "Ensures all participants remain active throughout simulation"
    }
}

/// Custom property checker for Byzantine behavior detection
struct ByzantineDetectionChecker;

impl PropertyChecker for ByzantineDetectionChecker {
    fn property_name(&self) -> &str {
        "byzantine_detection"
    }
    
    fn check_property(&self, world_state: &WorldState, _events: &[TraceEvent]) -> aura_simulator::Result<bool> {
        // Property holds if we have fewer than 1/3 byzantine participants
        let total_participants = world_state.participants.len();
        let byzantine_count = world_state.byzantine.byzantine_participants.len();
        
        Ok(byzantine_count * 3 < total_participants)
    }
    
    fn description(&self) -> &str {
        "Ensures byzantine participants are less than 1/3 of total"
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== Decoupled Debugging Architecture Demo ===\\n");

    // Demonstrate the core architectural benefits
    demonstrate_passive_observers()?;
    println!();
    
    demonstrate_time_travel_debugging()?;
    println!();
    
    demonstrate_scenario_engine()?;
    println!();
    
    demonstrate_property_checking()?;
    println!();
    
    demonstrate_failure_analysis()?;

    Ok(())
}

/// Demonstrate passive observer pattern
fn demonstrate_passive_observers() -> Result<(), Box<dyn Error>> {
    println!("1. Passive Observer Pattern");
    println!("===========================");

    let temp_dir = TempDir::new()?;
    
    // Create debugging tools as independent observers
    let mut trace_recorder = PassiveTraceRecorder::new();
    let mut checkpoint_manager = CheckpointManager::new(temp_dir.path())?;
    
    trace_recorder.set_scenario_name("passive_demo".to_string());
    trace_recorder.set_seed(42);
    
    println!("[OK] Created passive debugging tools:");
    println!("  - PassiveTraceRecorder: Records events without affecting simulation");
    println!("  - CheckpointManager: Saves/loads state without simulation knowledge");

    // Create simulation state - pure data container
    let mut world = WorldState::new(42);
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
    
    println!("[OK] Created WorldState with 2 participants");

    // Run simulation and observe with passive tools
    for tick_num in 0..5 {
        // Pure simulation step
        let events = tick(&mut world)?;
        
        // Passive observation - no coupling to simulation
        trace_recorder.record_tick_events(&events);
        
        // Create checkpoint every 2 ticks
        if tick_num % 2 == 0 {
            let checkpoint_id = checkpoint_manager.save(
                &world, 
                Some(format!("tick_{}", tick_num))
            )?;
            println!("  [checkpoint] Checkpoint created: {}", &checkpoint_id[..8]);
        }
        
        println!("  [target] Tick {}: {} events recorded", tick_num, events.len());
    }

    println!("[OK] Debugging tools observed simulation passively:");
    println!("  - Recorded {} events total", trace_recorder.event_count());
    println!("  - Created {} checkpoints", checkpoint_manager.checkpoint_count());
    println!("  - No coupling between simulation and debugging");
    println!("  - Core simulation stays pure and fast");

    Ok(())
}

/// Demonstrate time travel debugging capabilities
fn demonstrate_time_travel_debugging() -> Result<(), Box<dyn Error>> {
    println!("2. Time Travel Debugging");
    println!("========================");

    let temp_dir = TempDir::new()?;
    let mut checkpoint_manager = CheckpointManager::new(temp_dir.path())?;
    
    // Set up initial simulation state
    let mut world = WorldState::new(42);
    world.add_participant("alice".to_string(), "device_alice".to_string(), "account_1".to_string())?;
    world.add_participant("bob".to_string(), "device_bob".to_string(), "account_1".to_string())?;
    
    // Run simulation to tick 10 and create checkpoint
    for _ in 0..10 {
        let _ = tick(&mut world)?;
    }
    
    let checkpoint_id = checkpoint_manager.save(&world, Some("before_failure".to_string()))?;
    println!("[OK] Created checkpoint at tick 10: 'before_failure'");
    
    // Continue simulation to tick 20
    for _ in 0..10 {
        let _ = tick(&mut world)?;
    }
    println!("[OK] Continued simulation to tick 20");

    // Now use time travel debugger to investigate
    let mut debugger = TimeTravelDebugger::new(temp_dir.path())?;
    
    // Configure for detailed debugging
    let config = ReplayConfig {
        stop_on_violation: false,
        max_replay_ticks: 50,
        record_replay_events: true,
        step_by_step: false,
        ..Default::default()
    };
    debugger.configure(config);
    
    // Start debugging session from checkpoint
    debugger.start_session(
        &checkpoint_id,
        "Investigating simulation behavior".to_string(),
        Some(15),
    )?;
    
    println!("[OK] Started time travel debugging session");
    
    // Replay from checkpoint to target tick
    let result = debugger.replay_to_tick(15)?;
    
    println!("[OK] Time travel replay completed:");
    println!("  - Reached target: {}", result.reached_target);
    println!("  - Final tick: {}", result.final_tick);
    println!("  - Events replayed: {}", result.replay_events.len());
    println!("  - Replay time: {} ms", result.metrics.replay_time_ms);
    
    // Step forward one tick to demonstrate precise control
    let step_events = debugger.step_forward()?;
    println!("[OK] Stepped forward one tick, generated {} events", step_events.len());
    
    println!("[OK] Time travel debugging benefits:");
    println!("  - Precise replay from any checkpoint");
    println!("  - Step-by-step analysis capability");
    println!("  - Performance metrics for optimization");
    println!("  - Uses pure tick() function for consistency");

    Ok(())
}

/// Demonstrate high-level scenario coordination
fn demonstrate_scenario_engine() -> Result<(), Box<dyn Error>> {
    println!("3. Scenario Engine Coordination");
    println!("===============================");

    let temp_dir = TempDir::new()?;
    let mut engine = ScenarioEngine::new(temp_dir.path())?;
    
    // Add custom property checkers
    engine.add_property_checker(Box::new(ParticipantActivityChecker));
    engine.add_property_checker(Box::new(ByzantineDetectionChecker));
    
    println!("[OK] Created ScenarioEngine with custom property checkers");

    // Define a complex scenario
    let scenario_setup = ScenarioSetup {
        seed: 42,
        participants: vec![
            ParticipantSetup {
                participant_id: "alice".to_string(),
                device_id: "device_alice".to_string(),
                account_id: "shared_account".to_string(),
                is_byzantine: false,
            },
            ParticipantSetup {
                participant_id: "bob".to_string(),
                device_id: "device_bob".to_string(),
                account_id: "shared_account".to_string(),
                is_byzantine: false,
            },
            ParticipantSetup {
                participant_id: "charlie".to_string(),
                device_id: "device_charlie".to_string(),
                account_id: "shared_account".to_string(),
                is_byzantine: true, // This will trigger byzantine detection
            },
        ],
        network_conditions: vec![
            NetworkCondition::Partition {
                participants: vec!["alice".to_string(), "bob".to_string()],
                duration_ticks: 10,
                start_tick: 5,
            },
        ],
        byzantine_behaviors: Vec::new(),
        queued_protocols: Vec::new(),
    };
    
    println!("[OK] Defined complex scenario:");
    println!("  - 3 participants (1 byzantine)");
    println!("  - Network partition between alice and bob");
    println!("  - Custom property checking enabled");

    // Start and run the scenario
    let scenario_id = engine.start_scenario(
        scenario_setup,
        "Byzantine Network Test".to_string(),
        "Testing byzantine behavior with network partitions".to_string(),
    )?;
    
    let result = engine.run_scenario(&scenario_id, 20)?;
    
    println!("[OK] Scenario execution completed:");
    println!("  - Status: {:?}", result.final_status);
    println!("  - Events generated: {}", result.events_generated.len());
    println!("  - Property violations: {}", result.violations_found.len());
    println!("  - Execution time: {} ms", result.execution_time_ms);
    
    // Generate comprehensive report
    let report = engine.export_scenario_report(&scenario_id)?;
    println!("[OK] Generated comprehensive scenario report:");
    println!("  - Total events: {}", report.trace_summary.total_events);
    println!("  - Participant activity: {:?}", report.trace_summary.participant_activity);
    println!("  - Checkpoints available: {}", report.checkpoints_available);

    println!("[OK] Scenario engine benefits:");
    println!("  - High-level coordination of debugging tools");
    println!("  - Automated property checking");
    println!("  - Complex scenario setup and execution");
    println!("  - Comprehensive reporting and analysis");

    Ok(())
}

/// Demonstrate custom property checking
fn demonstrate_property_checking() -> Result<(), Box<dyn Error>> {
    println!("4. Custom Property Checking");
    println!("===========================");

    let temp_dir = TempDir::new()?;
    
    // Create a simple scenario with property violations
    let mut world = WorldState::new(42);
    world.add_participant("alice".to_string(), "device_alice".to_string(), "account_1".to_string())?;
    world.add_participant("bob".to_string(), "device_bob".to_string(), "account_1".to_string())?;
    world.add_participant("charlie".to_string(), "device_charlie".to_string(), "account_1".to_string())?;
    
    // Make 2 out of 3 participants byzantine (violates 1/3 rule)
    world.byzantine.byzantine_participants.push("alice".to_string());
    world.byzantine.byzantine_participants.push("bob".to_string());
    
    let mut trace_recorder = PassiveTraceRecorder::new();
    
    // Create property checkers
    let activity_checker = ParticipantActivityChecker;
    let byzantine_checker = ByzantineDetectionChecker;
    
    println!("[OK] Set up scenario with 2/3 byzantine participants");
    println!("[OK] Created custom property checkers");

    // Run simulation with property checking
    for tick_num in 0..5 {
        let events = tick(&mut world)?;
        trace_recorder.record_tick_events(&events);
        
        // Check activity property
        let activity_holds = activity_checker.check_property(&world, &events)?;
        
        // Check byzantine property
        let byzantine_holds = byzantine_checker.check_property(&world, &events)?;
        
        println!("  [search] Tick {}: Activity property: {}, Byzantine property: {}", 
                 tick_num, activity_holds, byzantine_holds);
        
        if !byzantine_holds {
            let violation = PropertyViolation {
                tick: world.current_tick,
                property: byzantine_checker.property_name().to_string(),
                participant: "system".to_string(),
                details: "Too many byzantine participants detected".to_string(),
            };
            trace_recorder.record_violation(violation);
            println!("  [ERROR] Property violation recorded!");
        }
    }

    println!("[OK] Property checking results:");
    println!("  - Total violations: {}", trace_recorder.violations().len());
    println!("  - Properties checked: activity, byzantine detection");
    println!("  - Violations automatically recorded in trace");

    println!("[OK] Property checking benefits:");
    println!("  - Extensible framework for custom properties");
    println!("  - Automatic violation detection and recording");
    println!("  - Integration with debugging tools");
    println!("  - Clear separation from simulation logic");

    Ok(())
}

/// Demonstrate comprehensive failure analysis
fn demonstrate_failure_analysis() -> Result<(), Box<dyn Error>> {
    println!("5. Comprehensive Failure Analysis");
    println!("=================================");

    let temp_dir = TempDir::new()?;
    
    // Set up a scenario that will have a "failure" at tick 15
    let mut world = WorldState::new(42);
    world.add_participant("alice".to_string(), "device_alice".to_string(), "account_1".to_string())?;
    world.add_participant("bob".to_string(), "device_bob".to_string(), "account_1".to_string())?;
    
    let mut checkpoint_manager = CheckpointManager::new(temp_dir.path())?;
    let mut trace_recorder = PassiveTraceRecorder::new();
    
    // Run simulation and create checkpoints along the way
    for tick_num in 0..20 {
        let events = tick(&mut world)?;
        trace_recorder.record_tick_events(&events);
        
        // Create checkpoints every 5 ticks
        if tick_num % 5 == 0 {
            checkpoint_manager.save(&world, Some(format!("checkpoint_tick_{}", tick_num)))?;
        }
        
        // Simulate a "failure" at tick 15
        if tick_num == 15 {
            let violation = PropertyViolation {
                tick: world.current_tick,
                property: "system_stability".to_string(),
                participant: "alice".to_string(),
                details: "Simulated failure for demonstration".to_string(),
            };
            trace_recorder.record_violation(violation);
            println!("  [ERROR] Simulated failure at tick 15");
        }
    }
    
    println!("[OK] Simulation completed with failure at tick 15");
    println!("[OK] Created {} checkpoints and recorded {} events", 
             checkpoint_manager.checkpoint_count(), trace_recorder.event_count());

    // Now analyze the failure using all debugging tools
    println!("[search] Starting comprehensive failure analysis...");
    
    // 1. Find checkpoint before failure
    let mut debugger = TimeTravelDebugger::new(temp_dir.path())?;
    let closest_checkpoint = debugger.find_checkpoint_before_failure(15);
    
    if let Some(checkpoint) = closest_checkpoint {
        println!("[OK] Found checkpoint before failure: '{}' at tick {}", 
                 checkpoint.label.as_ref().unwrap_or(&"unlabeled".to_string()), 
                 checkpoint.tick);
        
        // 2. Start debugging session from that checkpoint
        debugger.start_session(
            &checkpoint.id,
            "Failure analysis session".to_string(),
            Some(15),
        )?;
        
        // 3. Replay to failure point
        let replay_result = debugger.replay_to_tick(15)?;
        println!("[OK] Replayed to failure point: {} events generated", 
                 replay_result.replay_events.len());
        
        // 4. Analyze events around failure
        let analysis = debugger.analyze_events_around_tick(15, 3)?;
        println!("[OK] Event analysis around failure:");
        println!("  - Events in range: {}", analysis.total_events);
        println!("  - Participant activity: {:?}", analysis.participant_activity);
        println!("  - Violations found: {}", analysis.violations.len());
        
        // 5. Export debugging report
        let report_path = temp_dir.path().join("failure_analysis_report.json");
        debugger.export_report(&report_path)?;
        println!("[OK] Exported detailed debugging report");
    }

    println!("[OK] Comprehensive failure analysis complete!");
    println!("[OK] Analysis workflow:");
    println!("  1. Automatic checkpoint identification");
    println!("  2. Time travel to pre-failure state");  
    println!("  3. Precise replay to failure point");
    println!("  4. Event analysis around failure");
    println!("  5. Detailed report generation");
    
    println!("[OK] Decoupled architecture benefits:");
    println!("  - Tools work independently of simulation");
    println!("  - Easy to add new debugging capabilities");
    println!("  - No performance impact on core simulation");
    println!("  - Comprehensive analysis workflows");
    println!("  - Clear separation of concerns");

    Ok(())
}

/// Show the architectural comparison
#[allow(dead_code)]
fn show_architectural_comparison() {
    println!("=== Decoupled vs Coupled Debugging ===\\n");

    println!("BEFORE (Coupled Debugging):");
    println!("- Debugging tools wrapped around simulation");
    println!("- Tight coupling between debug logic and simulation");
    println!("- Performance impact even when debugging not used");
    println!("- Hard to add new debugging capabilities");
    println!("- Complex object hierarchies and dependencies");

    println!();
    println!("AFTER (Decoupled Debugging):");
    println!("- PassiveTraceRecorder: External event observer");
    println!("- CheckpointManager: Standalone state serialization");
    println!("- TimeTravelDebugger: Independent replay tool");
    println!("- ScenarioEngine: High-level coordination framework");
    println!("- PropertyChecker: Extensible validation system");

    println!();
    println!("Key Benefits:");
    println!("[OK] Zero coupling between simulation and debugging");
    println!("[OK] No performance impact when debugging not used");
    println!("[OK] Easy to add new debugging tools and capabilities");
    println!("[OK] Tools can be used independently or in combination");
    println!("[OK] Clear separation of concerns and responsibilities");
    println!("[OK] Passive observer pattern ensures simulation purity");
    println!("[OK] Comprehensive analysis workflows");
}