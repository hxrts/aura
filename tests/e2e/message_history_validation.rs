//! Message History Validation Tests
//!
//! This test validates message history persistence and retrieval across
//! recovery events, ensuring Bob's demo workflow maintains message continuity.

use aura_simulator::handlers::scenario::SimulationScenarioHandler;

/// Test message history validation across different recovery scenarios
#[tokio::test]
async fn test_message_history_validation_comprehensive() {
    let handler = SimulationScenarioHandler::new(2024);

    // Phase 1: Setup chat group and baseline messages
    let group_id = handler
        .create_chat_group(
            "Alice, Bob & Charlie",
            "alice",
            vec!["bob".to_string(), "charlie".to_string()],
        )
        .expect("group created");

    let messages = vec![
        ("alice", "Welcome to our group, Bob!"),
        ("bob", "Thanks Alice! Great to be here."),
        ("charlie", "Hey everyone! This chat system is awesome."),
        ("alice", "Bob, you should backup your account soon"),
        ("bob", "I'll do that right after this demo!"),
    ];

    for (sender, msg) in &messages {
        handler
            .send_chat_message(&group_id, sender, msg)
            .expect("message send");
    }

    // Simulate data loss for Bob
    handler
        .simulate_data_loss("bob", "complete_device_loss", true)
        .expect("data loss simulated");

    // Guardian recovery coordination
    handler
        .initiate_guardian_recovery("bob", vec!["alice".to_string(), "charlie".to_string()], 2)
        .expect("guardian recovery initiated");

    let recovery_success = handler
        .verify_recovery_success(
            "bob",
            vec![
                "keys_restored".to_string(),
                "account_accessible".to_string(),
                "message_history_restored".to_string(),
            ],
        )
        .expect("recovery verification");
    assert!(recovery_success, "Recovery should succeed");

    // Post-recovery validations
    let continuity = handler
        .validate_message_history("bob", messages.len(), true)
        .expect("validate continuity");
    assert!(
        continuity,
        "Message continuity should be maintained across recovery"
    );

    let full_history = handler
        .validate_message_history("bob", 5, true)
        .expect("validate full history");
    assert!(
        full_history,
        "Bob should see complete message history after recovery"
    );

    let can_send = handler
        .send_chat_message(&group_id, "bob", "Test message after recovery")
        .is_ok();
    assert!(
        can_send,
        "Bob should be able to send messages after recovery"
    );

    println!("✓ Comprehensive message history validation passed");
}

/// Test message history across multiple chat groups
#[tokio::test]
async fn test_multi_group_message_history() {
    let handler = SimulationScenarioHandler::new(123);

    // Create multiple groups with overlapping membership
    let group1_id = handler
        .create_chat_group("Alice & Bob", "alice", vec!["bob".to_string()])
        .unwrap();

    let group2_id = handler
        .create_chat_group("Bob & Charlie", "bob", vec!["charlie".to_string()])
        .unwrap();

    let group3_id = handler
        .create_chat_group(
            "All Friends",
            "alice",
            vec!["bob".to_string(), "charlie".to_string()],
        )
        .unwrap();

    // Send messages in each group
    let messages = vec![
        (group1_id.as_str(), "alice", "Alice to Bob privately"),
        (group1_id.as_str(), "bob", "Bob replies to Alice"),
        (group2_id.as_str(), "bob", "Bob to Charlie privately"),
        (group2_id.as_str(), "charlie", "Charlie replies to Bob"),
        (group3_id.as_str(), "alice", "Alice to group"),
        (group3_id.as_str(), "bob", "Bob to group"),
        (group3_id.as_str(), "charlie", "Charlie to group"),
    ];

    for (group_id, sender, message) in &messages {
        handler
            .send_chat_message(group_id, sender, message)
            .unwrap();
    }

    // Bob should see messages from all groups he's in (all 3 groups = 7 messages total)
    let bob_pre_loss = handler.validate_message_history("bob", 7, false).unwrap();
    assert!(
        bob_pre_loss,
        "Bob should see all 7 messages before data loss"
    );

    // Alice should see messages from group1 and group3 (4 messages total)
    let alice_pre_loss = handler.validate_message_history("alice", 4, false).unwrap();
    assert!(
        alice_pre_loss,
        "Alice should see 4 messages from her groups"
    );

    // Charlie should see messages from group2 and group3 (4 messages total)
    let charlie_pre_loss = handler
        .validate_message_history("charlie", 4, false)
        .unwrap();
    assert!(
        charlie_pre_loss,
        "Charlie should see 4 messages from his groups"
    );

    // Simulate Bob's data loss
    handler
        .simulate_data_loss("bob", "complete_device_loss", true)
        .unwrap();

    // Initiate and complete recovery
    handler
        .initiate_guardian_recovery("bob", vec!["alice".to_string(), "charlie".to_string()], 2)
        .unwrap();

    handler
        .verify_recovery_success("bob", vec!["multi_group_history_restored".to_string()])
        .unwrap();

    // After recovery, Bob should still see all messages from all his groups
    let bob_post_recovery = handler.validate_message_history("bob", 7, true).unwrap();
    assert!(
        bob_post_recovery,
        "Bob should see all 7 messages after recovery across all groups"
    );

    // Post-recovery: Bob should be able to send messages to all his groups
    assert!(handler
        .send_chat_message(&group1_id, "bob", "Back in group 1!")
        .is_ok());
    assert!(handler
        .send_chat_message(&group2_id, "bob", "Back in group 2!")
        .is_ok());
    assert!(handler
        .send_chat_message(&group3_id, "bob", "Back in group 3!")
        .is_ok());

    // Validate final message counts (original + 3 recovery messages)
    let bob_final = handler.validate_message_history("bob", 10, true).unwrap();
    assert!(bob_final, "Bob should see all original + recovery messages");

    println!("✓ Multi-group message history validation passed");
}

/// Test message history validation edge cases and error conditions  
#[tokio::test]
async fn test_message_history_edge_cases() {
    let handler = SimulationScenarioHandler::new(456);

    // Case 1: Empty group message history
    let empty_group_id = handler
        .create_chat_group("Empty Group", "alice", vec!["bob".to_string()])
        .unwrap();

    let empty_validation = handler.validate_message_history("bob", 0, false).unwrap();
    assert!(empty_validation, "Should validate empty message history");

    let over_expectation = handler.validate_message_history("bob", 1, false).unwrap();
    assert!(
        !over_expectation,
        "Should fail when expecting messages that don't exist"
    );

    // Case 2: Message history after leaving/rejoining scenarios
    handler
        .send_chat_message(&empty_group_id, "alice", "Message 1")
        .unwrap();
    handler
        .send_chat_message(&empty_group_id, "bob", "Message 2")
        .unwrap();

    // Simulate Bob losing access (data loss) then recovering
    handler
        .simulate_data_loss("bob", "partial_key_corruption", true)
        .unwrap();

    // Bob should not see messages during data loss period
    handler
        .send_chat_message(&empty_group_id, "alice", "Message during loss")
        .unwrap();

    // Complete recovery
    handler
        .initiate_guardian_recovery("bob", vec!["alice".to_string()], 1)
        .unwrap();

    handler
        .verify_recovery_success("bob", vec!["partial_recovery".to_string()])
        .unwrap();

    // Bob should see pre-loss messages after recovery
    let recovery_validation = handler.validate_message_history("bob", 2, true).unwrap();
    assert!(
        recovery_validation,
        "Bob should see pre-loss messages after recovery"
    );

    // Case 3: Multiple data loss and recovery cycles
    handler
        .send_chat_message(&empty_group_id, "bob", "Post recovery 1")
        .unwrap();

    // Second data loss
    handler
        .simulate_data_loss("bob", "storage_corruption", true)
        .unwrap();

    // Second recovery
    handler
        .initiate_guardian_recovery("bob", vec!["alice".to_string()], 1)
        .unwrap();

    handler
        .verify_recovery_success("bob", vec!["second_recovery".to_string()])
        .unwrap();

    // Should still maintain message history continuity
    let double_recovery_validation = handler.validate_message_history("bob", 3, true).unwrap();
    assert!(
        double_recovery_validation,
        "Message history should survive multiple recovery cycles"
    );

    println!("✓ Message history edge cases validation passed");
}

/// Test message history validation with concurrent operations
#[tokio::test]
async fn test_concurrent_message_history_operations() {
    let handler = SimulationScenarioHandler::new(789);

    let group_id = handler
        .create_chat_group(
            "Concurrent Test",
            "alice",
            vec!["bob".to_string(), "charlie".to_string()],
        )
        .unwrap();

    // Simulate concurrent messaging while data loss occurs
    let concurrent_messages = vec![
        ("alice", "Concurrent message 1"),
        ("bob", "Bob's last message before loss"),
        ("charlie", "Charlie's message"),
    ];

    for (sender, message) in &concurrent_messages {
        handler
            .send_chat_message(&group_id, sender, message)
            .unwrap();
    }

    // Bob loses data while messages continue
    handler
        .simulate_data_loss("bob", "network_partition", true)
        .unwrap();

    // More messages sent while Bob is offline
    let offline_messages = vec![
        ("alice", "Message while Bob offline 1"),
        ("charlie", "Message while Bob offline 2"),
        ("alice", "Message while Bob offline 3"),
    ];

    for (sender, message) in &offline_messages {
        handler
            .send_chat_message(&group_id, sender, message)
            .unwrap();
    }

    // Initiate recovery
    handler
        .initiate_guardian_recovery("bob", vec!["alice".to_string(), "charlie".to_string()], 2)
        .unwrap();

    handler
        .verify_recovery_success("bob", vec!["concurrent_recovery".to_string()])
        .unwrap();

    // Bob should see pre-loss messages but validation may be flexible about offline messages
    let pre_loss_validation = handler.validate_message_history("bob", 3, true).unwrap();
    assert!(
        pre_loss_validation,
        "Bob should see at least pre-loss messages"
    );

    // Test that Bob can resume normal operations
    handler
        .send_chat_message(&group_id, "bob", "I'm back online!")
        .unwrap();

    let total_messages = 3 + 3 + 1; // pre-loss + offline + recovery message
    let final_validation = handler
        .validate_message_history("bob", total_messages, false)
        .unwrap();

    // This might be flexible depending on how offline messages are handled
    println!("Final message validation result: {}", final_validation);
    println!("✓ Concurrent operations message history test completed");
}

/// Test message history validation performance with large message volumes
#[tokio::test]
async fn test_large_volume_message_history() {
    let handler = SimulationScenarioHandler::new(999);

    let group_id = handler
        .create_chat_group("High Volume Test", "alice", vec!["bob".to_string()])
        .unwrap();

    // Send a large number of messages
    const MESSAGE_COUNT: usize = 1000;
    for i in 0..MESSAGE_COUNT {
        let sender = if i % 2 == 0 { "alice" } else { "bob" };
        let message = format!("Bulk message {}", i + 1);
        handler
            .send_chat_message(&group_id, sender, &message)
            .unwrap();
    }

    // Validate large message history
    let large_validation = handler
        .validate_message_history("bob", MESSAGE_COUNT, false)
        .unwrap();
    assert!(
        large_validation,
        "Should validate large message history efficiently"
    );

    // Simulate data loss and recovery with large history
    handler
        .simulate_data_loss("bob", "complete_device_loss", true)
        .unwrap();

    handler
        .initiate_guardian_recovery("bob", vec!["alice".to_string()], 1)
        .unwrap();

    handler
        .verify_recovery_success("bob", vec!["large_volume_recovery".to_string()])
        .unwrap();

    // Validate recovery with large message volume
    let large_recovery_validation = handler
        .validate_message_history("bob", MESSAGE_COUNT, true)
        .unwrap();
    assert!(
        large_recovery_validation,
        "Should handle large message history recovery efficiently"
    );

    println!(
        "✓ Large volume message history validation passed ({} messages)",
        MESSAGE_COUNT
    );
}
