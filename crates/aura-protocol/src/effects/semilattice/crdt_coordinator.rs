//! CRDT Coordinator for Choreographic Protocol Integration
//!
//! This module provides the `CrdtCoordinator` that bridges CRDT handlers
//! with choreographic protocols, enabling distributed state synchronization
//! across all four CRDT types (CvRDT, CmRDT, Delta-CRDT, MvRDT).

use super::{CmHandler, CvHandler, DeltaHandler, MvHandler};
use crate::choreography::protocols::anti_entropy::{
    CrdtOperation, CrdtSyncData, CrdtSyncRequest, CrdtSyncResponse, CrdtType,
};
use aura_core::{
    semilattice::{
        Bottom, CausalOp, CmApply, CvState, Dedup, DeltaState, JoinSemilattice, MvState,
        OpWithCtx,
    },
    CausalContext, DeviceId, SessionId, VectorClock,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{collections::HashMap, marker::PhantomData};

/// Error types for CRDT coordination
#[derive(Debug, thiserror::Error)]
pub enum CrdtCoordinatorError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("CRDT type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        expected: CrdtType,
        actual: CrdtType,
    },
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
    #[error("Handler error: {0}")]
    HandlerError(String),
}

/// CRDT Coordinator managing all four CRDT handler types
///
/// This coordinator enables choreographic protocols to interact with CRDT handlers
/// in a type-safe manner, providing unified synchronization across different
/// CRDT semantics.
pub struct CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>
where
    CvS: CvState + Serialize + DeserializeOwned + 'static,
    CmS: CmApply<Op> + Dedup<Id> + Serialize + DeserializeOwned + 'static,
    DeltaS: CvState + DeltaState + Serialize + DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + DeserializeOwned,
    MvS: MvState + Serialize + DeserializeOwned + 'static,
    Op: CausalOp<Id = Id, Ctx = CausalContext> + Serialize + DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + DeserializeOwned,
{
    /// Convergent (state-based) CRDT handler
    cv_handler: Option<CvHandler<CvS>>,
    /// Commutative (operation-based) CRDT handler
    cm_handler: Option<CmHandler<CmS, Op, Id>>,
    /// Delta-based CRDT handler
    delta_handler: Option<DeltaHandler<DeltaS, DeltaS::Delta>>,
    /// Meet-semilattice CRDT handler
    mv_handler: Option<MvHandler<MvS>>,
    /// Device identifier for this coordinator
    device_id: DeviceId,
    /// Current vector clock for causal ordering
    vector_clock: VectorClock,
    /// Type markers
    _phantom: PhantomData<(CvS, CmS, DeltaS, MvS, Op, Id)>,
}

impl<CvS, CmS, DeltaS, MvS, Op, Id> CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>
where
    CvS: CvState + Serialize + DeserializeOwned + 'static,
    CmS: CmApply<Op> + Dedup<Id> + Serialize + DeserializeOwned + 'static,
    DeltaS: CvState + DeltaState + Serialize + DeserializeOwned + 'static,
    DeltaS::Delta: Serialize + DeserializeOwned,
    MvS: MvState + Serialize + DeserializeOwned + 'static,
    Op: CausalOp<Id = Id, Ctx = CausalContext> + Serialize + DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + DeserializeOwned,
{
    /// Create a new CRDT coordinator
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            cv_handler: None,
            cm_handler: None,
            delta_handler: None,
            mv_handler: None,
            device_id,
            vector_clock: VectorClock::new(),
            _phantom: PhantomData,
        }
    }

    /// Register a convergent CRDT handler
    pub fn with_cv_handler(mut self, handler: CvHandler<CvS>) -> Self {
        self.cv_handler = Some(handler);
        self
    }

    /// Register a commutative CRDT handler
    pub fn with_cm_handler(mut self, handler: CmHandler<CmS, Op, Id>) -> Self {
        self.cm_handler = Some(handler);
        self
    }

    /// Register a delta CRDT handler
    pub fn with_delta_handler(mut self, handler: DeltaHandler<DeltaS, DeltaS::Delta>) -> Self {
        self.delta_handler = Some(handler);
        self
    }

    /// Register a meet semilattice CRDT handler
    pub fn with_mv_handler(mut self, handler: MvHandler<MvS>) -> Self {
        self.mv_handler = Some(handler);
        self
    }

    /// Process a CRDT synchronization request
    ///
    /// This method handles incoming sync requests from choreographic protocols
    /// and delegates to the appropriate CRDT handler based on the request type.
    pub async fn handle_sync_request(
        &mut self,
        request: CrdtSyncRequest,
    ) -> Result<CrdtSyncResponse, CrdtCoordinatorError> {
        // Update vector clock from request
        let peer_clock = self.deserialize_vector_clock(&request.vector_clock)?;
        self.vector_clock.update(&peer_clock);

        let sync_data = match request.crdt_type {
            CrdtType::Convergent => {
                let handler = self.cv_handler.as_ref().ok_or_else(|| {
                    CrdtCoordinatorError::UnsupportedOperation(
                        "CvRDT handler not registered".to_string(),
                    )
                })?;
                let state_bytes = self.serialize_state(handler.get_state())?;
                CrdtSyncData::FullState(state_bytes)
            }
            CrdtType::Commutative => {
                let handler = self.cm_handler.as_ref().ok_or_else(|| {
                    CrdtCoordinatorError::UnsupportedOperation(
                        "CmRDT handler not registered".to_string(),
                    )
                })?;
                // For CmRDT, we need to provide buffered operations
                // This is a simplified implementation - in practice, we'd track
                // operations since the peer's vector clock
                let operations = Vec::new(); // TODO: Implement operation history tracking
                CrdtSyncData::Operations(operations)
            }
            CrdtType::Delta => {
                let handler = self.delta_handler.as_ref().ok_or_else(|| {
                    CrdtCoordinatorError::UnsupportedOperation(
                        "Delta CRDT handler not registered".to_string(),
                    )
                })?;
                // Provide available deltas
                let delta_bytes = Vec::new(); // TODO: Serialize buffered deltas
                CrdtSyncData::Deltas(vec![delta_bytes])
            }
            CrdtType::Meet => {
                let handler = self.mv_handler.as_ref().ok_or_else(|| {
                    CrdtCoordinatorError::UnsupportedOperation(
                        "MvRDT handler not registered".to_string(),
                    )
                })?;
                let state_bytes = self.serialize_state(handler.get_state())?;
                CrdtSyncData::Constraints(state_bytes)
            }
        };

        Ok(CrdtSyncResponse {
            session_id: request.session_id,
            crdt_type: request.crdt_type,
            sync_data,
        })
    }

    /// Process a CRDT synchronization response
    ///
    /// This method handles incoming sync responses from choreographic protocols
    /// and applies the received data to the appropriate CRDT handler.
    pub async fn handle_sync_response(
        &mut self,
        response: CrdtSyncResponse,
    ) -> Result<(), CrdtCoordinatorError> {
        match (response.crdt_type, response.sync_data) {
            (CrdtType::Convergent, CrdtSyncData::FullState(state_bytes)) => {
                if let Some(handler) = &mut self.cv_handler {
                    let peer_state: CvS = self.deserialize_state(&state_bytes)?;
                    handler.merge_state(peer_state);
                } else {
                    return Err(CrdtCoordinatorError::UnsupportedOperation(
                        "CvRDT handler not registered".to_string(),
                    ));
                }
            }
            (CrdtType::Commutative, CrdtSyncData::Operations(operations)) => {
                if let Some(handler) = &mut self.cm_handler {
                    for crdt_op in operations {
                        let op: Op = self.deserialize_operation(&crdt_op.operation_data)?;
                        let ctx: CausalContext =
                            self.deserialize_causal_context(&crdt_op.causal_context)?;
                        let op_with_ctx = OpWithCtx::new(op, ctx);
                        handler.on_recv(op_with_ctx);
                    }
                } else {
                    return Err(CrdtCoordinatorError::UnsupportedOperation(
                        "CmRDT handler not registered".to_string(),
                    ));
                }
            }
            (CrdtType::Delta, CrdtSyncData::Deltas(delta_bytes_vec)) => {
                if let Some(handler) = &mut self.delta_handler {
                    for delta_bytes in delta_bytes_vec {
                        let delta: DeltaS::Delta = self.deserialize_delta(&delta_bytes)?;
                        let delta_msg = handler.create_delta_msg(delta);
                        handler.on_recv(delta_msg);
                    }
                } else {
                    return Err(CrdtCoordinatorError::UnsupportedOperation(
                        "Delta CRDT handler not registered".to_string(),
                    ));
                }
            }
            (CrdtType::Meet, CrdtSyncData::Constraints(constraint_bytes)) => {
                if let Some(handler) = &mut self.mv_handler {
                    let peer_state: MvS = self.deserialize_state(&constraint_bytes)?;
                    handler.apply_constraint(peer_state);
                } else {
                    return Err(CrdtCoordinatorError::UnsupportedOperation(
                        "MvRDT handler not registered".to_string(),
                    ));
                }
            }
            (crdt_type, sync_data) => {
                let actual_type = match sync_data {
                    CrdtSyncData::FullState(_) => CrdtType::Convergent,
                    CrdtSyncData::Operations(_) => CrdtType::Commutative,
                    CrdtSyncData::Deltas(_) => CrdtType::Delta,
                    CrdtSyncData::Constraints(_) => CrdtType::Meet,
                };
                return Err(CrdtCoordinatorError::TypeMismatch {
                    expected: crdt_type,
                    actual: actual_type,
                });
            }
        }

        Ok(())
    }

    /// Create a sync request for a specific CRDT type
    pub fn create_sync_request(
        &self,
        session_id: SessionId,
        crdt_type: CrdtType,
    ) -> Result<CrdtSyncRequest, CrdtCoordinatorError> {
        let vector_clock_bytes = self.serialize_vector_clock(&self.vector_clock)?;

        Ok(CrdtSyncRequest {
            session_id,
            crdt_type,
            vector_clock: vector_clock_bytes,
        })
    }

    /// Get current vector clock
    pub fn current_vector_clock(&self) -> &VectorClock {
        &self.vector_clock
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Check if a specific CRDT handler is registered
    pub fn has_handler(&self, crdt_type: CrdtType) -> bool {
        match crdt_type {
            CrdtType::Convergent => self.cv_handler.is_some(),
            CrdtType::Commutative => self.cm_handler.is_some(),
            CrdtType::Delta => self.delta_handler.is_some(),
            CrdtType::Meet => self.mv_handler.is_some(),
        }
    }

    // === Serialization Helpers ===

    fn serialize_state<T: Serialize>(&self, state: &T) -> Result<Vec<u8>, CrdtCoordinatorError> {
        bincode::serialize(state).map_err(|e| {
            CrdtCoordinatorError::Serialization(format!("Failed to serialize state: {}", e))
        })
    }

    fn deserialize_state<T: DeserializeOwned>(
        &self,
        bytes: &[u8],
    ) -> Result<T, CrdtCoordinatorError> {
        bincode::deserialize(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!("Failed to deserialize state: {}", e))
        })
    }

    fn serialize_vector_clock(
        &self,
        clock: &VectorClock,
    ) -> Result<Vec<u8>, CrdtCoordinatorError> {
        bincode::serialize(clock).map_err(|e| {
            CrdtCoordinatorError::Serialization(format!("Failed to serialize vector clock: {}", e))
        })
    }

    fn deserialize_vector_clock(
        &self,
        bytes: &[u8],
    ) -> Result<VectorClock, CrdtCoordinatorError> {
        bincode::deserialize(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!(
                "Failed to deserialize vector clock: {}",
                e
            ))
        })
    }

    fn deserialize_operation(&self, bytes: &[u8]) -> Result<Op, CrdtCoordinatorError> {
        bincode::deserialize(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!("Failed to deserialize operation: {}", e))
        })
    }

    fn deserialize_causal_context(
        &self,
        bytes: &[u8],
    ) -> Result<CausalContext, CrdtCoordinatorError> {
        bincode::deserialize(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!(
                "Failed to deserialize causal context: {}",
                e
            ))
        })
    }

    fn deserialize_delta(&self, bytes: &[u8]) -> Result<DeltaS::Delta, CrdtCoordinatorError> {
        bincode::deserialize(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!("Failed to deserialize delta: {}", e))
        })
    }
}

/// Factory for creating CRDT coordinators with common configurations
pub struct CrdtCoordinatorFactory;

impl CrdtCoordinatorFactory {
    /// Create a coordinator with only convergent CRDT support
    pub fn cv_only<CvS>(
        device_id: DeviceId,
        cv_state: CvS,
    ) -> CrdtCoordinator<CvS, (), (), (), (), ()>
    where
        CvS: CvState + Serialize + DeserializeOwned + 'static,
    {
        CrdtCoordinator::new(device_id).with_cv_handler(CvHandler::with_state(cv_state))
    }

    /// Create a coordinator with only commutative CRDT support
    pub fn cm_only<CmS, Op, Id>(
        device_id: DeviceId,
        cm_state: CmS,
    ) -> CrdtCoordinator<(), CmS, (), (), Op, Id>
    where
        CmS: CmApply<Op> + Dedup<Id> + Serialize + DeserializeOwned + 'static,
        Op: CausalOp<Id = Id, Ctx = CausalContext> + Serialize + DeserializeOwned + Clone,
        Id: Clone + PartialEq + Serialize + DeserializeOwned,
    {
        CrdtCoordinator::new(device_id).with_cm_handler(CmHandler::new(cm_state))
    }

    /// Create a coordinator with full CRDT support
    pub fn full_support<CvS, CmS, DeltaS, MvS, Op, Id>(
        device_id: DeviceId,
    ) -> CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>
    where
        CvS: CvState + Serialize + DeserializeOwned + 'static,
        CmS: CmApply<Op> + Dedup<Id> + Serialize + DeserializeOwned + 'static,
        DeltaS: CvState + DeltaState + Serialize + DeserializeOwned + 'static,
        DeltaS::Delta: Serialize + DeserializeOwned,
        MvS: MvState + Serialize + DeserializeOwned + 'static,
        Op: CausalOp<Id = Id, Ctx = CausalContext> + Serialize + DeserializeOwned + Clone,
        Id: Clone + PartialEq + Serialize + DeserializeOwned,
    {
        CrdtCoordinator::new(device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::semilattice::{Bottom, JoinSemilattice};
    use std::collections::HashSet;

    // Test types for CvRDT
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
    struct TestCounter(u64);

    impl JoinSemilattice for TestCounter {
        fn join(&self, other: &Self) -> Self {
            TestCounter(self.0.max(other.0))
        }
    }

    impl Bottom for TestCounter {
        fn bottom() -> Self {
            TestCounter(0)
        }
    }

    impl CvState for TestCounter {}

    #[tokio::test]
    async fn test_coordinator_creation() {
        let device_id = DeviceId::new();
        let coordinator = CrdtCoordinatorFactory::cv_only(device_id, TestCounter(0));

        assert_eq!(coordinator.device_id(), device_id);
        assert!(coordinator.has_handler(CrdtType::Convergent));
        assert!(!coordinator.has_handler(CrdtType::Commutative));
    }

    #[tokio::test]
    async fn test_sync_request_creation() {
        let device_id = DeviceId::new();
        let coordinator = CrdtCoordinatorFactory::cv_only(device_id, TestCounter(0));
        let session_id = SessionId::new();

        let request = coordinator
            .create_sync_request(session_id, CrdtType::Convergent)
            .unwrap();

        assert_eq!(request.session_id, session_id);
        assert!(matches!(request.crdt_type, CrdtType::Convergent));
    }

    #[tokio::test]
    async fn test_cv_sync_request_handling() {
        let device_id = DeviceId::new();
        let mut coordinator = CrdtCoordinatorFactory::cv_only(device_id, TestCounter(42));
        let session_id = SessionId::new();

        let request = CrdtSyncRequest {
            session_id,
            crdt_type: CrdtType::Convergent,
            vector_clock: bincode::serialize(&VectorClock::new()).unwrap(),
        };

        let response = coordinator.handle_sync_request(request).await.unwrap();

        assert_eq!(response.session_id, session_id);
        assert!(matches!(response.crdt_type, CrdtType::Convergent));
        assert!(matches!(response.sync_data, CrdtSyncData::FullState(_)));
    }
}