//! DKD crypto integration tests

use aura_choreography::integration::crypto_bridge::DkdCryptoBridge;
use aura_types::effects::Effects;
use std::collections::HashMap;

/// Test DKD crypto bridge basic functionality
#[tokio::test]
async fn test_dkd_crypto_bridge_basic() {
    let effects = Effects::deterministic(42, 0);
    let bridge = DkdCryptoBridge::new(effects);
    
    let app_id = "test_app";
    let context = "test_context";
    
    // Generate share for participant 0
    let share_0 = bridge.derive_key_share(app_id, context, 0).await
        .expect("Failed to derive share 0");
    
    // Generate share for participant 1 
    let share_1 = bridge.derive_key_share(app_id, context, 1).await
        .expect("Failed to derive share 1");
    
    // Verify shares are different
    assert_ne!(share_0, share_1, "Different participants should generate different shares");
    
    // Verify shares are not empty
    assert!(!share_0.is_empty(), "Share should not be empty");
    assert!(!share_1.is_empty(), "Share should not be empty");
    
    // Verify shares are the expected length
    assert_eq!(share_0.len(), 32, "Share should be 32 bytes");
    assert_eq!(share_1.len(), 32, "Share should be 32 bytes");
}

/// Test DKD share aggregation
#[tokio::test]
async fn test_dkd_share_aggregation() {
    let effects = Effects::deterministic(12345, 0);
    let bridge = DkdCryptoBridge::new(effects);
    
    let app_id = "aggregation_test";
    let context = "test_context";
    
    // Generate shares for 3 participants
    let mut shares = Vec::new();
    for i in 0..3 {
        let share = bridge.derive_key_share(app_id, context, i).await
            .expect(&format!("Failed to derive share {}", i));
        shares.push(share);
    }
    
    // Aggregate shares
    let aggregated = bridge.aggregate_shares(&shares).await
        .expect("Failed to aggregate shares");
    
    // Verify aggregated result
    assert!(!aggregated.is_empty(), "Aggregated key should not be empty");
    assert_eq!(aggregated.len(), 32, "Aggregated key should be 32 bytes");
    assert_ne!(aggregated, vec![0u8; 32], "Aggregated key should not be all zeros");
    
    // Verify aggregation is deterministic
    let aggregated_2 = bridge.aggregate_shares(&shares).await
        .expect("Failed to aggregate shares second time");
    assert_eq!(aggregated, aggregated_2, "Aggregation should be deterministic");
}

/// Test DKD result verification
#[tokio::test]
async fn test_dkd_result_verification() {
    let effects = Effects::deterministic(98765, 0);
    let bridge = DkdCryptoBridge::new(effects.clone());
    
    let app_id = "verification_test";
    let context = "test_context";
    
    // Generate and aggregate shares
    let mut shares = Vec::new();
    for i in 0..3 {
        let share = bridge.derive_key_share(app_id, context, i).await
            .expect(&format!("Failed to derive share {}", i));
        shares.push(share);
    }
    
    let aggregated = bridge.aggregate_shares(&shares).await
        .expect("Failed to aggregate shares");
    
    // Generate hash of the aggregated key
    let key_hash = effects.blake3_hash(&aggregated);
    
    // Verify correct hash
    let is_valid = bridge.verify_result(&aggregated, &key_hash).await
        .expect("Failed to verify result");
    assert!(is_valid, "Valid hash should verify successfully");
    
    // Test with incorrect hash
    let wrong_hash = [0u8; 32];
    let is_invalid = bridge.verify_result(&aggregated, &wrong_hash).await
        .expect("Failed to verify wrong result");
    assert!(!is_invalid, "Invalid hash should fail verification");
}

/// Test DKD deterministic behavior across multiple runs
#[tokio::test]
async fn test_dkd_deterministic_behavior() {
    let seed = 55555;
    let app_id = "deterministic_test";
    let context = "test_context";
    
    // Run 1
    let effects_1 = Effects::deterministic(seed, 0);
    let bridge_1 = DkdCryptoBridge::new(effects_1);
    
    let mut shares_1 = Vec::new();
    for i in 0..3 {
        let share = bridge_1.derive_key_share(app_id, context, i).await
            .expect(&format!("Failed to derive share {} in run 1", i));
        shares_1.push(share);
    }
    let result_1 = bridge_1.aggregate_shares(&shares_1).await
        .expect("Failed to aggregate shares in run 1");
    
    // Run 2 with same seed
    let effects_2 = Effects::deterministic(seed, 0);
    let bridge_2 = DkdCryptoBridge::new(effects_2);
    
    let mut shares_2 = Vec::new();
    for i in 0..3 {
        let share = bridge_2.derive_key_share(app_id, context, i).await
            .expect(&format!("Failed to derive share {} in run 2", i));
        shares_2.push(share);
    }
    let result_2 = bridge_2.aggregate_shares(&shares_2).await
        .expect("Failed to aggregate shares in run 2");
    
    // Verify deterministic behavior
    assert_eq!(shares_1, shares_2, "Same seed should produce same shares");
    assert_eq!(result_1, result_2, "Same seed should produce same aggregated result");
}

/// Test DKD with different contexts
#[tokio::test]
async fn test_dkd_different_contexts() {
    let effects = Effects::deterministic(77777, 0);
    let bridge = DkdCryptoBridge::new(effects);
    
    let app_id = "context_test";
    let contexts = ["context_1", "context_2"];
    
    let mut results = HashMap::new();
    
    for context in &contexts {
        let mut shares = Vec::new();
        for i in 0..3 {
            let share = bridge.derive_key_share(app_id, context, i).await
                .expect(&format!("Failed to derive share {} for context {}", i, context));
            shares.push(share);
        }
        
        let aggregated = bridge.aggregate_shares(&shares).await
            .expect(&format!("Failed to aggregate shares for context {}", context));
        
        results.insert(*context, aggregated);
    }
    
    // Verify different contexts produce different results
    let result_1 = &results["context_1"];
    let result_2 = &results["context_2"];
    assert_ne!(result_1, result_2, "Different contexts should produce different keys");
}

/// Test DKD error handling with invalid inputs
#[tokio::test]
async fn test_dkd_error_handling() {
    let effects = Effects::deterministic(88888, 0);
    let bridge = DkdCryptoBridge::new(effects);
    
    // Test with empty shares
    let empty_shares: Vec<Vec<u8>> = vec![];
    let result = bridge.aggregate_shares(&empty_shares).await;
    assert!(result.is_err(), "Empty shares should cause an error");
    
    // Test with malformed shares
    let malformed_shares = vec![vec![1, 2, 3], vec![4, 5, 6]];
    let result = bridge.aggregate_shares(&malformed_shares).await;
    // This might succeed or fail depending on implementation - both are valid
    
    // Test verification with wrong key length
    let wrong_key = vec![1u8; 16]; // Wrong length
    let hash = [0u8; 32];
    let result = bridge.verify_result(&wrong_key, &hash).await;
    assert!(result.is_err(), "Wrong key length should cause an error");
}

/// Test DKD with maximum participants
#[tokio::test]
async fn test_dkd_many_participants() {
    let effects = Effects::deterministic(99999, 0);
    let bridge = DkdCryptoBridge::new(effects);
    
    let app_id = "many_participants_test";
    let context = "test_context";
    let num_participants = 10;
    
    // Generate shares for many participants
    let mut shares = Vec::new();
    for i in 0..num_participants {
        let share = bridge.derive_key_share(app_id, context, i).await
            .expect(&format!("Failed to derive share {}", i));
        shares.push(share);
    }
    
    // Verify all shares are unique
    for i in 0..shares.len() {
        for j in (i + 1)..shares.len() {
            assert_ne!(shares[i], shares[j], 
                      "Shares {} and {} should be different", i, j);
        }
    }
    
    // Aggregate all shares
    let aggregated = bridge.aggregate_shares(&shares).await
        .expect("Failed to aggregate many shares");
    
    assert!(!aggregated.is_empty(), "Aggregated result should not be empty");
    assert_eq!(aggregated.len(), 32, "Aggregated result should be 32 bytes");
}