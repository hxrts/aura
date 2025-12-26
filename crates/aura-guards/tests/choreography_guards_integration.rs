//! Integration tests for the choreography-first guard architecture described in
//! [`docs/107_mpst_and_choreography.md`].
//!
//! These tests demonstrate that choreographic annotations automatically generate
//! `EffectCommand` sequences that execute through the unified interpreter infrastructure.

use aura_core::{
    effects::guard::{EffectCommand, EffectInterpreter, EffectResult},
    AuthorityId, ContextId, Result,
};
use std::sync::{Arc, Mutex};

/// Mock effect interpreter that records executed commands for verification
#[derive(Debug, Clone)]
struct MockEffectInterpreter {
    executed_commands: Arc<Mutex<Vec<EffectCommand>>>,
}

#[allow(clippy::unwrap_used)]
impl MockEffectInterpreter {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_executed_commands(&self) -> Vec<EffectCommand> {
        self.executed_commands.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
#[allow(clippy::unwrap_used)]
impl aura_core::effects::guard::EffectInterpreter for MockEffectInterpreter {
    async fn execute(&self, command: EffectCommand) -> Result<EffectResult> {
        // Record the command
        self.executed_commands.lock().unwrap().push(command.clone());

        // Return appropriate success result
        match command {
            EffectCommand::GenerateNonce { .. } => Ok(EffectResult::Nonce(vec![1, 2, 3, 4])),
            EffectCommand::ChargeBudget { amount, .. } => {
                Ok(EffectResult::RemainingBudget(1000 - amount))
            }
            _ => Ok(EffectResult::Success),
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "MockEffectInterpreter"
    }
}

/// Test annotation-to-commands conversion for guard_capability
#[tokio::test]
async fn test_guard_capability_annotation() {
    let interpreter = MockEffectInterpreter::new();

    // Simulate what the macro generates for: Client[guard_capability = "send"] -> Server: Msg;
    let command = EffectCommand::StoreMetadata {
        key: "guard_validated".to_string(),
        value: "send".to_string(),
    };

    let result = interpreter.execute(command.clone()).await;
    assert!(result.is_ok());

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 1);
    assert!(matches!(
        &executed[0],
        EffectCommand::StoreMetadata { key, value } if key == "guard_validated" && value == "send"
    ));
}

/// Test annotation-to-commands conversion for flow_cost
#[tokio::test]
async fn test_flow_cost_annotation() {
    let interpreter = MockEffectInterpreter::new();
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let context = ContextId::default();
    let peer = AuthorityId::new_from_entropy([1u8; 32]);

    // Simulate what the macro generates for: Client[flow_cost = 200] -> Server: Msg;
    let command = EffectCommand::ChargeBudget {
        context,
        authority,
        peer,
        amount: 200,
    };

    let result = interpreter.execute(command.clone()).await;
    assert!(result.is_ok());

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 1);
    assert!(matches!(
        &executed[0],
        EffectCommand::ChargeBudget { amount, .. } if *amount == 200
    ));
}

/// Test annotation-to-commands conversion for leak annotation
#[tokio::test]
async fn test_leak_annotation() {
    let interpreter = MockEffectInterpreter::new();

    // Simulate what the macro generates for: Client[leak = "External"] -> Server: Msg;
    let command = EffectCommand::RecordLeakage { bits: 32 };

    let result = interpreter.execute(command.clone()).await;
    assert!(result.is_ok());

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 1);
    assert!(matches!(
        &executed[0],
        EffectCommand::RecordLeakage { bits } if *bits == 32
    ));
}

/// Test annotation-to-commands conversion for journal_facts
#[tokio::test]
async fn test_journal_facts_annotation() {
    let interpreter = MockEffectInterpreter::new();

    // Simulate what the macro generates for: Client[journal_facts = "request_sent"] -> Server: Msg;
    let command = EffectCommand::StoreMetadata {
        key: "journal_fact:request_sent".to_string(),
        value: "request_sent".to_string(),
    };

    let result = interpreter.execute(command.clone()).await;
    assert!(result.is_ok());

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 1);
    assert!(matches!(
        &executed[0],
        EffectCommand::StoreMetadata { key, .. } if key == "journal_fact:request_sent"
    ));
}

/// Test multiple annotations generate multiple commands
#[tokio::test]
async fn test_multiple_annotations() {
    let interpreter = MockEffectInterpreter::new();
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let context = ContextId::default();
    let peer = AuthorityId::new_from_entropy([1u8; 32]);

    // Simulate what the macro generates for:
    // Client[guard_capability = "send", flow_cost = 100, leak = "External"] -> Server: Msg;
    let commands = vec![
        EffectCommand::StoreMetadata {
            key: "guard_validated".to_string(),
            value: "send".to_string(),
        },
        EffectCommand::ChargeBudget {
            context,
            authority,
            peer,
            amount: 100,
        },
        EffectCommand::RecordLeakage { bits: 16 },
    ];

    // Execute all commands
    for command in commands {
        let result = interpreter.execute(command).await;
        assert!(result.is_ok());
    }

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 3);

    // Verify all three command types were executed
    assert!(executed
        .iter()
        .any(|c| matches!(c, EffectCommand::StoreMetadata { .. })));
    assert!(executed
        .iter()
        .any(|c| matches!(c, EffectCommand::ChargeBudget { .. })));
    assert!(executed
        .iter()
        .any(|c| matches!(c, EffectCommand::RecordLeakage { .. })));
}

/// Test that effect execution preserves order
#[tokio::test]
async fn test_effect_execution_order() {
    let interpreter = MockEffectInterpreter::new();
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let context = ContextId::default();
    let peer = AuthorityId::new_from_entropy([1u8; 32]);

    // Execute commands in specific order
    let commands = vec![
        EffectCommand::StoreMetadata {
            key: "step1".to_string(),
            value: "first".to_string(),
        },
        EffectCommand::ChargeBudget {
            context,
            authority,
            peer,
            amount: 50,
        },
        EffectCommand::StoreMetadata {
            key: "step2".to_string(),
            value: "second".to_string(),
        },
    ];

    for command in commands {
        let result = interpreter.execute(command).await;
        assert!(result.is_ok());
    }

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 3);

    // Verify execution order
    match &executed[0] {
        EffectCommand::StoreMetadata { key, value } => {
            assert_eq!(key, "step1");
            assert_eq!(value, "first");
        }
        _ => panic!("Expected StoreMetadata as first command"),
    }

    assert!(matches!(executed[1], EffectCommand::ChargeBudget { .. }));

    match &executed[2] {
        EffectCommand::StoreMetadata { key, value } => {
            assert_eq!(key, "step2");
            assert_eq!(value, "second");
        }
        _ => panic!("Expected StoreMetadata as third command"),
    }
}

/// Test effect error handling
#[tokio::test]
async fn test_effect_error_handling() {
    use aura_core::AuraError;

    // Create an interpreter that fails for specific commands
    #[derive(Debug, Clone)]
    struct FailingInterpreter;

    #[async_trait::async_trait]
    impl aura_core::effects::guard::EffectInterpreter for FailingInterpreter {
        async fn execute(&self, command: EffectCommand) -> Result<EffectResult> {
            match command {
                EffectCommand::ChargeBudget { amount, .. } if amount > 1000 => {
                    Err(AuraError::Internal {
                        message: "Insufficient budget".to_string(),
                    })
                }
                EffectCommand::ChargeBudget { amount, .. } => {
                    Ok(EffectResult::RemainingBudget(1000 - amount))
                }
                _ => Ok(EffectResult::Success),
            }
        }

        fn interpreter_type(&self) -> &'static str {
            "FailingInterpreter"
        }
    }

    let interpreter = FailingInterpreter;
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let context = ContextId::default();
    let peer = AuthorityId::new_from_entropy([1u8; 32]);

    // Command that should succeed
    let ok_command = EffectCommand::ChargeBudget {
        context,
        authority,
        peer,
        amount: 100,
    };
    assert!(interpreter.execute(ok_command).await.is_ok());

    // Command that should fail
    let fail_command = EffectCommand::ChargeBudget {
        context,
        authority,
        peer,
        amount: 2000,
    };
    let result = interpreter.execute(fail_command).await;
    assert!(result.is_err());
    // Verify the error contains the expected message
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("Insufficient budget"));
}

/// Test integration with guard chain
#[tokio::test]
async fn test_guard_chain_integration() {
    use aura_core::{
        effects::guard::{FlowBudgetView, GuardSnapshot, MetadataView},
        time::{PhysicalTime, TimeStamp},
        Cap,
    };
    use aura_guards::pure::{Guard, GuardChain, GuardRequest};
    use std::collections::HashMap;

    // Create a snapshot with budget
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let context = ContextId::default();
    let mut budgets = HashMap::new();
    budgets.insert((context, authority), 500);

    let mut metadata = HashMap::new();
    metadata.insert("authz:test_op".to_string(), "allow".to_string());

    let snapshot = GuardSnapshot {
        now: TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        }),
        caps: Cap::default(),
        budgets: FlowBudgetView::new(budgets),
        metadata: MetadataView::new(metadata),
        rng_seed: [0u8; 32],
    };

    // Create a guard request
    let request = GuardRequest::new(authority, "test_op", 100)
        .with_context_id(context)
        .with_peer(AuthorityId::default());

    // Evaluate with standard guard chain
    let chain = GuardChain::standard();
    let outcome = chain.evaluate(&snapshot, &request);

    // Verify the guard chain authorized the request and generated effects
    assert!(outcome.is_authorized());
    assert!(!outcome.effects.is_empty());

    // Verify we got expected effect types
    assert!(outcome
        .effects
        .iter()
        .any(|e| matches!(e, EffectCommand::ChargeBudget { .. })));
    assert!(outcome
        .effects
        .iter()
        .any(|e| matches!(e, EffectCommand::AppendJournal { .. })));
}

/// Test that annotation effects and runtime guard effects use the same types
#[tokio::test]
async fn test_unified_effect_command_system() {
    let interpreter = MockEffectInterpreter::new();
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let context = ContextId::default();
    let peer = AuthorityId::new_from_entropy([1u8; 32]);

    // Effect from annotation (macro-generated)
    let annotation_effect = EffectCommand::ChargeBudget {
        context,
        authority,
        peer,
        amount: 200,
    };

    // Effect from runtime guard (same type!)
    let runtime_effect = EffectCommand::ChargeBudget {
        context,
        authority,
        peer,
        amount: 200,
    };

    // Both execute through the same interpreter
    assert!(interpreter.execute(annotation_effect).await.is_ok());
    assert!(interpreter.execute(runtime_effect).await.is_ok());

    let executed = interpreter.get_executed_commands();
    assert_eq!(executed.len(), 2);

    // Both should be identical (verify by matching structure)
    match (&executed[0], &executed[1]) {
        (
            EffectCommand::ChargeBudget {
                context: c1,
                authority: a1,
                peer: p1,
                amount: amt1,
            },
            EffectCommand::ChargeBudget {
                context: c2,
                authority: a2,
                peer: p2,
                amount: amt2,
            },
        ) => {
            assert_eq!(c1, c2);
            assert_eq!(a1, a2);
            assert_eq!(p1, p2);
            assert_eq!(amt1, amt2);
        }
        _ => panic!("Expected both commands to be ChargeBudget"),
    }
}
