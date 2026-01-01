//! Guardian Recovery End-to-End Test
//!
//! This test validates guardian relationship setup and recovery context creation.

use aura_agent::runtime::AuthorityManager;
use aura_core::{hash, Hash32};
use aura_core::{AuthorityId, Result};
use aura_testkit::stateful_effects::random::MockRandomHandler;
use aura_core::relational::{GuardianBinding, GuardianParameters, RelationalFact};
use aura_relational::RelationalContext;

/// Test guardian binding creation
#[tokio::test]
async fn test_guardian_binding_setup() -> Result<()> {
    let mut manager = AuthorityManager::new("/tmp/aura-guardian-test".into());
    let random = MockRandomHandler::new_with_seed(48);

    // Create account and guardian authorities
    let account_id = manager.create_authority(&random, vec![], 1).await?;
    let guardian_id = manager.create_authority(&random, vec![], 1).await?;

    // Create relational context for guardian relationship
    let context_id = manager
        .create_context(vec![account_id, guardian_id], "guardian".to_string())
        .await?;

    // Verify context was created with both participants
    let context = manager
        .get_context(&context_id)
        .expect("context should exist");
    assert_eq!(context.participants.len(), 2);
    assert!(context.participants.contains(&account_id));
    assert!(context.participants.contains(&guardian_id));

    Ok(())
}

/// Test multiple guardian setup
#[tokio::test]
async fn test_multiple_guardians() -> Result<()> {
    let mut manager = AuthorityManager::new("/tmp/aura-multi-guardian-test".into());
    let random = MockRandomHandler::new_with_seed(49);

    // Create account authority
    let account_id = manager.create_authority(&random, vec![], 1).await?;

    // Create multiple guardians
    let mut guardian_ids = Vec::new();
    let mut context_ids = Vec::new();

    for _ in 0..3 {
        let guardian_id = manager.create_authority(&random, vec![], 1).await?;
        guardian_ids.push(guardian_id);

        // Create context for each guardian relationship
        let context_id = manager
            .create_context(vec![account_id, guardian_id], "guardian".to_string())
            .await?;
        context_ids.push(context_id);
    }

    // Verify all guardian contexts were created
    assert_eq!(context_ids.len(), 3);
    for context_id in context_ids {
        let context = manager
            .get_context(&context_id)
            .expect("context should exist");
        assert!(context.participants.contains(&account_id));
    }

    Ok(())
}

/// Test guardian binding in relational context
#[test]
fn test_guardian_binding_facts() {
    let account_id = AuthorityId::new_from_entropy([1u8; 32]);
    let guardian_id = AuthorityId::new_from_entropy([1u8; 32]);

    let mut context = RelationalContext::new(vec![account_id, guardian_id]);

    // Create guardian binding
    let account_hash = hash::hash(&account_id.to_bytes());
    let guardian_hash = hash::hash(&guardian_id.to_bytes());

    let binding = GuardianBinding::new(
        Hash32::new(account_hash),
        Hash32::new(guardian_hash),
        GuardianParameters::default(),
    );

    // Add binding to context
    context
        .add_fact(RelationalFact::GuardianBinding(binding))
        .unwrap();

    // Verify binding was recorded
    assert_eq!(context.guardian_bindings().len(), 1);
}
