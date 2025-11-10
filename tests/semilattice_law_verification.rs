//! Semilattice Law Verification Tests
//!
//! Comprehensive property-based tests to verify that all CRDT implementations
//! in Aura satisfy the mathematical laws of join-semilattices:
//! 1. Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
//! 2. Commutativity: a ⊔ b = b ⊔ a  
//! 3. Idempotency: a ⊔ a = a

use proptest::prelude::*;
use aura_journal::semilattice::{
    JournalMap, AccountState, ConcreteTypes, SemilatticeOps,
    AccountEntry, CapabilitySet, DeviceSet, TreeState, SessionEpoch,
};
use aura_core::{DeviceId, AccountId, RelationshipId, CapabilityId};
use std::collections::{HashMap, HashSet};

/// Property test: JournalMap satisfies semilattice laws
proptest! {
    #[test]
    fn prop_journal_map_semilattice_laws(
        operations1 in prop::collection::vec(arbitrary_journal_operation(), 0..20),
        operations2 in prop::collection::vec(arbitrary_journal_operation(), 0..20),
        operations3 in prop::collection::vec(arbitrary_journal_operation(), 0..20),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            // Create three JournalMap instances from operation sequences
            let mut map_a = JournalMap::new();
            let mut map_b = JournalMap::new();
            let mut map_c = JournalMap::new();
            
            // Apply operations to create different states
            for op in operations1 {
                map_a.apply_operation(op).await.unwrap();
            }
            
            for op in operations2 {
                map_b.apply_operation(op).await.unwrap();
            }
            
            for op in operations3 {
                map_c.apply_operation(op).await.unwrap();
            }
            
            // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
            let ab_join_c = map_a.join(&map_b).await.unwrap().join(&map_c).await.unwrap();
            let a_join_bc = map_a.join(&map_b.join(&map_c).await.unwrap()).await.unwrap();
            
            prop_assert_eq!(
                ab_join_c, 
                a_join_bc,
                "Associativity violated: (a ⊔ b) ⊔ c ≠ a ⊔ (b ⊔ c)"
            );
            
            // Test commutativity: a ⊔ b = b ⊔ a
            let a_join_b = map_a.join(&map_b).await.unwrap();
            let b_join_a = map_b.join(&map_a).await.unwrap();
            
            prop_assert_eq!(
                a_join_b,
                b_join_a,
                "Commutativity violated: a ⊔ b ≠ b ⊔ a"
            );
            
            // Test idempotency: a ⊔ a = a
            let a_join_a = map_a.join(&map_a).await.unwrap();
            
            prop_assert_eq!(
                map_a,
                a_join_a,
                "Idempotency violated: a ⊔ a ≠ a"
            );
            
            // Test monotonicity: a ≤ (a ⊔ b) for any b
            prop_assert!(
                map_a.is_subset_of(&a_join_b).await.unwrap(),
                "Monotonicity violated: a ⊈ (a ⊔ b)"
            );
            
            prop_assert!(
                map_b.is_subset_of(&a_join_b).await.unwrap(),
                "Monotonicity violated: b ⊈ (a ⊔ b)"
            );
        });
    }
}

/// Property test: AccountState satisfies semilattice laws
proptest! {
    #[test]
    fn prop_account_state_semilattice_laws(
        devices1 in prop::collection::vec(arbitrary_device_entry(), 0..10),
        devices2 in prop::collection::vec(arbitrary_device_entry(), 0..10),
        devices3 in prop::collection::vec(arbitrary_device_entry(), 0..10),
        capabilities1 in prop::collection::hash_set(arbitrary_capability_id(), 0..15),
        capabilities2 in prop::collection::hash_set(arbitrary_capability_id(), 0..15),
        capabilities3 in prop::collection::hash_set(arbitrary_capability_id(), 0..15),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            // Create three AccountState instances
            let mut state_a = AccountState::new(AccountId::new());
            let mut state_b = AccountState::new(AccountId::new());
            let mut state_c = AccountState::new(AccountId::new());
            
            // Populate states with different device and capability sets
            for device_entry in devices1 {
                state_a.add_device(device_entry).await.unwrap();
            }
            for cap_id in capabilities1 {
                state_a.add_capability(cap_id).await.unwrap();
            }
            
            for device_entry in devices2 {
                state_b.add_device(device_entry).await.unwrap();
            }
            for cap_id in capabilities2 {
                state_b.add_capability(cap_id).await.unwrap();
            }
            
            for device_entry in devices3 {
                state_c.add_device(device_entry).await.unwrap();
            }
            for cap_id in capabilities3 {
                state_c.add_capability(cap_id).await.unwrap();
            }
            
            // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
            let ab_join_c = state_a.join(&state_b).await.unwrap().join(&state_c).await.unwrap();
            let a_join_bc = state_a.join(&state_b.join(&state_c).await.unwrap()).await.unwrap();
            
            prop_assert_eq!(
                ab_join_c,
                a_join_bc,
                "AccountState associativity violated"
            );
            
            // Test commutativity: a ⊔ b = b ⊔ a
            let a_join_b = state_a.join(&state_b).await.unwrap();
            let b_join_a = state_b.join(&state_a).await.unwrap();
            
            prop_assert_eq!(
                a_join_b,
                b_join_a,
                "AccountState commutativity violated"
            );
            
            // Test idempotency: a ⊔ a = a
            let a_join_a = state_a.join(&state_a).await.unwrap();
            
            prop_assert_eq!(
                state_a,
                a_join_a,
                "AccountState idempotency violated"
            );
        });
    }
}

/// Property test: CapabilitySet satisfies semilattice laws
proptest! {
    #[test]
    fn prop_capability_set_semilattice_laws(
        caps1 in prop::collection::hash_set(arbitrary_capability_id(), 0..20),
        caps2 in prop::collection::hash_set(arbitrary_capability_id(), 0..20),
        caps3 in prop::collection::hash_set(arbitrary_capability_id(), 0..20),
    ) {
        let cap_set_a = CapabilitySet::from_iter(caps1);
        let cap_set_b = CapabilitySet::from_iter(caps2);
        let cap_set_c = CapabilitySet::from_iter(caps3);
        
        // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        let ab_join_c = cap_set_a.join(&cap_set_b).join(&cap_set_c);
        let a_join_bc = cap_set_a.join(&cap_set_b.join(&cap_set_c));
        
        prop_assert_eq!(
            ab_join_c,
            a_join_bc,
            "CapabilitySet associativity violated"
        );
        
        // Test commutativity: a ⊔ b = b ⊔ a
        let a_join_b = cap_set_a.join(&cap_set_b);
        let b_join_a = cap_set_b.join(&cap_set_a);
        
        prop_assert_eq!(
            a_join_b,
            b_join_a,
            "CapabilitySet commutativity violated"
        );
        
        // Test idempotency: a ⊔ a = a
        let a_join_a = cap_set_a.join(&cap_set_a);
        
        prop_assert_eq!(
            cap_set_a,
            a_join_a,
            "CapabilitySet idempotency violated"
        );
        
        // Test absorption: a ⊔ (a ⊓ b) = a (if meet is defined)
        if let Some(a_meet_b) = cap_set_a.meet(&cap_set_b) {
            let a_join_meet = cap_set_a.join(&a_meet_b);
            prop_assert_eq!(
                cap_set_a,
                a_join_meet,
                "CapabilitySet absorption violated"
            );
        }
    }
}

/// Property test: DeviceSet satisfies semilattice laws
proptest! {
    #[test]
    fn prop_device_set_semilattice_laws(
        devices1 in prop::collection::hash_set(arbitrary_device_entry(), 0..15),
        devices2 in prop::collection::hash_set(arbitrary_device_entry(), 0..15),
        devices3 in prop::collection::hash_set(arbitrary_device_entry(), 0..15),
    ) {
        let device_set_a = DeviceSet::from_iter(devices1);
        let device_set_b = DeviceSet::from_iter(devices2);
        let device_set_c = DeviceSet::from_iter(devices3);
        
        // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        let ab_join_c = device_set_a.join(&device_set_b).join(&device_set_c);
        let a_join_bc = device_set_a.join(&device_set_b.join(&device_set_c));
        
        prop_assert_eq!(
            ab_join_c,
            a_join_bc,
            "DeviceSet associativity violated"
        );
        
        // Test commutativity: a ⊔ b = b ⊔ a
        let a_join_b = device_set_a.join(&device_set_b);
        let b_join_a = device_set_b.join(&device_set_a);
        
        prop_assert_eq!(
            a_join_b,
            b_join_a,
            "DeviceSet commutativity violated"
        );
        
        // Test idempotency: a ⊔ a = a
        let a_join_a = device_set_a.join(&device_set_a);
        
        prop_assert_eq!(
            device_set_a,
            a_join_a,
            "DeviceSet idempotency violated"
        );
    }
}

/// Property test: TreeState satisfies semilattice laws
proptest! {
    #[test]
    fn prop_tree_state_semilattice_laws(
        tree_ops1 in prop::collection::vec(arbitrary_tree_operation(), 0..10),
        tree_ops2 in prop::collection::vec(arbitrary_tree_operation(), 0..10),
        tree_ops3 in prop::collection::vec(arbitrary_tree_operation(), 0..10),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            // Create three TreeState instances
            let mut tree_a = TreeState::new();
            let mut tree_b = TreeState::new();
            let mut tree_c = TreeState::new();
            
            // Apply operations to create different tree states
            for op in tree_ops1 {
                tree_a.apply_operation(op).await.unwrap();
            }
            
            for op in tree_ops2 {
                tree_b.apply_operation(op).await.unwrap();
            }
            
            for op in tree_ops3 {
                tree_c.apply_operation(op).await.unwrap();
            }
            
            // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
            let ab_join_c = tree_a.join(&tree_b).await.unwrap().join(&tree_c).await.unwrap();
            let a_join_bc = tree_a.join(&tree_b.join(&tree_c).await.unwrap()).await.unwrap();
            
            prop_assert_eq!(
                ab_join_c,
                a_join_bc,
                "TreeState associativity violated"
            );
            
            // Test commutativity: a ⊔ b = b ⊔ a
            let a_join_b = tree_a.join(&tree_b).await.unwrap();
            let b_join_a = tree_b.join(&tree_a).await.unwrap();
            
            prop_assert_eq!(
                a_join_b,
                b_join_a,
                "TreeState commutativity violated"
            );
            
            // Test idempotency: a ⊔ a = a
            let a_join_a = tree_a.join(&tree_a).await.unwrap();
            
            prop_assert_eq!(
                tree_a,
                a_join_a,
                "TreeState idempotency violated"
            );
        });
    }
}

/// Property test: SessionEpoch satisfies semilattice laws
proptest! {
    #[test]
    fn prop_session_epoch_semilattice_laws(
        epoch_a in 0u64..1000,
        epoch_b in 0u64..1000,
        epoch_c in 0u64..1000,
    ) {
        let session_a = SessionEpoch::new(epoch_a);
        let session_b = SessionEpoch::new(epoch_b);
        let session_c = SessionEpoch::new(epoch_c);
        
        // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        let ab_join_c = session_a.join(&session_b).join(&session_c);
        let a_join_bc = session_a.join(&session_b.join(&session_c));
        
        prop_assert_eq!(
            ab_join_c,
            a_join_bc,
            "SessionEpoch associativity violated"
        );
        
        // Test commutativity: a ⊔ b = b ⊔ a
        let a_join_b = session_a.join(&session_b);
        let b_join_a = session_b.join(&session_a);
        
        prop_assert_eq!(
            a_join_b,
            b_join_a,
            "SessionEpoch commutativity violated"
        );
        
        // Test idempotency: a ⊔ a = a
        let a_join_a = session_a.join(&session_a);
        
        prop_assert_eq!(
            session_a,
            a_join_a,
            "SessionEpoch idempotency violated"
        );
        
        // Test that join produces maximum epoch
        let max_epoch = epoch_a.max(epoch_b);
        let expected_join = SessionEpoch::new(max_epoch);
        
        prop_assert_eq!(
            a_join_b,
            expected_join,
            "SessionEpoch join does not produce maximum: expected {}, got {}",
            max_epoch,
            a_join_b.epoch()
        );
    }
}

/// Test semilattice laws for compound operations
#[tokio::test]
async fn test_compound_semilattice_operations() -> aura_core::AuraResult<()> {
    // Test that multiple sequential joins maintain laws
    let mut map_a = JournalMap::new();
    let mut map_b = JournalMap::new();
    let mut map_c = JournalMap::new();
    let mut map_d = JournalMap::new();
    
    // Apply different operations to each map
    map_a.apply_operation(JournalOperation::AddDevice(DeviceEntry::new(DeviceId::new()))).await?;
    map_b.apply_operation(JournalOperation::AddCapability(CapabilityId::new())).await?;
    map_c.apply_operation(JournalOperation::CreateRelationship(RelationshipId::new())).await?;
    map_d.apply_operation(JournalOperation::UpdateTreeEpoch(42)).await?;
    
    // Test multiple associativity combinations
    // ((a ⊔ b) ⊔ c) ⊔ d = a ⊔ (b ⊔ (c ⊔ d))
    let left_assoc = map_a.join(&map_b).await?
        .join(&map_c).await?
        .join(&map_d).await?;
    
    let right_assoc = map_a.join(
        &map_b.join(
            &map_c.join(&map_d).await?
        ).await?
    ).await?;
    
    assert_eq!(left_assoc, right_assoc, "Multiple associativity violated");
    
    // Test that any permutation of joins produces same result
    let perm1 = map_a.join(&map_b).await?.join(&map_c).await?.join(&map_d).await?;
    let perm2 = map_c.join(&map_a).await?.join(&map_d).await?.join(&map_b).await?;
    let perm3 = map_d.join(&map_c).await?.join(&map_b).await?.join(&map_a).await?;
    
    assert_eq!(perm1, perm2, "Join permutation 1-2 not equal");
    assert_eq!(perm2, perm3, "Join permutation 2-3 not equal");
    assert_eq!(perm1, perm3, "Join permutation 1-3 not equal");
    
    println!("✓ Compound semilattice operations satisfy laws");
    Ok(())
}

/// Test semilattice laws under concurrent operations
#[tokio::test]
async fn test_concurrent_semilattice_laws() -> aura_core::AuraResult<()> {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    
    let map_a = Arc::new(Mutex::new(JournalMap::new()));
    let map_b = Arc::new(Mutex::new(JournalMap::new()));
    
    // Simulate concurrent operations on both maps
    let tasks = (0..20).map(|i| {
        let map_a_clone = Arc::clone(&map_a);
        let map_b_clone = Arc::clone(&map_b);
        
        tokio::spawn(async move {
            let device_id = DeviceId::new();
            let cap_id = CapabilityId::new();
            
            // Apply operations to map_a
            {
                let mut map_a = map_a_clone.lock().await;
                map_a.apply_operation(JournalOperation::AddDevice(DeviceEntry::new(device_id))).await?;
                if i % 2 == 0 {
                    map_a.apply_operation(JournalOperation::AddCapability(cap_id)).await?;
                }
            }
            
            // Apply operations to map_b
            {
                let mut map_b = map_b_clone.lock().await;
                if i % 3 == 0 {
                    map_b.apply_operation(JournalOperation::AddDevice(DeviceEntry::new(device_id))).await?;
                }
                map_b.apply_operation(JournalOperation::AddCapability(cap_id)).await?;
            }
            
            aura_core::AuraResult::Ok(())
        })
    }).collect::<Vec<_>>();
    
    // Wait for all operations to complete
    for task in tasks {
        task.await.unwrap()?;
    }
    
    // Extract final states
    let final_map_a = {
        let map = map_a.lock().await;
        map.clone()
    };
    
    let final_map_b = {
        let map = map_b.lock().await;
        map.clone()
    };
    
    // Test that laws still hold after concurrent operations
    let a_join_b = final_map_a.join(&final_map_b).await?;
    let b_join_a = final_map_b.join(&final_map_a).await?;
    
    assert_eq!(a_join_b, b_join_a, "Commutativity violated after concurrent operations");
    
    let a_join_a = final_map_a.join(&final_map_a).await?;
    assert_eq!(final_map_a, a_join_a, "Idempotency violated after concurrent operations");
    
    println!("✓ Semilattice laws hold under concurrent operations");
    Ok(())
}

/// Test absorption law for lattice structures
#[tokio::test]
async fn test_absorption_laws() -> aura_core::AuraResult<()> {
    // For lattices that have both join and meet, test absorption laws:
    // a ⊔ (a ⊓ b) = a
    // a ⊓ (a ⊔ b) = a
    
    let cap1 = CapabilitySet::from_iter(vec![
        CapabilityId::new(),
        CapabilityId::new(),
    ]);
    
    let cap2 = CapabilitySet::from_iter(vec![
        CapabilityId::new(),
        CapabilityId::new(),
    ]);
    
    if let Some(meet) = cap1.meet(&cap2) {
        let join_absorption = cap1.join(&meet);
        assert_eq!(cap1, join_absorption, "Join absorption law violated: a ⊔ (a ⊓ b) ≠ a");
    }
    
    let join = cap1.join(&cap2);
    if let Some(meet_absorption) = cap1.meet(&join) {
        assert_eq!(cap1, meet_absorption, "Meet absorption law violated: a ⊓ (a ⊔ b) ≠ a");
    }
    
    println!("✓ Absorption laws verified");
    Ok(())
}

/// Test distributivity laws where applicable
#[tokio::test]
async fn test_distributivity_laws() -> aura_core::AuraResult<()> {
    // Test distributivity: a ⊔ (b ⊓ c) = (a ⊔ b) ⊓ (a ⊔ c) for Boolean lattices
    
    let cap_a = CapabilitySet::from_iter(vec![CapabilityId::new()]);
    let cap_b = CapabilitySet::from_iter(vec![CapabilityId::new()]);
    let cap_c = CapabilitySet::from_iter(vec![CapabilityId::new()]);
    
    if let Some(b_meet_c) = cap_b.meet(&cap_c) {
        let left_side = cap_a.join(&b_meet_c);
        
        let a_join_b = cap_a.join(&cap_b);
        let a_join_c = cap_a.join(&cap_c);
        
        if let Some(right_side) = a_join_b.meet(&a_join_c) {
            assert_eq!(
                left_side, 
                right_side,
                "Distributivity violated: a ⊔ (b ⊓ c) ≠ (a ⊔ b) ⊓ (a ⊔ c)"
            );
        }
    }
    
    println!("✓ Distributivity laws verified where applicable");
    Ok(())
}

// Arbitrary generators for property-based testing

fn arbitrary_journal_operation() -> impl Strategy<Value = JournalOperation> {
    prop_oneof![
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::AddDevice(
            DeviceEntry::new(DeviceId::from_bytes(bytes))
        )),
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::AddCapability(
            CapabilityId::from_bytes(bytes)
        )),
        any::<[u8; 32]>().prop_map(|bytes| JournalOperation::CreateRelationship(
            RelationshipId::from_bytes(bytes)
        )),
        any::<u64>().prop_map(|epoch| JournalOperation::UpdateTreeEpoch(epoch)),
        prop::collection::vec(any::<u8>(), 0..100).prop_map(|data| 
            JournalOperation::StoreContent(data)
        ),
    ]
}

fn arbitrary_device_entry() -> impl Strategy<Value = DeviceEntry> {
    any::<[u8; 32]>().prop_map(|bytes| DeviceEntry::new(DeviceId::from_bytes(bytes)))
}

fn arbitrary_capability_id() -> impl Strategy<Value = CapabilityId> {
    any::<[u8; 32]>().prop_map(|bytes| CapabilityId::from_bytes(bytes))
}

fn arbitrary_tree_operation() -> impl Strategy<Value = TreeOperation> {
    prop_oneof![
        (any::<[u8; 32]>(), any::<u64>()).prop_map(|(bytes, epoch)| 
            TreeOperation::AddNode(DeviceId::from_bytes(bytes), epoch)
        ),
        (any::<[u8; 32]>(), any::<u64>()).prop_map(|(bytes, epoch)| 
            TreeOperation::UpdateEpoch(DeviceId::from_bytes(bytes), epoch)
        ),
        any::<[u8; 32]>().prop_map(|bytes| 
            TreeOperation::RemoveNode(DeviceId::from_bytes(bytes))
        ),
        any::<u64>().prop_map(|epoch| TreeOperation::AdvanceEpoch(epoch)),
    ]
}

// Helper types for testing

#[derive(Debug, Clone, PartialEq, Eq)]
enum JournalOperation {
    AddDevice(DeviceEntry),
    AddCapability(CapabilityId),
    CreateRelationship(RelationshipId),
    UpdateTreeEpoch(u64),
    StoreContent(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DeviceEntry {
    device_id: DeviceId,
    added_at: u64,
}

impl DeviceEntry {
    fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeOperation {
    AddNode(DeviceId, u64),
    UpdateEpoch(DeviceId, u64),
    RemoveNode(DeviceId),
    AdvanceEpoch(u64),
}

// Trait implementations for testing CRDT types
impl JournalMap {
    async fn apply_operation(&mut self, operation: JournalOperation) -> aura_core::AuraResult<()> {
        match operation {
            JournalOperation::AddDevice(device_entry) => {
                // Add device to journal
                self.add_device(device_entry.device_id).await?;
            }
            JournalOperation::AddCapability(cap_id) => {
                // Add capability to journal  
                self.add_capability(cap_id).await?;
            }
            JournalOperation::CreateRelationship(rel_id) => {
                // Create relationship in journal
                self.create_relationship(rel_id).await?;
            }
            JournalOperation::UpdateTreeEpoch(epoch) => {
                // Update tree epoch
                self.update_tree_epoch(epoch).await?;
            }
            JournalOperation::StoreContent(data) => {
                // Store content in journal
                self.store_content(data).await?;
            }
        }
        Ok(())
    }
}

impl TreeState {
    async fn apply_operation(&mut self, operation: TreeOperation) -> aura_core::AuraResult<()> {
        match operation {
            TreeOperation::AddNode(device_id, epoch) => {
                self.add_node(device_id, epoch).await?;
            }
            TreeOperation::UpdateEpoch(device_id, epoch) => {
                self.update_node_epoch(device_id, epoch).await?;
            }
            TreeOperation::RemoveNode(device_id) => {
                self.remove_node(device_id).await?;
            }
            TreeOperation::AdvanceEpoch(epoch) => {
                self.advance_epoch(epoch).await?;
            }
        }
        Ok(())
    }
}