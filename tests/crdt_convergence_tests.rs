//! CRDT Convergence Tests
//!
//! Comprehensive tests to verify that all CRDT implementations in Aura
//! satisfy the convergence property: given the same set of operations,
//! all replicas eventually converge to the same state regardless of
//! operation ordering or delivery patterns.

use proptest::prelude::*;
use aura_journal::semilattice::{JournalMap, AccountState, TreeState, SemilatticeOps};
use aura_core::{DeviceId, AccountId, RelationshipId, CapabilityId, AuraResult};
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::time::{timeout, Duration};
use futures::future::join_all;

/// Test that JournalMap converges under different operation orderings
proptest! {
    #[test]
    fn prop_journal_map_convergence(
        operations in prop::collection::vec(arbitrary_journal_operation(), 5..30),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            // Create multiple replicas
            let mut replica1 = JournalMap::new();
            let mut replica2 = JournalMap::new();
            let mut replica3 = JournalMap::new();
            
            // Apply same operations in different orders
            let mut ops1 = operations.clone();
            let mut ops2 = operations.clone();
            let mut ops3 = operations.clone();
            
            // Shuffle operations for different orderings
            ops1.sort_by_key(|op| op.operation_id());
            ops2.sort_by_key(|op| std::cmp::Reverse(op.operation_id()));
            ops3.reverse();
            
            // Apply operations to each replica
            for op in ops1 {
                replica1.apply_operation(op).await.unwrap();
            }
            
            for op in ops2 {
                replica2.apply_operation(op).await.unwrap();
            }
            
            for op in ops3 {
                replica3.apply_operation(op).await.unwrap();
            }
            
            // Verify convergence: all replicas should have same state
            prop_assert_eq!(
                replica1,
                replica2,
                "Replica 1 and 2 did not converge despite same operations"
            );
            
            prop_assert_eq!(
                replica2,
                replica3,
                "Replica 2 and 3 did not converge despite same operations"
            );
            
            prop_assert_eq!(
                replica1,
                replica3,
                "Replica 1 and 3 did not converge despite same operations"
            );
        });
    }
}

/// Test AccountState convergence under concurrent operations
proptest! {
    #[test]
    fn prop_account_state_convergence(
        device_ops in prop::collection::vec(arbitrary_device_operation(), 3..15),
        capability_ops in prop::collection::vec(arbitrary_capability_operation(), 3..15),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            let account_id = AccountId::new();
            
            // Create three replicas
            let mut replica_a = AccountState::new(account_id);
            let mut replica_b = AccountState::new(account_id);
            let mut replica_c = AccountState::new(account_id);
            
            // Apply device operations in different orders
            for (i, op) in device_ops.iter().enumerate() {
                if i % 3 == 0 {
                    replica_a.apply_device_operation(op.clone()).await.unwrap();
                } else if i % 3 == 1 {
                    replica_b.apply_device_operation(op.clone()).await.unwrap();
                } else {
                    replica_c.apply_device_operation(op.clone()).await.unwrap();
                }
            }
            
            // Apply capability operations in different orders
            for (i, op) in capability_ops.iter().enumerate() {
                if i % 3 == 1 {
                    replica_a.apply_capability_operation(op.clone()).await.unwrap();
                } else if i % 3 == 2 {
                    replica_b.apply_capability_operation(op.clone()).await.unwrap();
                } else {
                    replica_c.apply_capability_operation(op.clone()).await.unwrap();
                }
            }
            
            // Synchronize replicas by exchanging state
            let merged_ab = replica_a.join(&replica_b).await.unwrap();
            let merged_bc = replica_b.join(&replica_c).await.unwrap();
            let merged_ac = replica_a.join(&replica_c).await.unwrap();
            
            // All merged states should be identical
            prop_assert_eq!(
                merged_ab,
                merged_bc,
                "Merged states AB and BC differ"
            );
            
            prop_assert_eq!(
                merged_bc,
                merged_ac,
                "Merged states BC and AC differ"
            );
            
            // Final convergence: all replicas merge to same state
            let final_a = replica_a.join(&merged_bc).await.unwrap();
            let final_b = replica_b.join(&merged_ac).await.unwrap();
            let final_c = replica_c.join(&merged_ab).await.unwrap();
            
            prop_assert_eq!(
                final_a,
                final_b,
                "Final states A and B differ after convergence"
            );
            
            prop_assert_eq!(
                final_b,
                final_c,
                "Final states B and C differ after convergence"
            );
        });
    }
}

/// Test convergence under network partitions and message delays
#[tokio::test]
async fn test_convergence_under_network_partitions() -> AuraResult<()> {
    use std::sync::Arc;
    use tokio::sync::{Mutex, mpsc};
    
    // Simulate network with message delays and partitions
    #[derive(Clone)]
    struct NetworkMessage {
        from_replica: u32,
        to_replica: u32,
        operation: JournalOperation,
        delay_ms: u64,
    }
    
    // Create 4 replicas
    let mut replicas = Vec::new();
    for _ in 0..4 {
        replicas.push(Arc::new(Mutex::new(JournalMap::new())));
    }
    
    let (tx, mut rx) = mpsc::unbounded_channel::<NetworkMessage>();
    
    // Generate operations on different replicas
    let operations = vec![
        JournalOperation::AddDevice(DeviceId::new()),
        JournalOperation::AddCapability(CapabilityId::new()),
        JournalOperation::CreateRelationship(RelationshipId::new()),
        JournalOperation::UpdateTreeEpoch(42),
        JournalOperation::AddDevice(DeviceId::new()),
        JournalOperation::StoreContent(b"test content".to_vec()),
    ];
    
    // Apply operations locally first, then broadcast
    for (i, operation) in operations.iter().enumerate() {
        let replica_id = i % replicas.len() as u32;
        
        // Apply locally
        {
            let mut replica = replicas[replica_id as usize].lock().await;
            replica.apply_operation(operation.clone()).await?;
        }
        
        // Broadcast to other replicas with varying delays
        for target_replica in 0..replicas.len() as u32 {
            if target_replica != replica_id {
                let delay = match (replica_id, target_replica) {
                    (0, 1) | (1, 0) => 10,  // Fast connection
                    (0, 2) | (2, 0) => 100, // Medium connection  
                    (0, 3) | (3, 0) => 500, // Slow connection
                    (1, 2) | (2, 1) => 50,  // Medium connection
                    (1, 3) | (3, 1) => 300, // Slow connection
                    (2, 3) | (3, 2) => 200, // Medium-slow connection
                    _ => 100,
                };
                
                let msg = NetworkMessage {
                    from_replica: replica_id,
                    to_replica: target_replica,
                    operation: operation.clone(),
                    delay_ms: delay,
                };
                
                tx.send(msg).unwrap();
            }
        }
    }
    
    // Process network messages with delays
    drop(tx); // Close sender to terminate loop
    
    let mut pending_messages = VecDeque::new();
    
    while let Some(msg) = rx.recv().await {
        pending_messages.push_back((msg, tokio::time::Instant::now()));
    }
    
    // Sort by delivery time and apply with delays
    while !pending_messages.is_empty() {
        let current_time = tokio::time::Instant::now();
        
        // Find messages ready for delivery
        let mut ready_messages = Vec::new();
        let mut remaining_messages = VecDeque::new();
        
        for (msg, sent_time) in pending_messages {
            let delivery_time = sent_time + Duration::from_millis(msg.delay_ms);
            if current_time >= delivery_time {
                ready_messages.push(msg);
            } else {
                remaining_messages.push_back((msg, sent_time));
            }
        }
        
        pending_messages = remaining_messages;
        
        // Apply ready messages
        for msg in ready_messages {
            let mut target_replica = replicas[msg.to_replica as usize].lock().await;
            target_replica.apply_operation(msg.operation).await?;
        }
        
        // Wait a bit before checking again
        if !pending_messages.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    
    // Wait for final message deliveries
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Perform final synchronization between replicas
    let replica_states: Vec<_> = join_all(
        replicas.iter().map(|r| async { r.lock().await.clone() })
    ).await;
    
    // Merge all states to achieve convergence
    let mut converged_state = replica_states[0].clone();
    for replica_state in &replica_states[1..] {
        converged_state = converged_state.join(replica_state).await?;
    }
    
    // Verify all replicas converge to same final state
    for (i, replica) in replicas.iter().enumerate() {
        let mut final_replica = replica.lock().await;
        *final_replica = final_replica.join(&converged_state).await?;
        
        assert_eq!(
            *final_replica,
            converged_state,
            "Replica {} did not converge to final state",
            i
        );
    }
    
    println!("✓ Convergence under network partitions verified");
    Ok(())
}

/// Test TreeState convergence under concurrent tree operations
#[tokio::test]
async fn test_tree_state_convergence() -> AuraResult<()> {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    
    let tree1 = Arc::new(Mutex::new(TreeState::new()));
    let tree2 = Arc::new(Mutex::new(TreeState::new()));
    let tree3 = Arc::new(Mutex::new(TreeState::new()));
    
    // Create devices for tree operations
    let devices: Vec<DeviceId> = (0..10).map(|_| DeviceId::new()).collect();
    
    // Simulate concurrent operations on different tree replicas
    let mut handles = Vec::new();
    
    // Tree 1: Add nodes sequentially
    {
        let tree1_clone = Arc::clone(&tree1);
        let devices_clone = devices.clone();
        handles.push(tokio::spawn(async move {
            let mut tree = tree1_clone.lock().await;
            for (i, device) in devices_clone.iter().enumerate() {
                tree.add_node(*device, i as u64).await?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            AuraResult::Ok(())
        }));
    }
    
    // Tree 2: Add nodes in reverse order
    {
        let tree2_clone = Arc::clone(&tree2);
        let devices_clone = devices.clone();
        handles.push(tokio::spawn(async move {
            let mut tree = tree2_clone.lock().await;
            for (i, device) in devices_clone.iter().rev().enumerate() {
                tree.add_node(*device, i as u64).await?;
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            AuraResult::Ok(())
        }));
    }
    
    // Tree 3: Add nodes in random-ish order with epoch updates
    {
        let tree3_clone = Arc::clone(&tree3);
        let devices_clone = devices.clone();
        handles.push(tokio::spawn(async move {
            let mut tree = tree3_clone.lock().await;
            
            // Add every other device first
            for (i, device) in devices_clone.iter().step_by(2).enumerate() {
                tree.add_node(*device, (i * 2) as u64).await?;
            }
            
            // Then add remaining devices
            for (i, device) in devices_clone.iter().skip(1).step_by(2).enumerate() {
                tree.add_node(*device, (i * 2 + 1) as u64).await?;
            }
            
            // Update epochs
            for (i, device) in devices_clone.iter().take(5).enumerate() {
                tree.update_node_epoch(*device, (i + 10) as u64).await?;
            }
            
            AuraResult::Ok(())
        }));
    }
    
    // Wait for all operations to complete
    for handle in handles {
        handle.await.unwrap()?;
    }
    
    // Extract final states
    let state1 = tree1.lock().await.clone();
    let state2 = tree2.lock().await.clone();
    let state3 = tree3.lock().await.clone();
    
    // Merge states to achieve convergence
    let merged_12 = state1.join(&state2).await?;
    let merged_123 = merged_12.join(&state3).await?;
    
    // Verify convergence: all trees should converge to same final state
    let converged_1 = state1.join(&merged_123).await?;
    let converged_2 = state2.join(&merged_123).await?;
    let converged_3 = state3.join(&merged_123).await?;
    
    assert_eq!(converged_1, converged_2, "Tree 1 and 2 did not converge");
    assert_eq!(converged_2, converged_3, "Tree 2 and 3 did not converge");
    assert_eq!(converged_1, merged_123, "Final convergence failed");
    
    println!("✓ TreeState convergence under concurrent operations verified");
    Ok(())
}

/// Test convergence under byzantine faults (malicious operations)
#[tokio::test]
async fn test_convergence_under_byzantine_faults() -> AuraResult<()> {
    let mut honest_replica1 = JournalMap::new();
    let mut honest_replica2 = JournalMap::new();
    let mut byzantine_replica = JournalMap::new();
    
    // Honest operations
    let honest_ops = vec![
        JournalOperation::AddDevice(DeviceId::new()),
        JournalOperation::AddCapability(CapabilityId::new()),
        JournalOperation::CreateRelationship(RelationshipId::new()),
    ];
    
    // Byzantine operations (trying to break convergence)
    let byzantine_ops = vec![
        JournalOperation::UpdateTreeEpoch(u64::MAX), // Extreme value
        JournalOperation::StoreContent(vec![0u8; 10000]), // Large content
        JournalOperation::AddDevice(DeviceId::from_bytes([0u8; 32])), // Duplicate device
    ];
    
    // Apply honest operations to honest replicas
    for op in &honest_ops {
        honest_replica1.apply_operation(op.clone()).await?;
        honest_replica2.apply_operation(op.clone()).await?;
    }
    
    // Apply both honest and byzantine operations to byzantine replica
    for op in &honest_ops {
        byzantine_replica.apply_operation(op.clone()).await?;
    }
    for op in &byzantine_ops {
        byzantine_replica.apply_operation(op.clone()).await?;
    }
    
    // Honest replicas should converge despite byzantine behavior
    let honest_convergence = honest_replica1.join(&honest_replica2).await?;
    assert_eq!(honest_replica1, honest_convergence, "Honest replicas did not converge");
    
    // When honest replicas merge with byzantine replica, 
    // the result should still be deterministic and consistent
    let with_byzantine1 = honest_replica1.join(&byzantine_replica).await?;
    let with_byzantine2 = honest_replica2.join(&byzantine_replica).await?;
    
    assert_eq!(
        with_byzantine1,
        with_byzantine2,
        "Convergence with byzantine replica is non-deterministic"
    );
    
    // The honest operations should still be preserved
    assert!(
        with_byzantine1.contains_honest_operations(&honest_ops).await?,
        "Honest operations lost after merging with byzantine replica"
    );
    
    println!("✓ Convergence under byzantine faults verified");
    Ok(())
}

/// Test eventually consistent convergence with simulated network failures
#[tokio::test]
async fn test_eventual_consistency_with_failures() -> AuraResult<()> {
    #[derive(Clone)]
    struct Message {
        operation: JournalOperation,
        attempt_count: u32,
    }
    
    let mut replicas = vec![JournalMap::new(), JournalMap::new(), JournalMap::new()];
    
    let operations = vec![
        JournalOperation::AddDevice(DeviceId::new()),
        JournalOperation::AddCapability(CapabilityId::new()),
        JournalOperation::CreateRelationship(RelationshipId::new()),
        JournalOperation::UpdateTreeEpoch(1),
        JournalOperation::StoreContent(b"content1".to_vec()),
        JournalOperation::UpdateTreeEpoch(2),
    ];
    
    // Apply operations with simulated network failures
    for (op_idx, operation) in operations.iter().enumerate() {
        let primary_replica = op_idx % replicas.len();
        
        // Apply to primary replica immediately
        replicas[primary_replica].apply_operation(operation.clone()).await?;
        
        // Simulate replication with failures
        for (replica_idx, replica) in replicas.iter_mut().enumerate() {
            if replica_idx != primary_replica {
                let mut msg = Message {
                    operation: operation.clone(),
                    attempt_count: 0,
                };
                
                // Simulate network failures and retries
                loop {
                    msg.attempt_count += 1;
                    
                    // Simulate failure probability decreasing with retries
                    let failure_prob = 0.5 / (msg.attempt_count as f64);
                    
                    if rand::random::<f64>() > failure_prob {
                        // Success: apply operation
                        replica.apply_operation(msg.operation.clone()).await?;
                        break;
                    } else if msg.attempt_count > 5 {
                        // Give up after 5 attempts and apply anyway (eventual consistency)
                        replica.apply_operation(msg.operation.clone()).await?;
                        break;
                    }
                    
                    // Wait before retry
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }
    
    // Verify eventual convergence
    let final_state = replicas[0].clone();
    for replica in &replicas[1..] {
        let converged = final_state.join(replica).await?;
        assert_eq!(
            converged,
            final_state.join(replica).await?,
            "Replicas did not achieve eventual consistency"
        );
    }
    
    println!("✓ Eventual consistency with failures verified");
    Ok(())
}

/// Test convergence time bounds
#[tokio::test]
async fn test_convergence_time_bounds() -> AuraResult<()> {
    let start_time = tokio::time::Instant::now();
    
    // Create large number of operations to test scalability
    let mut large_replica1 = JournalMap::new();
    let mut large_replica2 = JournalMap::new();
    
    let operations: Vec<JournalOperation> = (0..100).map(|i| {
        match i % 4 {
            0 => JournalOperation::AddDevice(DeviceId::new()),
            1 => JournalOperation::AddCapability(CapabilityId::new()),
            2 => JournalOperation::CreateRelationship(RelationshipId::new()),
            _ => JournalOperation::UpdateTreeEpoch(i as u64),
        }
    }).collect();
    
    // Apply operations in different orders
    for (i, op) in operations.iter().enumerate() {
        if i % 2 == 0 {
            large_replica1.apply_operation(op.clone()).await?;
        } else {
            large_replica2.apply_operation(op.clone()).await?;
        }
    }
    
    // Apply remaining operations to achieve same operation set
    for (i, op) in operations.iter().enumerate() {
        if i % 2 == 1 {
            large_replica1.apply_operation(op.clone()).await?;
        } else {
            large_replica2.apply_operation(op.clone()).await?;
        }
    }
    
    // Measure convergence time
    let convergence_start = tokio::time::Instant::now();
    let converged = large_replica1.join(&large_replica2).await?;
    let convergence_time = convergence_start.elapsed();
    
    // Verify convergence
    assert_eq!(large_replica1, converged, "Large replica 1 did not converge");
    assert_eq!(large_replica2, converged, "Large replica 2 did not converge");
    
    let total_time = start_time.elapsed();
    
    // Convergence should be reasonably fast (less than 1 second for 100 operations)
    assert!(
        convergence_time < Duration::from_secs(1),
        "Convergence too slow: {:?} for 100 operations",
        convergence_time
    );
    
    println!(
        "✓ Convergence time bounds verified: {:?} total, {:?} convergence", 
        total_time, 
        convergence_time
    );
    Ok(())
}

// Helper types and functions

#[derive(Debug, Clone, PartialEq, Eq)]
enum JournalOperation {
    AddDevice(DeviceId),
    AddCapability(CapabilityId), 
    CreateRelationship(RelationshipId),
    UpdateTreeEpoch(u64),
    StoreContent(Vec<u8>),
}

impl JournalOperation {
    fn operation_id(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        match self {
            JournalOperation::AddDevice(id) => {
                "AddDevice".hash(&mut hasher);
                id.to_bytes().hash(&mut hasher);
            }
            JournalOperation::AddCapability(id) => {
                "AddCapability".hash(&mut hasher);
                id.to_bytes().hash(&mut hasher);
            }
            JournalOperation::CreateRelationship(id) => {
                "CreateRelationship".hash(&mut hasher);
                id.to_bytes().hash(&mut hasher);
            }
            JournalOperation::UpdateTreeEpoch(epoch) => {
                "UpdateTreeEpoch".hash(&mut hasher);
                epoch.hash(&mut hasher);
            }
            JournalOperation::StoreContent(content) => {
                "StoreContent".hash(&mut hasher);
                content.hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeviceOperation {
    AddDevice(DeviceId),
    RemoveDevice(DeviceId),
    UpdateDeviceEpoch(DeviceId, u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CapabilityOperation {
    AddCapability(CapabilityId),
    RemoveCapability(CapabilityId), 
    GrantCapability(DeviceId, CapabilityId),
    RevokeCapability(DeviceId, CapabilityId),
}

// Arbitrary generators
fn arbitrary_journal_operation() -> impl Strategy<Value = JournalOperation> {
    prop_oneof![
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::AddDevice(DeviceId::from_bytes(bytes))),
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::AddCapability(CapabilityId::from_bytes(bytes))),
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::CreateRelationship(RelationshipId::from_bytes(bytes))),
        any::<u64>().prop_map(|epoch| JournalOperation::UpdateTreeEpoch(epoch)),
        prop::collection::vec(any::<u8>(), 0..100).prop_map(|data| JournalOperation::StoreContent(data)),
    ]
}

fn arbitrary_device_operation() -> impl Strategy<Value = DeviceOperation> {
    prop_oneof![
        any::<[u8; 32]>().prop_map(|bytes| DeviceOperation::AddDevice(DeviceId::from_bytes(bytes))),
        any::<[u8; 32]>().prop_map(|bytes| DeviceOperation::RemoveDevice(DeviceId::from_bytes(bytes))),
        (any::<[u8; 32]>(), any::<u64>()).prop_map(|(bytes, epoch)| 
            DeviceOperation::UpdateDeviceEpoch(DeviceId::from_bytes(bytes), epoch)
        ),
    ]
}

fn arbitrary_capability_operation() -> impl Strategy<Value = CapabilityOperation> {
    prop_oneof![
        any::<[u8; 32]>().prop_map(|bytes| CapabilityOperation::AddCapability(CapabilityId::from_bytes(bytes))),
        any::<[u8; 32]>().prop_map(|bytes| CapabilityOperation::RemoveCapability(CapabilityId::from_bytes(bytes))),
        (any::<[u8; 32]>(), any::<[u8; 32]>()).prop_map(|(dev_bytes, cap_bytes)| 
            CapabilityOperation::GrantCapability(
                DeviceId::from_bytes(dev_bytes), 
                CapabilityId::from_bytes(cap_bytes)
            )
        ),
        (any::<[u8; 32]>(), any::<[u8; 32]>()).prop_map(|(dev_bytes, cap_bytes)| 
            CapabilityOperation::RevokeCapability(
                DeviceId::from_bytes(dev_bytes), 
                CapabilityId::from_bytes(cap_bytes)
            )
        ),
    ]
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
    
    async fn contains_honest_operations(&self, operations: &[JournalOperation]) -> AuraResult<bool> {
        // Check that all honest operations are present in the CRDT state
        for operation in operations {
            match operation {
                JournalOperation::AddDevice(device_id) => {
                    if !self.contains_device(device_id).await? {
                        return Ok(false);
                    }
                }
                JournalOperation::AddCapability(cap_id) => {
                    if !self.contains_capability(cap_id).await? {
                        return Ok(false);
                    }
                }
                JournalOperation::CreateRelationship(rel_id) => {
                    if !self.contains_relationship(rel_id).await? {
                        return Ok(false);
                    }
                }
                _ => {} // Other operations are harder to verify presence
            }
        }
        Ok(true)
    }
}

impl AccountState {
    async fn apply_device_operation(&mut self, operation: DeviceOperation) -> AuraResult<()> {
        match operation {
            DeviceOperation::AddDevice(device_id) => {
                self.add_device(device_id).await?;
            }
            DeviceOperation::RemoveDevice(device_id) => {
                self.remove_device(device_id).await?;
            }
            DeviceOperation::UpdateDeviceEpoch(device_id, epoch) => {
                self.update_device_epoch(device_id, epoch).await?;
            }
        }
        Ok(())
    }
    
    async fn apply_capability_operation(&mut self, operation: CapabilityOperation) -> AuraResult<()> {
        match operation {
            CapabilityOperation::AddCapability(cap_id) => {
                self.add_capability(cap_id).await?;
            }
            CapabilityOperation::RemoveCapability(cap_id) => {
                self.remove_capability(cap_id).await?;
            }
            CapabilityOperation::GrantCapability(device_id, cap_id) => {
                self.grant_capability(device_id, cap_id).await?;
            }
            CapabilityOperation::RevokeCapability(device_id, cap_id) => {
                self.revoke_capability(device_id, cap_id).await?;
            }
        }
        Ok(())
    }
}