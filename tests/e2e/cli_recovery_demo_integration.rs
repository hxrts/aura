//! CLI Recovery Demo Integration Test
//!
//! This test validates the complete Bob's recovery journey scenario,
//! including multi-actor chat, data loss simulation, and message history validation.

use aura_simulator::handlers::scenario::{SimulationScenarioHandler, InjectionAction, ScenarioDefinition, TriggerCondition};
use aura_core::effects::TestingEffects;
use std::time::Duration;
use std::collections::HashMap;

/// Test the complete CLI recovery demo scenario
/// 
/// This test mirrors the workflow defined in scenarios/integration/cli_recovery_demo.toml:
/// 1. Alice & Carol setup
/// 2. Bob onboarding with guardian setup
/// 3. Group chat establishment and messaging
/// 4. Bob's account data loss simulation  
/// 5. Guardian-assisted recovery with message history restoration
#[tokio::test]
async fn test_complete_cli_recovery_demo() {
    let handler = SimulationScenarioHandler::new(2024); // Deterministic demo seed

    // Phase 1: Setup chat group (Alice creates, adds Bob & Carol)
    let group_id = handler
        .create_chat_group(
            "Alice, Bob & Carol",
            "alice",
            vec!["bob".to_string(), "carol".to_string()],
        )
        .expect("Should create chat group successfully");

    // Verify group creation
    let chat_stats = handler.get_chat_stats().unwrap();
    assert_eq!(chat_stats.get("chat_groups").unwrap(), "1");

    // Phase 2: Pre-data-loss messaging (establish message history)
    let messages = vec![
        ("alice", "Welcome to our group, Bob!"),
        ("bob", "Thanks Alice! Great to be here."),
        ("carol", "Hey everyone! This chat system is awesome."),
        ("alice", "Bob, you should backup your account soon"),
        ("bob", "I'll do that right after this demo!"),
    ];

    for (sender, message) in &messages {
        handler
            .send_chat_message(&group_id, sender, message)
            .expect("Should send message successfully");
    }

    // Verify message history
    let chat_stats = handler.get_chat_stats().unwrap();
    assert_eq!(chat_stats.get("total_messages").unwrap(), "5");

    // Validate Bob can see all messages before data loss
    let pre_loss_validation = handler
        .validate_message_history("bob", 5, false)
        .expect("Should validate message history");
    assert!(pre_loss_validation, "Bob should see all 5 messages before data loss");

    // Phase 3: Catastrophic failure simulation (Bob loses everything)
    handler
        .simulate_data_loss("bob", "complete_device_loss", true)
        .expect("Should simulate data loss successfully");

    // Verify data loss is recorded
    let chat_stats = handler.get_chat_stats().unwrap();
    assert_eq!(chat_stats.get("participants_with_data_loss").unwrap(), "1");

    let bob_loss_count = handler
        .inspect_state("data_loss", "bob")
        .await
        .expect("Should inspect Bob's data loss")
        .downcast::<usize>()
        .unwrap();
    assert_eq!(*bob_loss_count, 5, "Bob should have lost access to 5 messages");

    // Phase 4: Guardian recovery initiation
    handler
        .initiate_guardian_recovery(
            "bob",
            vec!["alice".to_string(), "carol".to_string()],
            2, // 2-of-3 threshold
        )
        .expect("Should initiate guardian recovery");

    // Verify recovery process is active
    let chat_stats = handler.get_chat_stats().unwrap();
    assert_eq!(chat_stats.get("active_recoveries").unwrap(), "1");

    // Phase 5: Recovery completion and validation
    let recovery_success = handler
        .verify_recovery_success(
            "bob",
            vec![
                "keys_restored".to_string(),
                "account_accessible".to_string(),
                "message_history_restored".to_string(),
            ],
        )
        .expect("Should verify recovery success");
    assert!(recovery_success, "Recovery should complete successfully");

    // Phase 6: Post-recovery message history validation
    // Bob should be able to see all pre-loss messages plus any new ones
    let post_recovery_validation = handler
        .validate_message_history("bob", 5, true) // Include pre-recovery messages
        .expect("Should validate post-recovery message history");
    assert!(
        post_recovery_validation,
        "Bob should see all original messages after recovery"
    );

    // Phase 7: Post-recovery messaging (Bob can participate again)
    let post_recovery_messages = vec![
        ("bob", "I'm back! Thanks Alice and Carol for helping me recover."),
        ("alice", "Welcome back Bob! Guardian recovery really works!"),
        ("carol", "Amazing! You can see all our previous messages too."),
    ];

    for (sender, message) in &post_recovery_messages {
        handler
            .send_chat_message(&group_id, sender, message)
            .expect("Should send post-recovery message successfully");
    }

    // Final validation: Complete message continuity
    let final_stats = handler.get_chat_stats().unwrap();
    assert_eq!(final_stats.get("total_messages").unwrap(), "8"); // 5 original + 3 post-recovery

    let final_validation = handler
        .validate_message_history("bob", 8, true)
        .expect("Should validate final message history");
    assert!(
        final_validation,
        "Bob should see complete message history including pre-recovery and post-recovery messages"
    );

    // Verify no active data loss or recovery processes remain
    assert_eq!(final_stats.get("participants_with_data_loss").unwrap(), "0");
    assert_eq!(final_stats.get("active_recoveries").unwrap(), "0");

    let recovery_complete = handler
        .inspect_state("recovery", "bob")
        .await
        .expect("Should inspect Bob's recovery status")
        .downcast::<bool>()
        .unwrap();
    assert!(*recovery_complete, "Bob's recovery should be marked as complete");
}

/// Test message history validation edge cases
#[tokio::test] 
async fn test_message_history_validation_edge_cases() {
    let handler = SimulationScenarioHandler::new(123);

    let group_id = handler
        .create_chat_group("Test Group", "alice", vec!["bob".to_string()])
        .unwrap();

    // Case 1: No messages - validation should handle gracefully
    let empty_validation = handler
        .validate_message_history("bob", 0, false)
        .unwrap();
    assert!(empty_validation, "Should validate with no messages");

    // Case 2: Messages after data loss but before recovery
    handler.send_chat_message(&group_id, "alice", "Message 1").unwrap();
    handler.simulate_data_loss("bob", "complete_device_loss", true).unwrap();
    handler.send_chat_message(&group_id, "alice", "Message 2").unwrap();

    let during_loss_validation = handler
        .validate_message_history("bob", 1, true)
        .unwrap();
    assert!(during_loss_validation, "Should validate with pre-loss message count");

    // Case 3: Over-expectations (asking for more messages than exist)
    let over_expectation = handler
        .validate_message_history("bob", 100, false)
        .unwrap();
    assert!(!over_expectation, "Should fail validation when expecting too many messages");

    // Case 4: Participant with no data loss
    let no_loss_validation = handler
        .validate_message_history("alice", 2, true)
        .unwrap();
    assert!(no_loss_validation, "Alice (no data loss) should see all messages");
}

/// Test multi-actor chat group dynamics
#[tokio::test]
async fn test_multi_actor_chat_dynamics() {
    let handler = SimulationScenarioHandler::new(456);

    // Create multiple groups to test complex scenarios
    let group1_id = handler
        .create_chat_group("Group 1", "alice", vec!["bob".to_string()])
        .unwrap();
    
    let group2_id = handler
        .create_chat_group("Group 2", "bob", vec!["carol".to_string()])
        .unwrap();

    let group3_id = handler
        .create_chat_group("All Friends", "alice", vec!["bob".to_string(), "carol".to_string()])
        .unwrap();

    // Send messages in different groups
    handler.send_chat_message(&group1_id, "alice", "Alice to Bob").unwrap();
    handler.send_chat_message(&group2_id, "bob", "Bob to Carol").unwrap();
    handler.send_chat_message(&group3_id, "carol", "Carol to all").unwrap();

    // Verify stats
    let stats = handler.get_chat_stats().unwrap();
    assert_eq!(stats.get("chat_groups").unwrap(), "3");
    assert_eq!(stats.get("total_messages").unwrap(), "3");

    // Test data loss affects all groups Bob is in
    handler.simulate_data_loss("bob", "complete_device_loss", true).unwrap();

    let bob_loss_count = handler
        .inspect_state("data_loss", "bob")
        .await
        .unwrap()
        .downcast::<usize>()
        .unwrap();
    
    // Bob was in group1, group2, and group3, so should have lost access to messages in all
    assert_eq!(*bob_loss_count, 3, "Bob should lose access to messages from all groups he's in");

    // Recovery should restore access to all groups
    handler.initiate_guardian_recovery("bob", vec!["alice".to_string(), "carol".to_string()], 2).unwrap();
    handler.verify_recovery_success("bob", vec!["all_groups_restored".to_string()]).unwrap();

    let post_recovery_validation = handler.validate_message_history("bob", 3, true).unwrap();
    assert!(post_recovery_validation, "Bob should regain access to all group messages after recovery");
}

/// Test guardian recovery failure scenarios  
#[tokio::test]
async fn test_guardian_recovery_failure_scenarios() {
    let handler = SimulationScenarioHandler::new(789);

    // Test: Insufficient guardians
    let insufficient_result = handler.initiate_guardian_recovery(
        "bob", 
        vec!["alice".to_string()], // Only 1 guardian
        2 // But need 2
    );
    assert!(insufficient_result.is_err(), "Should fail with insufficient guardians");

    // Test: Recovery verification for non-existent process
    let no_process_result = handler.verify_recovery_success(
        "nonexistent", 
        vec!["fake_validation".to_string()]
    );
    assert!(no_process_result.is_err(), "Should fail when no recovery process exists");

    // Test: Valid recovery setup and completion
    let valid_result = handler.initiate_guardian_recovery(
        "bob",
        vec!["alice".to_string(), "carol".to_string(), "dave".to_string()],
        2 // 2-of-3
    );
    assert!(valid_result.is_ok(), "Should succeed with sufficient guardians");

    let verification_result = handler.verify_recovery_success(
        "bob",
        vec!["threshold_signature_validated".to_string(), "keys_reconstructed".to_string()]
    );
    assert!(verification_result.is_ok(), "Should succeed in verifying recovery");
    assert!(verification_result.unwrap(), "Recovery verification should return true");
}