//! Anti-Entropy Choreography for State Synchronization
//!
//! This module implements choreographic protocols for digest-based
//! state reconciliation using the protocol guide design principles.
//!
//! ## Protocol Flow
//!
//! 1. Requester → Responder: Send digest request
//! 2. Responder → Requester: Send state digest (bloom filter)
//! 3. Requester → Responder: Request missing operations
//! 4. Responder → Requester: Send missing operations
//!
//! ## CRDT Integration
//!
//! This protocol integrates with all four CRDT handlers:
//! - CvHandler: Full state synchronization for convergent CRDTs
//! - CmHandler: Operation-based synchronization with causal ordering
//! - DeltaHandler: Delta-based synchronization for bandwidth optimization
//! - MvHandler: Meet-semilattice synchronization for constraint-based CRDTs

use crate::crate::effects::ChoreographyError;
use crate::effects::{
    ConsoleEffects, CryptoEffects, RandomEffects,
    semilattice::{CmHandler, CvHandler, DeltaHandler, MvHandler},
};
use crate::guards::{JournalCoupler, JournalCouplerBuilder, ProtocolGuard};
use aura_core::{
    semilattice::{CausalOp, CvState, DeltaState, MvState, OpWithCtx},
    DeviceId, SessionId, CausalContext, VectorClock, Journal,
};
use aura_mpst::journal_coupling::{JournalAnnotation, JournalOpType};
use aura_wot::Capability;
use rumpsteak_choreography::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Anti-entropy choreography configuration
#[derive(Debug, Clone)]
pub struct AntiEntropyConfig {
    pub participants: Vec<DeviceId>,
    pub max_ops_per_sync: usize,
}

/// Anti-entropy choreography result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyResult {
    pub ops_sent: usize,
    pub ops_received: usize,
    pub success: bool,
}

/// Anti-entropy error types
#[derive(Debug, thiserror::Error)]
pub enum AntiEntropyError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Sync failed: {0}")]
    SyncFailed(String),
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
}

/// Message types for anti-entropy choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestRequest {
    pub session_id: SessionId,
    pub requester_id: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestResponse {
    pub session_id: SessionId,
    pub bloom_filter: Vec<u8>,
    pub operation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingOpsRequest {
    pub session_id: SessionId,
    pub missing_cids: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingOpsResponse {
    pub session_id: SessionId,
    pub operations: Vec<Vec<u8>>,
    pub ops_count: usize,
}

/// CRDT synchronization request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtSyncRequest {
    pub session_id: SessionId,
    pub crdt_type: CrdtType,
    pub vector_clock: Vec<u8>, // Serialized VectorClock
}

/// CRDT synchronization response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtSyncResponse {
    pub session_id: SessionId,
    pub crdt_type: CrdtType,
    pub sync_data: CrdtSyncData,
}

/// Types of CRDT synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrdtType {
    /// Convergent (state-based) CRDT
    Convergent,
    /// Commutative (operation-based) CRDT
    Commutative,
    /// Delta-based CRDT
    Delta,
    /// Meet-semilattice CRDT
    Meet,
}

/// CRDT synchronization data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrdtSyncData {
    /// Full state for convergent CRDTs
    FullState(Vec<u8>),
    /// Operations for commutative CRDTs
    Operations(Vec<CrdtOperation>),
    /// Deltas for delta-based CRDTs
    Deltas(Vec<Vec<u8>>),
    /// Constraint updates for meet-semilattice CRDTs
    Constraints(Vec<u8>),
}

/// CRDT operation with causal context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtOperation {
    pub operation_id: Vec<u8>,
    pub operation_data: Vec<u8>,
    pub causal_context: Vec<u8>, // Serialized CausalContext
}

/// Anti-entropy synchronization choreography
///
/// Multi-phase protocol for digest-based state reconciliation and CRDT synchronization
choreography! {
    protocol AntiEntropy {
        roles: Requester, Responder;

        // Phase 1: Request digest
        Requester -> Responder: RequestDigest(DigestRequest);

        // Phase 2: Send digest
        Responder -> Requester: SendDigest(DigestResponse);

        // Phase 3: Request missing operations
        Requester -> Responder: RequestMissingOps(MissingOpsRequest);

        // Phase 4: Send missing operations
        Responder -> Requester: SendMissingOps(MissingOpsResponse);

        // Phase 5: CRDT synchronization request
        Requester -> Responder: RequestCrdtSync(CrdtSyncRequest);

        // Phase 6: CRDT synchronization response
        Responder -> Requester: SendCrdtSync(CrdtSyncResponse);
    }
}

/// Execute anti-entropy sync protocol with CRDT integration
pub async fn execute_anti_entropy<CvS, CmS, DeltaS, MvS, Op, Id>(
    device_id: DeviceId,
    config: AntiEntropyConfig,
    is_requester: bool,
    effect_system: &crate::effects::system::AuraEffectSystem,
    mut crdt_coordinator: crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>,
) -> Result<(AntiEntropyResult, crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>), AntiEntropyError>
where
    CvS: aura_core::semilattice::CvState + Serialize + serde::de::DeserializeOwned + 'static,
    CmS: aura_core::semilattice::CmApply<Op> + aura_core::semilattice::Dedup<Id> + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS: aura_core::semilattice::CvState + aura_core::semilattice::DeltaState + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + serde::de::DeserializeOwned,
    MvS: aura_core::semilattice::MvState + Serialize + serde::de::DeserializeOwned + 'static,
    Op: aura_core::semilattice::CausalOp<Id = Id, Ctx = CausalContext> + Serialize + serde::de::DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + serde::de::DeserializeOwned,
{
    // Validate configuration
    if config.participants.len() != 2 {
        return Err(AntiEntropyError::InvalidConfig(format!(
            "Anti-entropy requires exactly 2 participants, got {}",
            config.participants.len()
        )));
    }

    // Create handler adapter
    let mut adapter = crate::choreography::AuraHandlerAdapter::new(
        device_id,
        effect_system.execution_mode(),
    );

    // Execute appropriate role
    let result = if is_requester {
        let responder_id = config
            .participants
            .iter()
            .find(|&&id| id != device_id)
            .copied()
            .ok_or_else(|| AntiEntropyError::InvalidConfig("No responder found".to_string()))?;

        requester_session_with_crdt(&mut adapter, responder_id, &config, &mut crdt_coordinator).await
    } else {
        let requester_id = config
            .participants
            .iter()
            .find(|&&id| id != device_id)
            .copied()
            .ok_or_else(|| AntiEntropyError::InvalidConfig("No requester found".to_string()))?;

        responder_session_with_crdt(&mut adapter, requester_id, &config, &mut crdt_coordinator).await
    }?;

    Ok((result, crdt_coordinator))
}

/// Execute anti-entropy with complete guard chain (CapGuard → FlowGuard → JournalCoupler)
///
/// This function demonstrates the complete integration of the guard chain with
/// choreographic protocol execution, including journal coupling for CRDT updates.
pub async fn execute_anti_entropy_with_guard_chain<CvS, CmS, DeltaS, MvS, Op, Id>(
    device_id: DeviceId,
    config: AntiEntropyConfig,
    is_requester: bool,
    effect_system: &crate::effects::system::AuraEffectSystem,
    mut crdt_coordinator: crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>,
) -> Result<(AntiEntropyResult, crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>), AntiEntropyError>
where
    CvS: aura_core::semilattice::CvState + Serialize + serde::de::DeserializeOwned + 'static,
    CmS: aura_core::semilattice::CmApply<Op> + aura_core::semilattice::Dedup<Id> + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS: aura_core::semilattice::CvState + aura_core::semilattice::DeltaState + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + serde::de::DeserializeOwned,
    MvS: aura_core::semilattice::MvState + Serialize + serde::de::DeserializeOwned + 'static,
    Op: aura_core::semilattice::CausalOp<Id = Id, Ctx = CausalContext> + Serialize + serde::de::DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + serde::de::DeserializeOwned,
{
    // Validate configuration
    if config.participants.len() != 2 {
        return Err(AntiEntropyError::InvalidConfig(format!(
            "Anti-entropy requires exactly 2 participants, got {}",
            config.participants.len()
        )));
    }

    // Create handler adapter
    let mut adapter = crate::choreography::AuraHandlerAdapter::new(
        device_id,
        effect_system.execution_mode(),
    );

    // Create complete guard chain for anti-entropy operations
    let guard_chain = create_anti_entropy_guard_chain(&config);
    let journal_coupler = create_anti_entropy_journal_coupler();

    // Execute with complete guard chain integration
    let result = if is_requester {
        let responder_id = config
            .participants
            .iter()
            .find(|&&id| id != device_id)
            .copied()
            .ok_or_else(|| AntiEntropyError::InvalidConfig("No responder found".to_string()))?;

        execute_requester_with_guards(
            &mut adapter,
            responder_id,
            &config,
            &mut crdt_coordinator,
            &guard_chain,
            &journal_coupler,
        )
        .await
    } else {
        let requester_id = config
            .participants
            .iter()
            .find(|&&id| id != device_id)
            .copied()
            .ok_or_else(|| AntiEntropyError::InvalidConfig("No requester found".to_string()))?;

        execute_responder_with_guards(
            &mut adapter,
            requester_id,
            &config,
            &mut crdt_coordinator,
            &guard_chain,
            &journal_coupler,
        )
        .await
    }?;

    Ok((result, crdt_coordinator))
}

/// Create guard chain for anti-entropy operations
fn create_anti_entropy_guard_chain(config: &AntiEntropyConfig) -> ProtocolGuard {
    ProtocolGuard::new("anti_entropy_sync")
        .require_capabilities(vec![
            Capability::Execute {
                operation: "sync_state".to_string(),
            },
            Capability::Send {
                message_type: "sync_request".to_string(),
            },
            Capability::Receive {
                message_type: "sync_response".to_string(),
            },
        ])
        .delta_facts(vec![
            serde_json::json!({
                "type": "session_attestation",
                "session_id": "anti_entropy_session",
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                "participants": config.participants,
                "operation": "anti_entropy_sync"
            }),
            serde_json::json!({
                "type": "capability_grant",
                "capability": "sync_coordination",
                "target_device": "self",
                "expiry": null,
                "granted_for": "anti_entropy_protocol"
            }),
        ])
        .leakage_budget(crate::guards::LeakageBudget::new(
            2, // External adversary: minimal leakage (sync timing)
            1, // Neighbor adversary: participant information  
            0, // In-group adversary: no additional leakage
        ))
}

/// Create journal coupler for anti-entropy operations
fn create_anti_entropy_journal_coupler() -> JournalCoupler {
    JournalCouplerBuilder::new()
        .optimistic() // Use optimistic application for better performance
        .max_retries(3)
        .with_annotation(
            "sync_initiation".to_string(),
            JournalAnnotation::add_facts("Record anti-entropy sync initiation"),
        )
        .with_annotation(
            "sync_completion".to_string(),
            JournalAnnotation::add_facts("Record anti-entropy sync completion"),
        )
        .with_annotation(
            "crdt_state_update".to_string(),
            JournalAnnotation::merge("Update CRDT state after synchronization"),
        )
        .build()
}

/// Execute requester role with complete guard chain
async fn execute_requester_with_guards<CvS, CmS, DeltaS, MvS, Op, Id>(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    responder_id: DeviceId,
    config: &AntiEntropyConfig,
    crdt_coordinator: &mut crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>,
    guard_chain: &ProtocolGuard,
    journal_coupler: &JournalCoupler,
) -> Result<AntiEntropyResult, AntiEntropyError>
where
    CvS: aura_core::semilattice::CvState + Serialize + serde::de::DeserializeOwned + 'static,
    CmS: aura_core::semilattice::CmApply<Op> + aura_core::semilattice::Dedup<Id> + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS: aura_core::semilattice::CvState + aura_core::semilattice::DeltaState + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + serde::de::DeserializeOwned,
    MvS: aura_core::semilattice::MvState + Serialize + serde::de::DeserializeOwned + 'static,
    Op: aura_core::semilattice::CausalOp<Id = Id, Ctx = CausalContext> + Serialize + serde::de::DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + serde::de::DeserializeOwned,
{
    // Execute the requester logic with full guard chain protection
    let coupling_result = guard_chain
        .execute_with_journal_coupling(
            adapter.effects_mut(),
            journal_coupler,
            |effects| async move {
                // Execute the core anti-entropy protocol logic
                requester_session_with_crdt(adapter, responder_id, config, crdt_coordinator).await
            },
        )
        .await
        .map_err(|e| AntiEntropyError::Handler(crate::handlers::AuraHandlerError::ContextError {
            message: format!("Guard chain execution failed: {}", e),
        }))?;

    Ok(coupling_result.result)
}

/// Execute responder role with complete guard chain
async fn execute_responder_with_guards<CvS, CmS, DeltaS, MvS, Op, Id>(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    requester_id: DeviceId,
    config: &AntiEntropyConfig,
    crdt_coordinator: &mut crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>,
    guard_chain: &ProtocolGuard,
    journal_coupler: &JournalCoupler,
) -> Result<AntiEntropyResult, AntiEntropyError>
where
    CvS: aura_core::semilattice::CvState + Serialize + serde::de::DeserializeOwned + 'static,
    CmS: aura_core::semilattice::CmApply<Op> + aura_core::semilattice::Dedup<Id> + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS: aura_core::semilattice::CvState + aura_core::semilattice::DeltaState + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + serde::de::DeserializeOwned,
    MvS: aura_core::semilattice::MvState + Serialize + serde::de::DeserializeOwned + 'static,
    Op: aura_core::semilattice::CausalOp<Id = Id, Ctx = CausalContext> + Serialize + serde::de::DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + serde::de::DeserializeOwned,
{
    // Execute the responder logic with full guard chain protection
    let coupling_result = guard_chain
        .execute_with_journal_coupling(
            adapter.effects_mut(),
            journal_coupler,
            |effects| async move {
                // Execute the core anti-entropy protocol logic
                responder_session_with_crdt(adapter, requester_id, config, crdt_coordinator).await
            },
        )
        .await
        .map_err(|e| AntiEntropyError::Handler(crate::handlers::AuraHandlerError::ContextError {
            message: format!("Guard chain execution failed: {}", e),
        }))?;

    Ok(coupling_result.result)
}

/// Requester's role in anti-entropy sync with CRDT integration
async fn requester_session_with_crdt<CvS, CmS, DeltaS, MvS, Op, Id>(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    responder_id: DeviceId,
    config: &AntiEntropyConfig,
    crdt_coordinator: &mut crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>,
) -> Result<AntiEntropyResult, AntiEntropyError>
where
    CvS: aura_core::semilattice::CvState + Serialize + serde::de::DeserializeOwned + 'static,
    CmS: aura_core::semilattice::CmApply<Op> + aura_core::semilattice::Dedup<Id> + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS: aura_core::semilattice::CvState + aura_core::semilattice::DeltaState + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + serde::de::DeserializeOwned,
    MvS: aura_core::semilattice::MvState + Serialize + serde::de::DeserializeOwned + 'static,
    Op: aura_core::semilattice::CausalOp<Id = Id, Ctx = CausalContext> + Serialize + serde::de::DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + serde::de::DeserializeOwned,
{
    let session_id = SessionId::new();

    // Phase 1: Request digest
    let digest_request = DigestRequest {
        session_id: session_id.clone(),
        requester_id: adapter.device_id(),
    };

    adapter
        .send(responder_id, digest_request)
        .await
        .map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send digest request: {}", e))
        })?;

    // Phase 2: Receive digest
    let digest_response: DigestResponse = adapter
        .recv_from(responder_id)
        .await
        .map_err(|e| AntiEntropyError::Communication(format!("Failed to receive digest: {}", e)))?;

    if digest_response.session_id != session_id {
        return Err(AntiEntropyError::SyncFailed(
            "Session ID mismatch".to_string(),
        ));
    }

    // Phase 3: Calculate missing operations (TODO fix - Simplified - use hash of bloom filter)
    let local_bloom = adapter.effects().random_bytes(32).await;
    let combined = [&local_bloom[..], &digest_response.bloom_filter[..]].concat();
    let diff_hash = adapter.effects().hash(&combined).await;

    // Simulate missing CIDs based on difference
    let missing_cids = vec![diff_hash.to_vec()];

    let missing_request = MissingOpsRequest {
        session_id: session_id.clone(),
        missing_cids,
    };

    adapter
        .send(responder_id, missing_request)
        .await
        .map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send missing ops request: {}", e))
        })?;

    // Phase 4: Receive missing operations
    let missing_response: MissingOpsResponse =
        adapter.recv_from(responder_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive missing ops: {}", e))
        })?;

    // Phase 5: CRDT synchronization - request for each supported CRDT type
    let mut total_sync_ops = 0;
    
    // Sync convergent CRDTs
    if crdt_coordinator.has_handler(CrdtType::Convergent) {
        let cv_request = crdt_coordinator
            .create_sync_request(session_id.clone(), CrdtType::Convergent)
            .map_err(|e| AntiEntropyError::SyncFailed(format!("Failed to create CV sync request: {}", e)))?;
        
        adapter.send(responder_id, cv_request).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send CV sync request: {}", e))
        })?;

        let cv_response: CrdtSyncResponse = adapter.recv_from(responder_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive CV sync response: {}", e))
        })?;

        crdt_coordinator.handle_sync_response(cv_response).await.map_err(|e| {
            AntiEntropyError::SyncFailed(format!("Failed to handle CV sync response: {}", e))
        })?;
        total_sync_ops += 1;
    }

    // Sync commutative CRDTs
    if crdt_coordinator.has_handler(CrdtType::Commutative) {
        let cm_request = crdt_coordinator
            .create_sync_request(session_id.clone(), CrdtType::Commutative)
            .map_err(|e| AntiEntropyError::SyncFailed(format!("Failed to create CM sync request: {}", e)))?;
        
        adapter.send(responder_id, cm_request).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send CM sync request: {}", e))
        })?;

        let cm_response: CrdtSyncResponse = adapter.recv_from(responder_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive CM sync response: {}", e))
        })?;

        crdt_coordinator.handle_sync_response(cm_response).await.map_err(|e| {
            AntiEntropyError::SyncFailed(format!("Failed to handle CM sync response: {}", e))
        })?;
        total_sync_ops += 1;
    }

    // Sync delta CRDTs
    if crdt_coordinator.has_handler(CrdtType::Delta) {
        let delta_request = crdt_coordinator
            .create_sync_request(session_id.clone(), CrdtType::Delta)
            .map_err(|e| AntiEntropyError::SyncFailed(format!("Failed to create Delta sync request: {}", e)))?;
        
        adapter.send(responder_id, delta_request).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send Delta sync request: {}", e))
        })?;

        let delta_response: CrdtSyncResponse = adapter.recv_from(responder_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive Delta sync response: {}", e))
        })?;

        crdt_coordinator.handle_sync_response(delta_response).await.map_err(|e| {
            AntiEntropyError::SyncFailed(format!("Failed to handle Delta sync response: {}", e))
        })?;
        total_sync_ops += 1;
    }

    // Sync meet semilattice CRDTs
    if crdt_coordinator.has_handler(CrdtType::Meet) {
        let mv_request = crdt_coordinator
            .create_sync_request(session_id, CrdtType::Meet)
            .map_err(|e| AntiEntropyError::SyncFailed(format!("Failed to create MV sync request: {}", e)))?;
        
        adapter.send(responder_id, mv_request).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send MV sync request: {}", e))
        })?;

        let mv_response: CrdtSyncResponse = adapter.recv_from(responder_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive MV sync response: {}", e))
        })?;

        crdt_coordinator.handle_sync_response(mv_response).await.map_err(|e| {
            AntiEntropyError::SyncFailed(format!("Failed to handle MV sync response: {}", e))
        })?;
        total_sync_ops += 1;
    }

    Ok(AntiEntropyResult {
        ops_sent: total_sync_ops,
        ops_received: missing_response.ops_count + total_sync_ops,
        success: true,
    })
}

/// Responder's role in anti-entropy sync with CRDT integration
async fn responder_session_with_crdt<CvS, CmS, DeltaS, MvS, Op, Id>(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    requester_id: DeviceId,
    _config: &AntiEntropyConfig,
    crdt_coordinator: &mut crate::effects::semilattice::CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>,
) -> Result<AntiEntropyResult, AntiEntropyError>
where
    CvS: aura_core::semilattice::CvState + Serialize + serde::de::DeserializeOwned + 'static,
    CmS: aura_core::semilattice::CmApply<Op> + aura_core::semilattice::Dedup<Id> + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS: aura_core::semilattice::CvState + aura_core::semilattice::DeltaState + Serialize + serde::de::DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + serde::de::DeserializeOwned,
    MvS: aura_core::semilattice::MvState + Serialize + serde::de::DeserializeOwned + 'static,
    Op: aura_core::semilattice::CausalOp<Id = Id, Ctx = CausalContext> + Serialize + serde::de::DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + serde::de::DeserializeOwned,
{
    // Phase 1: Receive digest request
    let digest_request: DigestRequest = adapter.recv_from(requester_id).await.map_err(|e| {
        AntiEntropyError::Communication(format!("Failed to receive digest request: {}", e))
    })?;

    // Phase 2: Generate and send digest
    let bloom_filter = adapter.effects().random_bytes(32).await;

    let digest_response = DigestResponse {
        session_id: digest_request.session_id.clone(),
        bloom_filter: bloom_filter.to_vec(),
        operation_count: 10, // TODO fix - Simplified
    };

    adapter
        .send(requester_id, digest_response)
        .await
        .map_err(|e| AntiEntropyError::Communication(format!("Failed to send digest: {}", e)))?;

    // Phase 3: Receive missing ops request
    let missing_request: MissingOpsRequest =
        adapter.recv_from(requester_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive missing ops request: {}", e))
        })?;

    // Phase 4: Send missing operations
    let operations: Vec<Vec<u8>> = missing_request
        .missing_cids
        .iter()
        .map(|cid| {
            // TODO fix - Simplified: generate operation data based on CID
            let op_data = [cid.as_slice(), b"_operation_data"].concat();
            op_data
        })
        .collect();

    let ops_count = operations.len();

    let missing_response = MissingOpsResponse {
        session_id: missing_request.session_id,
        operations,
        ops_count,
    };

    adapter
        .send(requester_id, missing_response)
        .await
        .map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send missing ops: {}", e))
        })?;

    // Phase 5: Handle CRDT synchronization requests
    let mut total_sync_responses = 0;

    // The responder now waits for and handles CRDT sync requests
    // We handle requests for each CRDT type that we support
    for _ in 0..4 {  // Maximum 4 CRDT types
        match adapter.recv_from::<CrdtSyncRequest>(requester_id).await {
            Ok(sync_request) => {
                let sync_response = crdt_coordinator
                    .handle_sync_request(sync_request)
                    .await
                    .map_err(|e| {
                        AntiEntropyError::SyncFailed(format!("Failed to handle CRDT sync request: {}", e))
                    })?;

                adapter.send(requester_id, sync_response).await.map_err(|e| {
                    AntiEntropyError::Communication(format!("Failed to send CRDT sync response: {}", e))
                })?;

                total_sync_responses += 1;
            }
            Err(_) => {
                // No more sync requests - break the loop
                break;
            }
        }
    }

    Ok(AntiEntropyResult {
        ops_sent: ops_count + total_sync_responses,
        ops_received: 0,
        success: true,
    })
}
