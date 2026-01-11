//! CLI recovery demo simulation
//!
//! Simulates the full CLI recovery demo flow using the simulator.

use crate::error::{TerminalError, TerminalResult};
use aura_agent::core::AuthorityContext;
use aura_agent::handlers::RecoveryServiceApi;
use aura_agent::SharedTransport;
use aura_agent::{AgentConfig, AuraEffectSystem};
use aura_core::effects::PhysicalTimeEffects;
use aura_simulator::handlers::scenario::SimulationScenarioHandler;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::handlers::HandlerContext;

/// A step in the simulation
pub struct SimStep {
    pub phase: String,
    pub action: String,
    pub details: Option<String>,
}

/// Result of CLI recovery demo simulation
pub struct CliRecoverySimResult {
    pub outcome: String,
    pub duration_ms: u64,
    pub steps: Vec<SimStep>,
    pub validation_results: HashMap<String, bool>,
}

/// Simulate the CLI recovery demo flow
pub async fn simulate_cli_recovery_demo(
    seed: u64,
    ctx: &HandlerContext<'_>,
) -> TerminalResult<CliRecoverySimResult> {
    let handler = SimulationScenarioHandler::new(seed);
    let mut steps = Vec::new();
    let start = ctx
        .effects()
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to read time: {e}")))?;

    // Run guardian setup choreography via execute_as runtime wiring
    run_guardian_setup_choreography(&mut steps).await?;

    // Phase 1: Alice & Carol pre-setup (log only)
    steps.push(SimStep {
        phase: "alice_carol_setup".into(),
        action: "create_accounts".into(),
        details: Some("Alice and Carol accounts created".into()),
    });

    // Phase 2: Requests and acceptance to become guardians
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "create_account".into(),
        details: Some("Bob account created".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_request_alice".into(),
        details: Some("Bob requests Alice to be guardian".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_accept_alice".into(),
        details: Some("Alice accepts guardian responsibility".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_request_carol".into(),
        details: Some("Bob requests Carol to be guardian".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_accept_carol".into(),
        details: Some("Carol accepts guardian responsibility".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_authority_configuration".into(),
        details: Some("Alice+Carol become guardian authority for Bob".into()),
    });

    // Phase 3-4: group chat setup and initial messaging
    let group_id = handler.create_chat_group(
        "Alice, Bob & Carol",
        "alice",
        vec!["bob".into(), "carol".into()],
    )?;
    steps.push(SimStep {
        phase: "group_chat_setup".into(),
        action: "create_group".into(),
        details: Some(format!("Group ID: {}", group_id)),
    });

    let messages = vec![
        ("group_messaging", "alice", "Welcome to our group, Bob!"),
        ("group_messaging", "bob", "Thanks Alice! Great to be here."),
        (
            "group_messaging",
            "carol",
            "Hey everyone! This chat system is awesome.",
        ),
        (
            "group_messaging",
            "alice",
            "Bob, you should backup your account soon",
        ),
        (
            "group_messaging",
            "bob",
            "I'll do that right after this demo!",
        ),
    ];
    for (phase, sender, message) in &messages {
        handler.send_chat_message(&group_id, sender, message)?;
        steps.push(SimStep {
            phase: (*phase).into(),
            action: "send_message".into(),
            details: Some(format!("{}: {}", sender, message)),
        });
    }

    // Phase 5: data loss
    handler.simulate_data_loss("bob", "complete_device_loss", true)?;
    steps.push(SimStep {
        phase: "bob_account_loss".into(),
        action: "simulate_data_loss".into(),
        details: Some("Bob loses all account data".into()),
    });

    // Phase 6-7: recovery
    handler.initiate_guardian_recovery("bob", vec!["alice".into(), "carol".into()], 2)?;
    steps.push(SimStep {
        phase: "recovery_initiation".into(),
        action: "initiate_guardian_recovery".into(),
        details: Some("Alice and Carol assist recovery".into()),
    });

    let recovery_success = handler.verify_recovery_success(
        "bob",
        vec![
            "keys_restored".into(),
            "account_accessible".into(),
            "message_history_restored".into(),
        ],
    )?;
    steps.push(SimStep {
        phase: "account_restoration".into(),
        action: "verify_recovery".into(),
        details: Some(if recovery_success { "ok" } else { "fail" }.into()),
    });

    // Phase 8: post recovery messaging
    let post_recovery_messages = vec![
        (
            "post_recovery_messaging",
            "bob",
            "I'm back! Thanks Alice and Carol for helping me recover.",
        ),
        (
            "post_recovery_messaging",
            "alice",
            "Welcome back Bob! Guardian recovery really works!",
        ),
        (
            "post_recovery_messaging",
            "carol",
            "Amazing! You can see all our previous messages too.",
        ),
    ];
    for (phase, sender, message) in &post_recovery_messages {
        handler.send_chat_message(&group_id, sender, message)?;
        steps.push(SimStep {
            phase: (*phase).into(),
            action: "send_message".into(),
            details: Some(format!("{}: {}", sender, message)),
        });
    }

    // Validations
    let mut validation_results = HashMap::new();
    let message_continuity = handler.validate_message_history("bob", 8, true)?;
    validation_results.insert("message_continuity_maintained".into(), message_continuity);

    let bob_can_send = handler
        .send_chat_message(&group_id, "bob", "Test message after recovery")
        .is_ok();
    validation_results.insert("bob_can_send_messages".into(), bob_can_send);

    let group_functional = handler.get_chat_stats().is_ok();
    validation_results.insert("group_functionality_restored".into(), group_functional);

    let full_history_access = handler.validate_message_history("bob", 5, true)?;
    validation_results.insert("bob_can_see_full_history".into(), full_history_access);

    let outcome = if validation_results.values().all(|v| *v) && recovery_success {
        "RecoveryDemoSuccess"
    } else {
        "Failure"
    }
    .to_string();

    let end = ctx
        .effects()
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to read time: {e}")))?;
    let duration_ms = end.ts_ms.saturating_sub(start.ts_ms);

    Ok(CliRecoverySimResult {
        outcome,
        duration_ms,
        steps,
        validation_results,
    })
}

/// Run the guardian setup choreography
async fn run_guardian_setup_choreography(steps: &mut Vec<SimStep>) -> TerminalResult<()> {
    let shared_transport = SharedTransport::new();
    let config = AgentConfig::default();

    let initiator_id = crate::ids::authority_id("scenario:guardian-setup:initiator");
    let account_id = crate::ids::authority_id("scenario:guardian-setup:account");
    let guardians = vec![
        crate::ids::authority_id("guardian:alice"),
        crate::ids::authority_id("guardian:carol"),
        crate::ids::authority_id("guardian:dave"),
    ];

    let initiator_effects = Arc::new(
        AuraEffectSystem::simulation_with_shared_transport_for_authority(
            &config,
            42,
            initiator_id,
            shared_transport.clone(),
        )?,
    );
    let initiator_service =
        RecoveryServiceApi::new(initiator_effects, AuthorityContext::new(initiator_id))?;

    let mut guardian_services = Vec::with_capacity(guardians.len());
    for (index, guardian_id) in guardians.iter().copied().enumerate() {
        let guardian_effects = Arc::new(
            AuraEffectSystem::simulation_with_shared_transport_for_authority(
                &config,
                100 + index as u64,
                guardian_id,
                shared_transport.clone(),
            )?,
        );
        let service =
            RecoveryServiceApi::new(guardian_effects, AuthorityContext::new(guardian_id))?;
        guardian_services.push(service);
    }

    let setup_id = format!("setup_{}_{}", account_id, Uuid::new_v4());
    let invitation = aura_recovery::guardian_setup::GuardianInvitation {
        setup_id: setup_id.clone(),
        account_id,
        target_guardians: guardians.clone(),
        threshold: 2,
        timestamp: aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        }),
    };

    let mut guardian_tasks = Vec::with_capacity(guardian_services.len());
    for service in guardian_services.into_iter() {
        let invite = invitation.clone();
        guardian_tasks.push(tokio::spawn(async move {
            service.execute_guardian_setup_guardian(invite, true).await
        }));
    }

    let initiator_task = tokio::spawn(async move {
        initiator_service
            .execute_guardian_setup_initiator_with_id(&setup_id, account_id, guardians, 2)
            .await
    });

    let response = initiator_task
        .await
        .map_err(|e| TerminalError::Operation(format!("Guardian setup join failed: {e}")))?
        .map_err(|e| TerminalError::Operation(format!("Guardian setup failed: {e}")))?;

    for task in guardian_tasks {
        task.await
            .map_err(|e| TerminalError::Operation(format!("Guardian setup join failed: {e}")))?
            .map_err(|e| TerminalError::Operation(format!("Guardian setup failed: {e}")))?;
    }

    if !response.success {
        return Err(TerminalError::Operation(
            "Guardian setup failed: threshold not met".to_string(),
        ));
    }

    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_setup_choreography".into(),
        details: Some("Guardian setup choreography executed".into()),
    });

    Ok(())
}
