//! Example showing how to migrate from SendGuardChain to pure guards
//!
//! This example demonstrates:
//! 1. Using the pure guard chain with existing effect interpreters
//! 2. Converting old SendGuardChain usage to the new model
//! 3. Custom guard implementation
//! 4. Simulation with deterministic effect interpretation (no runtime dependency)

use aura_core::{
    effects::{
        guard::{
            EffectCommand, EffectInterpreter, EffectResult, GuardOutcome, GuardSnapshot,
            JournalEntry,
        },
        FlowBudgetView, MetadataView,
    },
    identifiers::{AuthorityId, ContextId},
    journal::Cap,
    time::{PhysicalTime, TimeStamp},
    AuraResult,
};
use aura_protocol::guards::pure::{Guard, GuardChain, GuardRequest};
// Note: Examples can use tokio::main for simplicity (arch-check accepts this for examples/)
use std::collections::HashMap;

/// Example custom guard that checks domain-specific rules
#[derive(Debug)]
struct DomainSpecificGuard {
    max_message_size: usize,
}

impl Guard for DomainSpecificGuard {
    fn evaluate(&self, _snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
        if request.operation.len() > self.max_message_size {
            return GuardOutcome::denied(format!(
                "Operation size {} exceeds limit {}",
                request.operation.len(),
                self.max_message_size
            ));
        }

        GuardOutcome::authorized(vec![])
    }

    fn name(&self) -> &'static str {
        "DomainSpecificGuard"
    }
}

/// Example deterministic interpreter for simulation
struct SimulationInterpreter {
    events: Vec<String>,
    flow_budgets: HashMap<(ContextId, AuthorityId), u32>,
    journal_entries: Vec<JournalEntry>,
}

impl SimulationInterpreter {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            flow_budgets: HashMap::new(),
            journal_entries: Vec::new(),
        }
    }

    fn set_budget(&mut self, context: ContextId, authority: AuthorityId, budget: u32) {
        self.flow_budgets.insert((context, authority), budget);
    }
}

#[async_trait::async_trait]
impl EffectInterpreter for SimulationInterpreter {
    async fn execute(&self, cmd: EffectCommand) -> AuraResult<EffectResult> {
        match cmd {
            EffectCommand::ChargeBudget {
                context,
                authority,
                amount,
                peer: _,
            } => {
                let current = self
                    .flow_budgets
                    .get(&(context, authority))
                    .copied()
                    .unwrap_or(0);
                if current < amount {
                    Ok(EffectResult::Failure("Insufficient budget".to_string()))
                } else {
                    Ok(EffectResult::RemainingBudget(current - amount))
                }
            }
            EffectCommand::AppendJournal { entry } => {
                let mut entries = self.journal_entries.clone();
                entries.push(entry);
                Ok(EffectResult::Success)
            }
            EffectCommand::RecordLeakage { bits } => {
                let mut events = self.events.clone();
                events.push(format!("Leakage recorded: {bits} bits"));
                Ok(EffectResult::Success)
            }
            _ => Ok(EffectResult::Success),
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "SimulationInterpreter"
    }
}

async fn run_examples() -> AuraResult<()> {
    // Example 1: Using pure guards with custom domain rules
    println!("=== Example 1: Custom Domain Guard ===");

    let guard_chain = GuardChain::standard().with(Box::new(DomainSpecificGuard {
        max_message_size: 1024,
    }));

    let authority = AuthorityId::new();
    let context = ContextId::new();
    let mut budgets = HashMap::new();
    budgets.insert((context, authority), 1000);

    let snapshot = GuardSnapshot {
        now: TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        }),
        caps: Cap::default(),
        budgets: FlowBudgetView::new(budgets),
        metadata: MetadataView::default(),
        rng_seed: [0u8; 32],
    };

    let request =
        GuardRequest::new(authority, "send_message", 100).with_context(b"Hello, World!".to_vec());

    let mut interpreter = SimulationInterpreter::new();
    interpreter.set_budget(context, authority, 1000);

    let outcome = guard_chain.evaluate(&snapshot, &request);
    let executed = run_effects(&interpreter, &outcome.effects).await?;

    println!("Authorized: {}", outcome.is_authorized());
    println!("Effects executed: {}", executed);

    // Example 2: Migration from SendGuardChain
    println!("\n=== Example 2: Migration Path ===");

    // Old way (would use SendGuardChain)
    // let send_guard = SendGuardChain::new(
    //     "message:send".to_string(),
    //     context,
    //     peer,
    //     100,
    // );
    // let result = send_guard.evaluate(&effect_system).await?;

    // New way - same behavior, pure implementation
    let request = GuardRequest::new(authority, "message:send", 100).with_capability(Cap::default());

    let guard_chain = GuardChain::standard();
    let interpreter = SimulationInterpreter::new();
    let outcome = guard_chain.evaluate(&snapshot, &request);
    let executed = run_effects(&interpreter, &outcome.effects).await?;
    println!("Migration result - Authorized: {}", outcome.is_authorized());
    println!("Effects executed: {}", executed);

    // Example 3: Testing guard logic without I/O
    println!("\n=== Example 3: Pure Testing ===");

    // Test insufficient budget scenario
    let expensive_request = GuardRequest::new(authority, "expensive_op", 2000);
    let result = guard_chain.evaluate(&snapshot, &expensive_request);

    println!("Expensive operation denied: {}", !result.is_authorized());
    println!("Denial reason: {:?}", result.decision.denial_reason());

    Ok(())
}

async fn run_effects(
    interpreter: &SimulationInterpreter,
    commands: &[EffectCommand],
) -> AuraResult<usize> {
    let mut executed = 0;

    for command in commands {
        interpreter.execute(command.clone()).await?;
        executed += 1;
    }

    Ok(executed)
}

#[tokio::main]
async fn main() -> AuraResult<()> {
    run_examples().await
}
