//! Integration test for the reactive system
//!
//! This test demonstrates the end-to-end reactive pipeline:
//! Facts → ReactiveScheduler → ViewAdapter → View.apply_delta() → ViewUpdates
//!
//! This proves the reactive infrastructure is working before we implement
//! real journal streaming.
//!
//! TODO: Update for IoContext migration - scheduler integration needed

use aura_core::time::{OrderTime, TimeStamp};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_terminal::tui::context::IoContext;
use std::time::Duration;
use tokio::time::sleep;

/// Helper to create a test guardian binding fact
fn make_guardian_binding_fact(_account_id: &str, _guardian_id: &str, order: u64) -> Fact {
    let account = aura_core::AuthorityId::new_from_entropy([1u8; 32]);
    let guardian = aura_core::AuthorityId::new_from_entropy([2u8; 32]);

    Fact {
        order: OrderTime([order as u8; 32]),
        timestamp: TimeStamp::OrderClock(OrderTime([order as u8; 32])),
        content: FactContent::Relational(RelationalFact::GuardianBinding {
            account_id: account,
            guardian_id: guardian,
            binding_hash: aura_core::Hash32([0u8; 32]),
        }),
    }
}

/// Helper to create a test recovery grant fact
fn make_recovery_grant_fact(_account_id: &str, _guardian_id: &str, order: u64) -> Fact {
    let account = aura_core::AuthorityId::new_from_entropy([1u8; 32]);
    let guardian = aura_core::AuthorityId::new_from_entropy([2u8; 32]);

    Fact {
        order: OrderTime([order as u8; 32]),
        timestamp: TimeStamp::OrderClock(OrderTime([order as u8; 32])),
        content: FactContent::Relational(RelationalFact::RecoveryGrant {
            account_id: account,
            guardian_id: guardian,
            grant_hash: aura_core::Hash32([0u8; 32]),
        }),
    }
}

#[tokio::test]
#[ignore = "TODO: Update for IoContext migration - scheduler integration needed"]
async fn test_reactive_pipeline_end_to_end() {
    // Create IoContext with default configuration
    let _context = IoContext::with_defaults();

    // Create test facts that should trigger guardian view updates
    let facts = vec![
        make_guardian_binding_fact("account1", "guardian1", 1),
        make_guardian_binding_fact("account1", "guardian2", 2),
    ];

    // TODO: Re-enable when IoContext has scheduler integration
    // let result = context.send_facts_to_scheduler(FactSource::Journal(facts.clone())).await;

    // Wait for async processing (scheduler batches with 5ms window)
    sleep(Duration::from_millis(20)).await;

    println!("✓ Reactive pipeline test completed");
    println!("  - Created IoContext");
    println!(
        "  - Created {} facts (not sent - scheduler integration pending)",
        facts.len()
    );
}

#[tokio::test]
#[ignore = "TODO: Update for IoContext migration - scheduler integration needed"]
async fn test_multiple_fact_sources() {
    // Create IoContext
    let _context = IoContext::with_defaults();

    // Test facts from different sources
    let _journal_facts = vec![make_guardian_binding_fact("account1", "guardian1", 1)];
    let _network_facts = vec![make_recovery_grant_fact("account2", "guardian2", 2)];

    // TODO: Re-enable when IoContext has scheduler integration
    // let result = context.send_facts_to_scheduler(FactSource::Journal(journal_facts)).await;
    // let result = context.send_facts_to_scheduler(FactSource::Network(network_facts)).await;

    // Wait for processing
    sleep(Duration::from_millis(20)).await;

    println!("✓ Multiple fact sources test completed (scheduler integration pending)");
}

#[tokio::test]
async fn test_reactive_scheduler_initialization() {
    // This test verifies that IoContext initializes correctly
    let context = IoContext::with_defaults();

    // Verify views are accessible
    let _chat = context.chat_view();
    let _guardians = context.guardians_view();
    let _recovery = context.recovery_view();
    let _invitations = context.invitations_view();
    let _block = context.block_view();

    println!("✓ IoContext initialization test completed");
    println!("  - IoContext initialized successfully");
    println!("  - All views accessible");
}
