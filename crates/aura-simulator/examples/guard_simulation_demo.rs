//! Demonstration of SimulationEffectInterpreter usage
//!
//! This example shows how to use the SimulationEffectInterpreter for
//! deterministic simulation of guard evaluation and effect execution.

use aura_core::{
    effects::{
        guard::{
            EffectCommand, EffectInterpreter, FlowBudgetView, GuardOutcome,
            GuardSnapshot, JournalEntry, MetadataView, SimulationEvent,
        },
        NetworkAddress,
    },
    identifiers::{AuthorityId, ContextId},
    journal::{Cap, Fact},
    time::TimeStamp,
};
use aura_simulator::effects::SimulationEffectInterpreter;

/// Example guard function that evaluates a request
fn evaluate_request_guard(snapshot: &GuardSnapshot, request_type: &str) -> GuardOutcome {
    // Check if user is authorized based on metadata
    let is_authorized = snapshot
        .metadata
        .get("user:authorized")
        .map(|v| v == "true")
        .unwrap_or(false);

    if !is_authorized {
        return GuardOutcome::denied("User not authorized");
    }

    // Check flow budget
    let context = ContextId::new(); // Would come from request context
    let authority = AuthorityId::new(); // Would come from request
    let required_budget = match request_type {
        "read" => 10,
        "write" => 50,
        "admin" => 100,
        _ => 25,
    };

    if !snapshot.budgets.has_budget(&context, &authority, required_budget) {
        return GuardOutcome::denied("Insufficient flow budget");
    }

    // Build effects for authorized request
    let mut effects = vec![
        // Charge flow budget
        EffectCommand::ChargeBudget {
            context,
            authority,
            peer: authority, // Peer would be the requesting authority
            amount: required_budget,
        },
        // Record metadata leakage (request type)
        EffectCommand::RecordLeakage { bits: 8 },
    ];

    // Add request-specific effects
    match request_type {
        "write" => {
            effects.push(EffectCommand::AppendJournal {
                entry: JournalEntry {
                    fact: Fact::default(), // Would contain actual write data
                    authority,
                    timestamp: snapshot.now.clone(),
                },
            });
        }
        "admin" => {
            effects.push(EffectCommand::StoreMetadata {
                key: "last_admin_access".to_string(),
                value: format!("{:?}", snapshot.now),
            });
        }
        _ => {}
    }

    // Send response
    effects.push(EffectCommand::SendEnvelope {
        to: NetworkAddress::new("test://client".to_string()),
        envelope: vec![1, 2, 3], // Mock response
    });

    GuardOutcome::authorized(effects)
}

#[tokio::main]
async fn main() {
    println!("=== Simulation Effect Interpreter Demo ===\n");

    // Initialize simulation
    use aura_core::time::PhysicalTime;
    let initial_time = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });
    let authority = AuthorityId::new();
    let interpreter = SimulationEffectInterpreter::new(
        42, // Deterministic seed
        initial_time.clone(),
        authority,
        NetworkAddress::new("demo://server".to_string()),
    );

    // Set up initial state
    interpreter.set_initial_budget(authority, 200);
    interpreter
        .execute(EffectCommand::StoreMetadata {
            key: "user:authorized".to_string(),
            value: "true".to_string(),
        })
        .await
        .unwrap();

    println!("Initial state:");
    println!("  Authority: {:?}", authority);
    println!("  Budget: 200");
    println!("  User authorized: true\n");

    // Simulate multiple requests
    let requests = vec![
        ("read", "Read operation"),
        ("write", "Write operation"),
        ("read", "Another read"),
        ("admin", "Admin operation"),
        ("write", "Final write"),
    ];

    for (request_type, description) in &requests {
        println!("Processing: {} - {}", request_type, description);

        // Create guard snapshot from current state
        let state = interpreter.snapshot_state();
        // Convert flow_budgets to include context
        let context = ContextId::new();
        let budgets_with_context: std::collections::HashMap<(ContextId, AuthorityId), u32> =
            state.flow_budgets.iter()
                .map(|(auth, amount)| ((context, *auth), *amount))
                .collect();
        let snapshot = GuardSnapshot {
            now: state.current_time,
            caps: Cap::new(),
            budgets: FlowBudgetView::new(budgets_with_context),
            metadata: MetadataView::new(state.metadata.clone()),
            rng_seed: [0; 32], // Would be properly initialized
        };

        // Evaluate guard
        let outcome = evaluate_request_guard(&snapshot, request_type);

        if outcome.is_authorized() {
            println!("  ✓ Authorized");

            // Execute effects
            for effect in outcome.effects {
                match &effect {
                    EffectCommand::ChargeBudget { amount, .. } => {
                        println!("  - Charging {} budget", amount);
                    }
                    EffectCommand::AppendJournal { .. } => {
                        println!("  - Appending to journal");
                    }
                    EffectCommand::StoreMetadata { key, .. } => {
                        println!("  - Storing metadata: {}", key);
                    }
                    _ => {}
                }

                interpreter.execute(effect).await.unwrap();
            }

            let new_budget = interpreter.state().get_budget(&authority);
            println!("  - Remaining budget: {}", new_budget);
        } else {
            println!("  ✗ Denied: {}", outcome.decision.denial_reason().unwrap());
        }

        println!();
    }

    // Show final state
    println!("=== Final State ===");
    let final_state = interpreter.snapshot_state();
    println!("Total events recorded: {}", final_state.events.len());
    println!("Journal entries: {}", final_state.journal.len());
    println!("Messages queued: {}", final_state.message_queue.len());
    println!("Total leakage bits: {}", final_state.total_leakage_bits);
    println!("Final budget: {}", final_state.get_budget(&authority));

    // Demonstrate event analysis
    println!("\n=== Event Analysis ===");
    let budget_events =
        interpreter.events_of_type(|e| matches!(e, SimulationEvent::BudgetCharged { .. }));
    println!("Budget charges: {}", budget_events.len());

    for event in &budget_events {
        if let SimulationEvent::BudgetCharged {
            amount, remaining, ..
        } = event
        {
            println!("  - Charged {}, remaining {}", amount, remaining);
        }
    }

    // Demonstrate replay capability
    println!("\n=== Testing Replay ===");
    let events = interpreter.events();

    // Create new interpreter with different seed
    let replay_interpreter = SimulationEffectInterpreter::new(
        99, // Different seed
        initial_time,
        authority,
        NetworkAddress::new("demo://replay".to_string()),
    );

    // Set same initial conditions
    replay_interpreter.set_initial_budget(authority, 200);

    // Replay events
    replay_interpreter.replay(events).await.unwrap();

    let replay_state = replay_interpreter.snapshot_state();
    println!("Replay successful!");
    println!(
        "  Original final budget: {}",
        final_state.get_budget(&authority)
    );
    println!(
        "  Replayed final budget: {}",
        replay_state.get_budget(&authority)
    );
    println!(
        "  States match: {}",
        final_state.get_budget(&authority) == replay_state.get_budget(&authority)
    );
}
