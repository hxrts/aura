//! Storage Authorization End-to-End Test
//!
//! This test validates storage access control using authority-based permissions.

use aura_agent::runtime::AuthorityManager;
use aura_core::{AuthorityId, Result};
use aura_testkit::stateful_effects::random::MockRandomHandler;

/// Test storage context creation
#[tokio::test]
async fn test_storage_context_setup() -> Result<()> {
    let test_id = AuthorityId::new();
    let mut manager = AuthorityManager::new(format!("/tmp/aura-storage-test-{}", test_id));
    let random = MockRandomHandler::new_with_seed(50);

    // Create authority for storage access
    let authority_id = manager.create_authority(&random, vec![], 1).await?;

    // Add device to authority
    manager
        .add_device_to_authority(&random, authority_id, vec![1, 2, 3, 4])
        .await?;

    // Create storage access context
    let storage_authority = AuthorityId::new(); // Represents storage service
    let context_id = manager
        .create_context(
            vec![authority_id, storage_authority],
            "storage_access".to_string(),
        )
        .await?;

    // Verify context was created
    let context = manager
        .get_context(&context_id)
        .expect("context should exist");
    assert_eq!(context.participants.len(), 2);
    assert!(context.participants.contains(&authority_id));
    assert!(context.participants.contains(&storage_authority));

    Ok(())
}

/// Test multiple storage contexts
#[tokio::test]
async fn test_multiple_storage_contexts() -> Result<()> {
    let test_id = AuthorityId::new();
    let mut manager = AuthorityManager::new(format!("/tmp/aura-multi-storage-test-{}", test_id));
    let random = MockRandomHandler::new_with_seed(51);

    // Create user authority
    let user_id = manager.create_authority(&random, vec![], 1).await?;

    // Create different storage contexts
    let storage_types = vec!["documents", "photos", "backups"];
    let mut context_ids = Vec::new();

    for storage_type in storage_types {
        let storage_authority = AuthorityId::new();
        let context_id = manager
            .create_context(
                vec![user_id, storage_authority],
                format!("storage_{}", storage_type),
            )
            .await?;
        context_ids.push(context_id);
    }

    // Verify all storage contexts were created
    assert_eq!(context_ids.len(), 3);
    for context_id in context_ids {
        let context = manager
            .get_context(&context_id)
            .expect("context should exist");
        assert!(context.participants.contains(&user_id));
    }

    Ok(())
}

/// Test storage authority with device threshold
#[tokio::test]
async fn test_storage_with_threshold() -> Result<()> {
    let test_id = AuthorityId::new();
    let mut manager =
        AuthorityManager::new(format!("/tmp/aura-storage-threshold-test-{}", test_id));
    let random = MockRandomHandler::new_with_seed(52);

    // Create authority with multiple devices
    let authority_id = manager.create_authority(&random, vec![], 1).await?;
    for i in 0..3 {
        manager
            .add_device_to_authority(&random, authority_id, vec![i; 4])
            .await?;
    }

    // Set threshold for storage operations
    manager.update_authority_threshold(&random, authority_id, 2).await?;

    // Create storage context
    let storage_authority = AuthorityId::new();
    let context_id = manager
        .create_context(
            vec![authority_id, storage_authority],
            "secure_storage".to_string(),
        )
        .await?;

    // Verify setup
    let context = manager
        .get_context(&context_id)
        .expect("context should exist");
    assert!(context.participants.contains(&authority_id));

    let (threshold, device_count, _) = manager.get_authority_tree_info(authority_id).await?;
    assert!(threshold > 0);
    assert!(device_count > 0);

    Ok(())
}
