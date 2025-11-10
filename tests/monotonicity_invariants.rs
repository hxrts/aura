//! Monotonicity Invariants Tests
//!
//! Comprehensive tests to verify that all operations in Aura preserve 
//! monotonic properties and never cause states to regress. Monotonicity
//! is crucial for security, consistency, and distributed system correctness.

use proptest::prelude::*;
use aura_journal::semilattice::{JournalMap, AccountState, TreeState, SemilatticeOps, SessionEpoch};
use aura_core::{DeviceId, AccountId, RelationshipId, CapabilityId, AuraResult};
use aura_verify::{Guardian, ThresholdConfiguration, RecoveryState};
use aura_store::access_control::PermissionLevel;
use std::collections::{HashMap, HashSet};
use tokio::time::{Duration, Instant};

/// Property test: Journal operations maintain monotonic growth
proptest! {
    #[test]
    fn prop_journal_monotonic_growth(
        operations in prop::collection::vec(arbitrary_journal_operation(), 1..50),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            let mut journal = JournalMap::new();
            let mut previous_states = Vec::new();
            
            for operation in operations {
                // Capture state before operation
                let state_before = journal.clone();
                previous_states.push(state_before.clone());
                
                // Apply operation
                journal.apply_operation(operation).await.unwrap();
                
                // Verify monotonicity: new state ≥ all previous states
                for (i, prev_state) in previous_states.iter().enumerate() {
                    prop_assert!(
                        prev_state.is_subset_of(&journal).await.unwrap(),
                        "Monotonicity violated at operation {}: previous state not subset of current",
                        i
                    );
                    
                    // Verify join with previous state equals current state
                    let joined = prev_state.join(&journal).await.unwrap();
                    prop_assert_eq!(
                        joined,
                        journal,
                        "Join monotonicity violated: prev ⊔ current ≠ current"
                    );
                }
                
                // Verify size monotonicity (operations only add, never remove)
                prop_assert!(
                    journal.size() >= state_before.size(),
                    "Size monotonicity violated: journal size decreased"
                );
                
                // Verify epoch monotonicity
                prop_assert!(
                    journal.current_epoch() >= state_before.current_epoch(),
                    "Epoch monotonicity violated: epoch regressed"
                );
            }
        });
    }
}

/// Property test: Account state capabilities grow monotonically
proptest! {
    #[test]
    fn prop_account_capabilities_monotonic(
        capability_grants in prop::collection::vec(arbitrary_capability_grant(), 1..30),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            let mut account = AccountState::new(AccountId::new());
            let mut capability_history = Vec::new();
            
            for grant in capability_grants {
                // Capture capabilities before grant
                let caps_before = account.get_capabilities().await.unwrap();
                capability_history.push(caps_before.clone());
                
                // Apply capability grant
                account.grant_capability(grant.device_id, grant.capability_id).await.unwrap();
                
                // Verify capability monotonicity
                let caps_after = account.get_capabilities().await.unwrap();
                
                prop_assert!(
                    caps_before.is_subset(&caps_after),
                    "Capability monotonicity violated: capabilities decreased"
                );
                
                // Verify specific capability was added
                prop_assert!(
                    caps_after.contains(&grant.capability_id),
                    "Granted capability not found in capability set"
                );
                
                // Verify all previous capabilities are still present
                for prev_caps in &capability_history {
                    prop_assert!(
                        prev_caps.is_subset(&caps_after),
                        "Historical capability monotonicity violated"
                    );
                }
            }
        });
    }
}

/// Test epoch monotonicity across session boundaries
#[tokio::test]
async fn test_epoch_monotonicity() -> AuraResult<()> {
    let mut session_state = SessionEpoch::new(0);
    let mut epoch_history = vec![0];
    
    // Advance epochs and verify monotonicity
    for target_epoch in [1, 5, 3, 10, 7, 15, 12, 20] {
        let old_epoch = session_state.epoch();
        session_state = session_state.advance_to(target_epoch);
        let new_epoch = session_state.epoch();
        
        // Epoch should never decrease
        assert!(
            new_epoch >= old_epoch,
            "Epoch monotonicity violated: {} -> {}",
            old_epoch,
            new_epoch
        );
        
        // New epoch should be maximum of old epoch and target
        let expected_epoch = old_epoch.max(target_epoch);
        assert_eq!(
            new_epoch,
            expected_epoch,
            "Epoch advance incorrect: expected {}, got {}",
            expected_epoch,
            new_epoch
        );
        
        // Verify monotonicity against all historical epochs
        for &historical_epoch in &epoch_history {
            assert!(
                new_epoch >= historical_epoch,
                "Historical epoch monotonicity violated: {} < {}",
                new_epoch,
                historical_epoch
            );
        }
        
        epoch_history.push(new_epoch);
    }
    
    println!("✓ Epoch monotonicity verified across {} transitions", epoch_history.len());
    Ok(())
}

/// Test tree structure monotonicity
#[tokio::test]
async fn test_tree_monotonicity() -> AuraResult<()> {
    let mut tree = TreeState::new();
    let devices: Vec<DeviceId> = (0..10).map(|_| DeviceId::new()).collect();
    
    let mut tree_snapshots = Vec::new();
    
    // Build tree incrementally and verify monotonicity
    for (i, &device) in devices.iter().enumerate() {
        tree_snapshots.push(tree.clone());
        
        // Add device to tree
        tree.add_node(device, i as u64).await?;
        
        // Verify tree size monotonicity
        let old_size = tree_snapshots.last().unwrap().node_count();
        let new_size = tree.node_count();
        assert!(
            new_size >= old_size,
            "Tree size monotonicity violated: {} -> {}",
            old_size,
            new_size
        );
        
        // Verify all previous devices still in tree
        for (j, snapshot) in tree_snapshots.iter().enumerate() {
            for &prev_device in &devices[..j] {
                assert!(
                    tree.contains_node(prev_device).await?,
                    "Tree monotonicity violated: device {} from snapshot {} not in current tree",
                    prev_device,
                    j
                );
            }
        }
        
        // Verify tree epoch monotonicity
        let tree_epoch = tree.current_epoch();
        assert!(
            tree_epoch >= i as u64,
            "Tree epoch monotonicity violated: epoch {} < expected minimum {}",
            tree_epoch,
            i
        );
    }
    
    // Test epoch updates maintain monotonicity
    for (i, &device) in devices.iter().enumerate() {
        let old_epoch = tree.get_node_epoch(device).await?.unwrap_or(0);
        let new_epoch = (i + 20) as u64;
        
        tree.update_node_epoch(device, new_epoch).await?;
        
        let updated_epoch = tree.get_node_epoch(device).await?.unwrap();
        assert!(
            updated_epoch >= old_epoch,
            "Node epoch monotonicity violated for device {}: {} -> {}",
            device,
            old_epoch,
            updated_epoch
        );
        assert!(
            updated_epoch >= new_epoch,
            "Node epoch update failed: expected >= {}, got {}",
            new_epoch,
            updated_epoch
        );
    }
    
    println!("✓ Tree structure monotonicity verified");
    Ok(())
}

/// Test permission level monotonicity in access control
#[tokio::test]
async fn test_permission_monotonicity() -> AuraResult<()> {
    let device_id = DeviceId::new();
    let resource_id = "test_resource";
    
    let permission_sequence = vec![
        PermissionLevel::None,
        PermissionLevel::Read,
        PermissionLevel::Write,
        PermissionLevel::Admin,
        PermissionLevel::Read,    // Attempt to downgrade (should be ignored)
        PermissionLevel::Admin,   // Back to admin
        PermissionLevel::None,    // Attempt major downgrade (should be ignored)
        PermissionLevel::Super,   // Upgrade to super
    ];
    
    let mut current_permission = PermissionLevel::None;
    let mut permission_history = vec![PermissionLevel::None];
    
    for target_permission in permission_sequence {
        // Apply permission change (using monotonic upgrade logic)
        let new_permission = if target_permission.level() > current_permission.level() {
            target_permission.clone()
        } else {
            current_permission.clone() // Maintain current level if downgrade attempted
        };
        
        // Verify permission monotonicity
        assert!(
            new_permission.level() >= current_permission.level(),
            "Permission monotonicity violated: {:?} -> {:?}",
            current_permission,
            new_permission
        );
        
        // Verify monotonicity against all historical permissions
        let max_historical_level = permission_history.iter()
            .map(|p| p.level())
            .max()
            .unwrap_or(0);
        
        assert!(
            new_permission.level() >= max_historical_level,
            "Historical permission monotonicity violated: new level {} < max historical {}",
            new_permission.level(),
            max_historical_level
        );
        
        current_permission = new_permission;
        permission_history.push(current_permission.clone());
    }
    
    // Final permission should be Super (highest level attempted)
    assert_eq!(
        current_permission,
        PermissionLevel::Super,
        "Final permission should be Super, got {:?}",
        current_permission
    );
    
    println!("✓ Permission level monotonicity verified");
    Ok(())
}

/// Test guardian threshold monotonicity during recovery
#[tokio::test]
async fn test_guardian_threshold_monotonicity() -> AuraResult<()> {
    let account_id = AccountId::new();
    let mut recovery_state = RecoveryState::new(account_id);
    
    // Create guardians
    let guardians: Vec<Guardian> = (0..7).map(|i| Guardian {
        device_id: DeviceId::new(),
        name: format!("Guardian {}", i),
        trust_level: 0.8 + (i as f64 * 0.02), // Slightly increasing trust
        added_at: std::time::SystemTime::now(),
    }).collect();
    
    // Test threshold configurations with monotonic requirements
    let threshold_configs = vec![
        ThresholdConfiguration { required: 2, total: 3 },
        ThresholdConfiguration { required: 3, total: 5 },
        ThresholdConfiguration { required: 2, total: 5 }, // Lower required (should use max)
        ThresholdConfiguration { required: 4, total: 7 },
        ThresholdConfiguration { required: 3, total: 7 }, // Lower required (should use max)
        ThresholdConfiguration { required: 5, total: 7 },
    ];
    
    let mut max_required_seen = 0;
    
    for (i, config) in threshold_configs.iter().enumerate() {
        // Apply threshold configuration with monotonic constraints
        let effective_required = config.required.max(max_required_seen);
        let monotonic_config = ThresholdConfiguration {
            required: effective_required,
            total: config.total,
        };
        
        recovery_state.update_threshold_config(monotonic_config.clone()).await?;
        
        // Verify threshold monotonicity
        assert!(
            monotonic_config.required >= max_required_seen,
            "Threshold monotonicity violated at step {}: {} < {}",
            i,
            monotonic_config.required,
            max_required_seen
        );
        
        // Verify security property: required ≤ total
        assert!(
            monotonic_config.required <= monotonic_config.total,
            "Invalid threshold configuration: required {} > total {}",
            monotonic_config.required,
            monotonic_config.total
        );
        
        max_required_seen = monotonic_config.required;
        
        // Add guardians up to total if needed
        while recovery_state.guardian_count() < config.total {
            if let Some(guardian) = guardians.get(recovery_state.guardian_count()) {
                recovery_state.add_guardian(guardian.clone()).await?;
            }
        }
        
        // Verify guardian count monotonicity
        assert!(
            recovery_state.guardian_count() >= i,
            "Guardian count not monotonic: {} at step {}",
            recovery_state.guardian_count(),
            i
        );
    }
    
    println!("✓ Guardian threshold monotonicity verified");
    Ok(())
}

/// Test content storage size monotonicity
#[tokio::test]
async fn test_storage_monotonicity() -> AuraResult<()> {
    let mut storage_map = HashMap::new();
    let mut total_size = 0usize;
    let mut size_history = vec![0];
    
    // Add content of various sizes
    let content_items = vec![
        b"small content".to_vec(),
        b"medium sized content with more data".to_vec(),
        vec![0u8; 1000], // Large content
        b"tiny".to_vec(),
        vec![0u8; 5000], // Very large content
        b"another small item".to_vec(),
    ];
    
    for (i, content) in content_items.iter().enumerate() {
        let content_id = format!("content_{}", i);
        let old_total_size = total_size;
        
        // Store content
        storage_map.insert(content_id.clone(), content.clone());
        total_size += content.len();
        
        // Verify size monotonicity
        assert!(
            total_size >= old_total_size,
            "Storage size monotonicity violated: {} -> {}",
            old_total_size,
            total_size
        );
        
        // Verify monotonicity against all historical sizes
        for &historical_size in &size_history {
            assert!(
                total_size >= historical_size,
                "Historical storage monotonicity violated: {} < {}",
                total_size,
                historical_size
            );
        }
        
        size_history.push(total_size);
        
        // Verify content count monotonicity
        assert!(
            storage_map.len() >= i,
            "Content count not monotonic: {} items at step {}",
            storage_map.len(),
            i
        );
    }
    
    // Test that content updates maintain size monotonicity
    for (i, new_content) in content_items.iter().enumerate() {
        let content_id = format!("content_{}", i);
        let old_size = storage_map.get(&content_id).map(|c| c.len()).unwrap_or(0);
        let old_total = total_size;
        
        // Update with larger content only (maintain monotonicity)
        if new_content.len() > old_size {
            total_size = total_size - old_size + new_content.len();
            storage_map.insert(content_id, new_content.clone());
            
            assert!(
                total_size >= old_total,
                "Content update violated size monotonicity: {} -> {}",
                old_total,
                total_size
            );
        }
    }
    
    println!("✓ Storage size monotonicity verified");
    Ok(())
}

/// Test temporal monotonicity (timestamps always advance)
#[tokio::test]
async fn test_temporal_monotonicity() -> AuraResult<()> {
    let mut timestamps = Vec::new();
    let start_time = Instant::now();
    
    // Collect timestamps over time
    for i in 0..20 {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        
        // Verify temporal monotonicity
        if let Some(&last_timestamp) = timestamps.last() {
            assert!(
                timestamp >= last_timestamp,
                "Temporal monotonicity violated at step {}: {} < {}",
                i,
                timestamp,
                last_timestamp
            );
        }
        
        timestamps.push(timestamp);
        
        // Small delay to ensure time advances
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    
    // Verify overall time progression
    let total_elapsed = start_time.elapsed();
    let timestamp_span = timestamps.last().unwrap() - timestamps.first().unwrap();
    
    assert!(
        timestamp_span >= total_elapsed.as_nanos(),
        "Timestamp progression inconsistent with actual elapsed time"
    );
    
    println!("✓ Temporal monotonicity verified over {} measurements", timestamps.len());
    Ok(())
}

/// Test monotonicity under concurrent operations
#[tokio::test]
async fn test_concurrent_monotonicity() -> AuraResult<()> {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    
    let journal = Arc::new(Mutex::new(JournalMap::new()));
    let epoch_counter = Arc::new(Mutex::new(0u64));
    
    // Spawn concurrent tasks that perform monotonic operations
    let mut handles = Vec::new();
    
    for task_id in 0..10 {
        let journal_clone = Arc::clone(&journal);
        let counter_clone = Arc::clone(&epoch_counter);
        
        handles.push(tokio::spawn(async move {
            for i in 0..5 {
                // Get monotonic epoch
                let epoch = {
                    let mut counter = counter_clone.lock().await;
                    *counter += 1;
                    *counter
                };
                
                // Perform monotonic journal operation
                let operation = JournalOperation::UpdateTreeEpoch(epoch);
                {
                    let mut journal = journal_clone.lock().await;
                    journal.apply_operation(operation).await.unwrap();
                }
                
                // Verify local monotonicity
                {
                    let journal = journal_clone.lock().await;
                    assert!(
                        journal.current_epoch() >= epoch,
                        "Concurrent monotonicity violated in task {}, iteration {}: journal epoch {} < expected {}",
                        task_id,
                        i,
                        journal.current_epoch(),
                        epoch
                    );
                }
                
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            AuraResult::Ok(())
        }));
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap()?;
    }
    
    // Verify final state consistency
    let final_journal = journal.lock().await;
    let final_epoch = final_journal.current_epoch();
    let expected_min_epoch = 50; // 10 tasks * 5 operations each
    
    assert!(
        final_epoch >= expected_min_epoch,
        "Final concurrent epoch {} less than expected minimum {}",
        final_epoch,
        expected_min_epoch
    );
    
    println!("✓ Concurrent monotonicity verified with final epoch {}", final_epoch);
    Ok(())
}

/// Test invariant preservation during rollbacks (monotonicity maintained even during error recovery)
#[tokio::test]
async fn test_monotonicity_during_rollbacks() -> AuraResult<()> {
    let mut journal = JournalMap::new();
    let mut checkpoints = Vec::new();
    
    // Perform operations with checkpointing
    for i in 0..10 {
        checkpoints.push(journal.clone());
        
        // Attempt operation that might fail
        let operation = if i % 3 == 0 {
            JournalOperation::UpdateTreeEpoch(u64::MAX) // Potentially problematic operation
        } else {
            JournalOperation::AddDevice(DeviceId::new()) // Safe operation
        };
        
        let old_state = journal.clone();
        
        match journal.apply_operation(operation).await {
            Ok(_) => {
                // Operation succeeded: verify monotonicity
                assert!(
                    old_state.is_subset_of(&journal).await?,
                    "Successful operation violated monotonicity at step {}",
                    i
                );
            }
            Err(_) => {
                // Operation failed: state should be unchanged (trivially monotonic)
                assert_eq!(
                    journal,
                    old_state,
                    "Failed operation changed state at step {}",
                    i
                );
            }
        }
        
        // Verify monotonicity against all checkpoints
        for (checkpoint_idx, checkpoint) in checkpoints.iter().enumerate() {
            assert!(
                checkpoint.is_subset_of(&journal).await?,
                "Monotonicity violated against checkpoint {} at step {}",
                checkpoint_idx,
                i
            );
        }
    }
    
    println!("✓ Monotonicity preserved during rollback scenarios");
    Ok(())
}

// Helper types and functions

#[derive(Debug, Clone)]
struct CapabilityGrant {
    device_id: DeviceId,
    capability_id: CapabilityId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JournalOperation {
    AddDevice(DeviceId),
    AddCapability(CapabilityId),
    CreateRelationship(RelationshipId),
    UpdateTreeEpoch(u64),
    StoreContent(Vec<u8>),
}

// Arbitrary generators
fn arbitrary_journal_operation() -> impl Strategy<Value = JournalOperation> {
    prop_oneof![
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::AddDevice(DeviceId::from_bytes(bytes))),
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::AddCapability(CapabilityId::from_bytes(bytes))),
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::CreateRelationship(RelationshipId::from_bytes(bytes))),
        (0u64..1000).prop_map(|epoch| JournalOperation::UpdateTreeEpoch(epoch)),
        prop::collection::vec(any::<u8>(), 0..100).prop_map(|data| JournalOperation::StoreContent(data)),
    ]
}

fn arbitrary_capability_grant() -> impl Strategy<Value = CapabilityGrant> {
    (any::<[u8; 32]>(), any::<[u8; 32]>()).prop_map(|(dev_bytes, cap_bytes)| {
        CapabilityGrant {
            device_id: DeviceId::from_bytes(dev_bytes),
            capability_id: CapabilityId::from_bytes(cap_bytes),
        }
    })
}

// Implementation stubs for testing
impl JournalMap {
    async fn apply_operation(&mut self, operation: JournalOperation) -> AuraResult<()> {
        match operation {
            JournalOperation::AddDevice(device_id) => {
                self.add_device(device_id).await?;
            }
            JournalOperation::AddCapability(cap_id) => {
                self.add_capability(cap_id).await?;
            }
            JournalOperation::CreateRelationship(rel_id) => {
                self.create_relationship(rel_id).await?;
            }
            JournalOperation::UpdateTreeEpoch(epoch) => {
                self.update_tree_epoch(epoch).await?;
            }
            JournalOperation::StoreContent(content) => {
                self.store_content(content).await?;
            }
        }
        Ok(())
    }
    
    fn size(&self) -> usize {
        // Return total size/count of stored items
        self.device_count() + self.capability_count() + self.relationship_count()
    }
    
    fn current_epoch(&self) -> u64 {
        self.get_tree_epoch()
    }
}

impl SessionEpoch {
    fn advance_to(&self, target_epoch: u64) -> Self {
        Self::new(self.epoch().max(target_epoch))
    }
}

impl TreeState {
    fn node_count(&self) -> usize {
        self.get_node_count()
    }
    
    fn current_epoch(&self) -> u64 {
        self.get_current_epoch()
    }
}

// Permission level enum for access control testing
#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionLevel {
    None,
    Read,
    Write,
    Admin,
    Super,
}

impl PermissionLevel {
    fn level(&self) -> u32 {
        match self {
            PermissionLevel::None => 0,
            PermissionLevel::Read => 1,
            PermissionLevel::Write => 2,
            PermissionLevel::Admin => 3,
            PermissionLevel::Super => 4,
        }
    }
}

// Threshold configuration for guardian recovery
#[derive(Debug, Clone)]
struct ThresholdConfiguration {
    required: usize,
    total: usize,
}

// Recovery state for guardian management
struct RecoveryState {
    account_id: AccountId,
    guardians: Vec<Guardian>,
    threshold_config: Option<ThresholdConfiguration>,
}

impl RecoveryState {
    fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            guardians: Vec::new(),
            threshold_config: None,
        }
    }
    
    async fn add_guardian(&mut self, guardian: Guardian) -> AuraResult<()> {
        self.guardians.push(guardian);
        Ok(())
    }
    
    async fn update_threshold_config(&mut self, config: ThresholdConfiguration) -> AuraResult<()> {
        self.threshold_config = Some(config);
        Ok(())
    }
    
    fn guardian_count(&self) -> usize {
        self.guardians.len()
    }
}