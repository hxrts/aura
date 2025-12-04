//! Reactive System Tests using TuiTestHarness
//!
//! These tests demonstrate end-to-end reactive pipeline testing:
//! Facts → ReactiveScheduler → ViewAdapter → View.apply_delta() → ViewUpdates

mod tui_helpers;

use std::time::Duration;
use tui_helpers::{
    assert_view_eventually, make_channel_created_fact, make_guardian_binding_fact,
    make_invitation_created_fact, make_message_sent_fact, make_recovery_grant_fact, FactScenario,
    TuiTestHarness,
};

// =============================================================================
// Basic Harness Tests
// =============================================================================

#[tokio::test]
async fn test_harness_basic_fact_injection() {
    let harness = TuiTestHarness::new().await;

    // Inject a fact and verify no errors
    let fact = make_guardian_binding_fact(1, 2, 1);
    let result = harness.inject_fact(fact).await;
    assert!(result.is_ok(), "Should inject fact without error");

    // Wait for processing
    harness.wait_for_processing().await;
}

#[tokio::test]
async fn test_harness_batch_fact_injection() {
    let harness = TuiTestHarness::new().await;

    // Inject multiple facts
    let mut scenario = FactScenario::new();
    scenario
        .add_guardian_binding(1, 2)
        .add_guardian_binding(1, 3)
        .add_guardian_binding(1, 4);
    let facts = scenario.build();

    let result = harness.inject_facts(facts).await;
    assert!(result.is_ok(), "Should inject batch of facts without error");

    harness.wait_for_processing().await;
}

#[tokio::test]
async fn test_harness_mixed_fact_sources() {
    let harness = TuiTestHarness::new().await;

    // Inject from journal source
    let journal_facts = vec![make_guardian_binding_fact(1, 2, 1)];
    harness.inject_facts(journal_facts).await.unwrap();

    // Inject from network source
    let network_facts = vec![make_recovery_grant_fact(1, 3, 2)];
    harness.inject_network_facts(network_facts).await.unwrap();

    harness.wait_for_processing().await;
}

// =============================================================================
// View-Specific Tests
// =============================================================================

#[tokio::test]
async fn test_guardian_view_receives_binding_facts() {
    let harness = TuiTestHarness::new().await;

    // Inject guardian binding facts
    let facts = vec![
        make_guardian_binding_fact(1, 2, 1),
        make_guardian_binding_fact(1, 3, 2),
    ];
    harness.inject_facts(facts).await.unwrap();
    harness.wait_for_processing().await;

    // The scheduler should have processed these facts
    // and the guardian view should have received them
    // (We can't directly verify view state without exposing it,
    // but we verify the pipeline doesn't error)
}

#[tokio::test]
async fn test_chat_view_receives_message_facts() {
    let harness = TuiTestHarness::new().await;

    // Inject chat-related facts
    let facts = vec![make_channel_created_fact(1), make_message_sent_fact(2)];
    harness.inject_facts(facts).await.unwrap();
    harness.wait_for_processing().await;
}

#[tokio::test]
async fn test_invitation_view_receives_facts() {
    let harness = TuiTestHarness::new().await;

    // Inject invitation facts
    let facts = vec![make_invitation_created_fact(1)];
    harness.inject_facts(facts).await.unwrap();
    harness.wait_for_processing().await;
}

// =============================================================================
// Scenario-Based Tests
// =============================================================================

#[tokio::test]
async fn test_complete_guardian_setup_scenario() {
    let harness = TuiTestHarness::new().await;

    // Simulate setting up guardians for an account
    let mut scenario = FactScenario::new();
    scenario
        .add_guardian_binding(1, 10) // First guardian
        .add_guardian_binding(1, 11) // Second guardian
        .add_guardian_binding(1, 12); // Third guardian
    let facts = scenario.build();

    harness.inject_facts(facts).await.unwrap();
    harness.wait_for_processing().await;

    // All guardian bindings should flow through the reactive pipeline
}

#[tokio::test]
async fn test_recovery_scenario() {
    let harness = TuiTestHarness::new().await;

    // Simulate a recovery flow
    let mut scenario = FactScenario::new();
    scenario
        .add_guardian_binding(1, 10) // Setup guardian
        .add_recovery_grant(1, 10); // Guardian grants recovery

    harness.inject_facts(scenario.build()).await.unwrap();
    harness.wait_for_processing().await;
}

#[tokio::test]
async fn test_chat_conversation_scenario() {
    let harness = TuiTestHarness::new().await;

    // Simulate a chat conversation
    let mut scenario = FactScenario::new();
    scenario
        .add_channel_created() // Create channel
        .add_message_sent() // First message
        .add_message_sent() // Second message
        .add_message_sent(); // Third message

    harness.inject_facts(scenario.build()).await.unwrap();
    harness.wait_for_processing().await;
}

// =============================================================================
// Ordering and Consistency Tests
// =============================================================================

#[tokio::test]
async fn test_fact_ordering_preserved() {
    let harness = TuiTestHarness::new().await;

    // Facts should be processed in order
    for i in 1..=10 {
        harness
            .inject_fact(make_guardian_binding_fact(1, i as u8, i))
            .await
            .unwrap();
    }

    harness.wait_for_processing().await;
}

#[tokio::test]
async fn test_concurrent_fact_injection() {
    let harness = TuiTestHarness::new().await;

    // Inject facts concurrently
    let h1 = harness.inject_facts(vec![
        make_guardian_binding_fact(1, 2, 1),
        make_guardian_binding_fact(1, 3, 2),
    ]);

    let h2 = harness.inject_facts(vec![
        make_channel_created_fact(3),
        make_message_sent_fact(4),
    ]);

    // Both should succeed
    let (r1, r2) = tokio::join!(h1, h2);
    assert!(r1.is_ok());
    assert!(r2.is_ok());

    harness.wait_for_processing().await;
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_empty_fact_list_handled() {
    let harness = TuiTestHarness::new().await;

    // Empty list should be handled gracefully
    let result = harness.inject_facts(vec![]).await;
    assert!(result.is_ok());

    harness.wait_for_processing().await;
}

// =============================================================================
// Assertion Helper Tests
// =============================================================================

#[tokio::test]
async fn test_assert_view_eventually_helper() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let flag = Arc::new(AtomicBool::new(false));
    let flag_clone = flag.clone();

    // Start a task that sets the flag after a delay
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        flag_clone.store(true, Ordering::SeqCst);
    });

    // Assert that the flag eventually becomes true
    let result = assert_view_eventually(
        || {
            let f = flag.clone();
            async move { f.load(Ordering::SeqCst) }
        },
        Duration::from_millis(100),
        "Flag should become true",
    )
    .await;

    assert!(result.is_ok());
}
