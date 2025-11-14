//! Integration tests for CRDT handler and choreographic protocol integration
//!
//! This test suite verifies that CRDT handlers are properly integrated with
//! choreographic protocols, enabling distributed state synchronization across
//! all four CRDT types.

use aura_core::{
    semilattice::{Bottom, CmApply, CvState, Dedup, DeltaState, JoinSemilattice, MeetSemiLattice},
    CausalContext, DeviceId, SessionId, AuraResult,
};
use aura_protocol::{
    choreography::protocols::anti_entropy::{
        execute_anti_entropy, AntiEntropyConfig, CrdtType,
    },
    effects::{
        semilattice::CrdtCoordinator,
    },
};
use aura_macros::aura_test;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// === Test CRDT Types ===

/// Test convergent CRDT (G-Counter)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCounter {
    counts: std::collections::HashMap<DeviceId, u64>,
}

impl TestCounter {
    pub fn new() -> Self {
        Self {
            counts: std::collections::HashMap::new(),
        }
    }

    pub fn increment(&mut self, device: DeviceId) {
        *self.counts.entry(device).or_insert(0) += 1;
    }

    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }
}

impl JoinSemilattice for TestCounter {
    fn join(&self, other: &Self) -> Self {
        let mut counts = self.counts.clone();
        for (device, &count) in &other.counts {
            let entry = counts.entry(*device).or_insert(0);
            *entry = (*entry).max(count);
        }
        Self { counts }
    }
}

impl Bottom for TestCounter {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for TestCounter {}

/// Test commutative CRDT operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncrementOp {
    pub id: u64,
    pub device: DeviceId,
    pub causal_ctx: CausalContext,
}

impl aura_core::semilattice::CausalOp for IncrementOp {
    type Id = u64;
    type Ctx = CausalContext;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn ctx(&self) -> &Self::Ctx {
        &self.causal_ctx
    }
}

/// Test commutative CRDT state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmTestCounter {
    total: u64,
    seen_ops: HashSet<u64>,
}

impl CmTestCounter {
    pub fn new() -> Self {
        Self {
            total: 0,
            seen_ops: HashSet::new(),
        }
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

impl CmApply<IncrementOp> for CmTestCounter {
    fn apply(&mut self, op: IncrementOp) {
        if !self.seen_ops.contains(&op.id) {
            self.total += 1;
        }
    }
}

impl Dedup<u64> for CmTestCounter {
    fn seen(&self, id: &u64) -> bool {
        self.seen_ops.contains(id)
    }

    fn mark_seen(&mut self, id: u64) {
        self.seen_ops.insert(id);
    }
}

/// Test delta CRDT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaTestCounter {
    value: u64,
}

impl DeltaTestCounter {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn value(&self) -> u64 {
        self.value
    }
}

impl JoinSemilattice for DeltaTestCounter {
    fn join(&self, other: &Self) -> Self {
        Self {
            value: self.value.max(other.value),
        }
    }
}

impl Bottom for DeltaTestCounter {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for DeltaTestCounter {}

impl DeltaState for DeltaTestCounter {
    type Delta = DeltaIncrement;

    fn apply_delta(&self, delta: &Self::Delta) -> Self {
        Self {
            value: self.value + delta.increment,
        }
    }
}

/// Delta for test counter
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaIncrement {
    increment: u64,
}

impl aura_core::semilattice::Delta for DeltaIncrement {
    fn join_delta(&self, other: &Self) -> Self {
        Self {
            increment: self.increment.max(other.increment),
        }
    }
}

impl aura_core::semilattice::DeltaProduce<DeltaTestCounter> for DeltaIncrement {
    fn delta_from(old: &DeltaTestCounter, new: &DeltaTestCounter) -> Self {
        Self {
            increment: if new.value > old.value {
                new.value - old.value
            } else {
                0
            },
        }
    }
}

/// Test meet semilattice CRDT (permission set)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet {
    permissions: HashSet<String>,
}

impl PermissionSet {
    pub fn new() -> Self {
        Self {
            permissions: HashSet::new(),
        }
    }

    pub fn with_permissions(permissions: Vec<String>) -> Self {
        Self {
            permissions: permissions.into_iter().collect(),
        }
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }
}

impl MeetSemiLattice for PermissionSet {
    fn meet(&self, other: &Self) -> Self {
        Self {
            permissions: self.permissions.intersection(&other.permissions).cloned().collect(),
        }
    }
}

impl aura_core::semilattice::MvState for PermissionSet {}

// === Integration Tests ===

#[aura_test]
async fn test_cv_crdt_choreography_integration() -> AuraResult<()> {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Create coordinators for both devices using builder pattern
    let mut coordinator_a = CrdtCoordinator::with_cv_state(device_a, TestCounter::new());
    let mut coordinator_b = CrdtCoordinator::with_cv_state(device_b, TestCounter::new());

    // Simulate state changes
    let mut state_a = TestCounter::new();
    state_a.increment(device_a);
    state_a.increment(device_a);

    let mut state_b = TestCounter::new();
    state_b.increment(device_b);

    // Update coordinators with new states
    coordinator_a = coordinator_a.with_cv_handler(
        aura_protocol::effects::semilattice::CvHandler::with_state(state_a.clone())
    );
    coordinator_b = coordinator_b.with_cv_handler(
        aura_protocol::effects::semilattice::CvHandler::with_state(state_b.clone())
    );

    // Create test fixture
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_a).await?;
    let effect_system = fixture.effects();

    // Configure anti-entropy
    let config = AntiEntropyConfig {
        participants: vec![device_a, device_b],
        max_ops_per_sync: 100,
    };

    // Test sync as requester
    let result = execute_anti_entropy(
        device_a,
        config.clone(),
        true,  // is_requester
        &effect_system,
        coordinator_a,
    ).await;

    // Verify successful synchronization
    assert!(result.is_ok(), "CV CRDT synchronization should succeed");
    let (sync_result, updated_coordinator) = result.unwrap();
    assert!(sync_result.success, "Sync should report success");
    assert_eq!(updated_coordinator.device_id(), device_a);
    
    Ok(())
}

#[aura_test]
async fn test_cm_crdt_choreography_integration() -> AuraResult<()> {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Create coordinators for operation-based CRDTs using builder pattern
    let coordinator_a = CrdtCoordinator::with_cm(device_a, CmTestCounter::new());
    let coordinator_b = CrdtCoordinator::with_cm(device_b, CmTestCounter::new());

    // Create test fixture
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_a).await?;
    let effect_system = fixture.effects();

    // Configure anti-entropy
    let config = AntiEntropyConfig {
        participants: vec![device_a, device_b],
        max_ops_per_sync: 100,
    };

    // Test sync as responder
    let result = execute_anti_entropy(
        device_b,
        config,
        false, // is_responder
        &effect_system,
        coordinator_b,
    ).await;

    // Verify successful synchronization
    assert!(result.is_ok(), "CM CRDT synchronization should succeed");
    let (sync_result, _) = result.unwrap();
    assert!(sync_result.success, "Sync should report success");
    
    Ok(())
}

#[aura_test]
async fn test_delta_crdt_choreography_integration() -> AuraResult<()> {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Create delta CRDT coordinators
    let mut coordinator_a = CrdtCoordinator::new(device_a);
    coordinator_a = coordinator_a.with_delta_handler(
        aura_protocol::effects::semilattice::DeltaHandler::with_state(DeltaTestCounter::new())
    );

    let mut coordinator_b = CrdtCoordinator::new(device_b);
    coordinator_b = coordinator_b.with_delta_handler(
        aura_protocol::effects::semilattice::DeltaHandler::with_state(DeltaTestCounter::new())
    );

    // Create test fixture
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_a).await?;
    let effect_system = fixture.effects();

    // Configure anti-entropy
    let config = AntiEntropyConfig {
        participants: vec![device_a, device_b],
        max_ops_per_sync: 100,
    };

    // Test sync
    let result = execute_anti_entropy(
        device_a,
        config,
        true,  // is_requester
        &effect_system,
        coordinator_a,
    ).await;

    // Verify successful synchronization
    assert!(result.is_ok(), "Delta CRDT synchronization should succeed");
    let (sync_result, _) = result.unwrap();
    assert!(sync_result.success, "Sync should report success");
    
    Ok(())
}

#[aura_test]
async fn test_mv_crdt_choreography_integration() -> AuraResult<()> {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Create meet semilattice CRDT coordinators
    let mut coordinator_a = CrdtCoordinator::new(device_a);
    coordinator_a = coordinator_a.with_mv_handler(
        aura_protocol::effects::semilattice::MvHandler::with_state(
            PermissionSet::with_permissions(vec!["read".to_string(), "write".to_string()])
        )
    );

    let mut coordinator_b = CrdtCoordinator::new(device_b);
    coordinator_b = coordinator_b.with_mv_handler(
        aura_protocol::effects::semilattice::MvHandler::with_state(
            PermissionSet::with_permissions(vec!["read".to_string(), "execute".to_string()])
        )
    );

    // Create test fixture
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_a).await?;
    let effect_system = fixture.effects();

    // Configure anti-entropy
    let config = AntiEntropyConfig {
        participants: vec![device_a, device_b],
        max_ops_per_sync: 100,
    };

    // Test sync
    let result = execute_anti_entropy(
        device_b,
        config,
        false, // is_responder
        &effect_system,
        coordinator_b,
    ).await;

    // Verify successful synchronization
    assert!(result.is_ok(), "MV CRDT synchronization should succeed");
    let (sync_result, _) = result.unwrap();
    assert!(sync_result.success, "Sync should report success");
    
    Ok(())
}

#[aura_test]
async fn test_multi_crdt_choreography_integration() -> AuraResult<()> {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Create coordinators with multiple CRDT types
    let mut coordinator_a = CrdtCoordinator::new(device_a)
        .with_cv_handler(
            aura_protocol::effects::semilattice::CvHandler::with_state(TestCounter::new())
        )
        .with_cm_handler(
            aura_protocol::effects::semilattice::CmHandler::new(CmTestCounter::new())
        );

    let mut coordinator_b = CrdtCoordinator::new(device_b)
        .with_cv_handler(
            aura_protocol::effects::semilattice::CvHandler::with_state(TestCounter::new())
        )
        .with_cm_handler(
            aura_protocol::effects::semilattice::CmHandler::new(CmTestCounter::new())
        );

    // Verify multiple handlers are registered
    assert!(coordinator_a.has_handler(CrdtType::Convergent));
    assert!(coordinator_a.has_handler(CrdtType::Commutative));
    assert!(!coordinator_a.has_handler(CrdtType::Delta));
    assert!(!coordinator_a.has_handler(CrdtType::Meet));

    // Create test fixture
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_a).await?;
    let effect_system = fixture.effects();

    // Configure anti-entropy
    let config = AntiEntropyConfig {
        participants: vec![device_a, device_b],
        max_ops_per_sync: 100,
    };

    // Test sync with multiple CRDT types
    let result = execute_anti_entropy(
        device_a,
        config,
        true,  // is_requester
        &effect_system,
        coordinator_a,
    ).await;

    // Verify successful synchronization
    assert!(result.is_ok(), "Multi-CRDT synchronization should succeed");
    let (sync_result, final_coordinator) = result.unwrap();
    assert!(sync_result.success, "Sync should report success");

    // Should sync both CV and CM CRDTs
    assert!(sync_result.ops_sent >= 2, "Should sync at least 2 CRDT types");
    assert_eq!(final_coordinator.device_id(), device_a);
    
    Ok(())
}

#[aura_test]
async fn test_crdt_coordinator_builder_patterns() -> AuraResult<()> {
    let device_id = DeviceId::new();

    // Test CV-only builder
    let cv_coordinator = CrdtCoordinator::with_cv_state(device_id, TestCounter::new());
    assert!(cv_coordinator.has_handler(CrdtType::Convergent));
    assert!(!cv_coordinator.has_handler(CrdtType::Commutative));

    // Test CM-only builder
    let cm_coordinator = CrdtCoordinator::with_cm(device_id, CmTestCounter::new());
    assert!(cm_coordinator.has_handler(CrdtType::Commutative));
    assert!(!cm_coordinator.has_handler(CrdtType::Convergent));

    // Test Delta-only builder
    let delta_coordinator = CrdtCoordinator::with_delta(device_id);
    assert!(delta_coordinator.has_handler(CrdtType::Delta));
    assert!(!delta_coordinator.has_handler(CrdtType::Convergent));

    // Test MV-only builder
    let mv_coordinator = CrdtCoordinator::with_mv_state(device_id, PermissionSet::new());
    assert!(mv_coordinator.has_handler(CrdtType::Meet));
    assert!(!mv_coordinator.has_handler(CrdtType::Convergent));

    // Test chained builder for multiple handlers
    let multi_coordinator = CrdtCoordinator::new(device_id)
        .with_cv_handler(aura_protocol::effects::semilattice::CvHandler::with_state(TestCounter::new()))
        .with_cm_handler(aura_protocol::effects::semilattice::CmHandler::new(CmTestCounter::new()));

    assert!(multi_coordinator.has_handler(CrdtType::Convergent));
    assert!(multi_coordinator.has_handler(CrdtType::Commutative));
    assert!(!multi_coordinator.has_handler(CrdtType::Delta));
    assert!(!multi_coordinator.has_handler(CrdtType::Meet));
    
    Ok(())
}

#[aura_test]
async fn test_sync_request_creation_and_handling() -> AuraResult<()> {
    let device_id = DeviceId::new();
    let session_id = SessionId::new();

    // Create coordinator using builder pattern
    let mut coordinator = CrdtCoordinator::with_cv_state(device_id, TestCounter::new());

    // Test sync request creation
    let request = coordinator.create_sync_request(session_id, CrdtType::Convergent)?;
    assert_eq!(request.session_id, session_id);
    assert!(matches!(request.crdt_type, CrdtType::Convergent));

    // Test sync request handling
    let response = coordinator.handle_sync_request(request).await?;
    assert_eq!(response.session_id, session_id);
    assert!(matches!(response.crdt_type, CrdtType::Convergent));
    assert!(matches!(response.sync_data, aura_protocol::choreography::protocols::anti_entropy::CrdtSyncData::FullState(_)));
    
    Ok(())
}
