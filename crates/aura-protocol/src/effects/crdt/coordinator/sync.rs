//! CRDT Synchronization Request/Response Handling
//!
//! Implements the sync protocol for CRDT coordination.

use super::{
    clock::{increment_actor, merge_vector_clocks},
    error::CrdtCoordinatorError,
    CrdtCoordinator,
};
use crate::choreography::{CrdtSyncData, CrdtSyncRequest, CrdtSyncResponse, CrdtType};
use aura_core::{
    semilattice::{CausalOp, CmApply, CvState, Dedup, Delta, DeltaState, MvState, OpWithCtx, Top},
    SessionId,
};
use aura_journal::CausalContext;
use serde::{de::DeserializeOwned, Serialize};

impl<CvS, CmS, DeltaS, MvS, Op, Id> CrdtCoordinator<CvS, CmS, DeltaS, MvS, Op, Id>
where
    CvS: CvState + Serialize + DeserializeOwned + 'static,
    CmS: CmApply<Op> + Dedup<Id> + Serialize + DeserializeOwned + 'static,
    DeltaS: CvState + DeltaState + Serialize + DeserializeOwned + 'static,
    DeltaS::Delta: Delta + Serialize + DeserializeOwned,
    MvS: MvState + Top + Serialize + DeserializeOwned + 'static,
    Op: CausalOp<Id = Id, Ctx = CausalContext> + Serialize + DeserializeOwned + Clone,
    Id: Clone + PartialEq + Serialize + DeserializeOwned,
{
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
        merge_vector_clocks(&mut self.vector_clock, &peer_clock);
        self.bump_local();

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
                // Buffered operations not exposed by current handler; return empty set.
                let _ = handler;
                CrdtSyncData::Operations(Vec::new())
            }
            CrdtType::Delta => {
                let handler = self.delta_handler.as_ref().ok_or_else(|| {
                    CrdtCoordinatorError::UnsupportedOperation(
                        "Delta CRDT handler not registered".to_string(),
                    )
                })?;
                // Deltas not buffered; return empty list.
                let _ = handler;
                CrdtSyncData::Deltas(Vec::new())
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
                    // Deserialize outside the borrow scope to avoid borrowing conflicts
                    let peer_state: CvS = {
                        let bytes = &state_bytes;
                        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
                            CrdtCoordinatorError::Deserialization(format!(
                                "Failed to deserialize state: {e}"
                            ))
                        })?
                    };
                    handler.update_state(peer_state);
                } else {
                    return Err(CrdtCoordinatorError::UnsupportedOperation(
                        "CvRDT handler not registered".to_string(),
                    ));
                }
            }
            (CrdtType::Commutative, CrdtSyncData::Operations(operations)) => {
                if let Some(handler) = &mut self.cm_handler {
                    // Deserialize all operations first to avoid borrowing conflicts
                    let mut ops_with_ctx = Vec::new();
                    for crdt_op in operations {
                        let op: Op =
                            aura_core::util::serialization::from_slice(&crdt_op.operation_data)
                                .map_err(|e| {
                                    CrdtCoordinatorError::Deserialization(format!(
                                        "Failed to deserialize operation: {e}"
                                    ))
                                })?;
                        let ctx: CausalContext =
                            aura_core::util::serialization::from_slice(&crdt_op.causal_context)
                                .map_err(|e| {
                                    CrdtCoordinatorError::Deserialization(format!(
                                        "Failed to deserialize causal context: {e}"
                                    ))
                                })?;
                        ops_with_ctx.push(OpWithCtx::new(op, ctx));
                    }

                    // Now apply all operations
                    for op_with_ctx in ops_with_ctx {
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
                    // Deserialize all deltas first to avoid borrowing conflicts
                    let mut deltas = Vec::new();
                    for delta_bytes in delta_bytes_vec {
                        let delta: DeltaS::Delta = aura_core::util::serialization::from_slice(
                            &delta_bytes,
                        )
                        .map_err(|e| {
                            CrdtCoordinatorError::Deserialization(format!(
                                "Failed to deserialize delta: {e}"
                            ))
                        })?;
                        deltas.push(delta);
                    }

                    // Now apply all deltas
                    for delta in deltas {
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
                    // Deserialize outside the borrow scope to avoid borrowing conflicts
                    let peer_state: MvS = {
                        let bytes = &constraint_bytes;
                        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
                            CrdtCoordinatorError::Deserialization(format!(
                                "Failed to deserialize state: {e}"
                            ))
                        })?
                    };
                    handler.on_constraint(peer_state);
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
        &mut self,
        session_id: SessionId,
        crdt_type: CrdtType,
    ) -> Result<CrdtSyncRequest, CrdtCoordinatorError> {
        self.bump_local();
        let vector_clock_bytes = self.serialize_vector_clock(&self.vector_clock)?;

        Ok(CrdtSyncRequest {
            session_id,
            crdt_type,
            vector_clock: vector_clock_bytes,
        })
    }

    /// Increment local actor clock if configured.
    pub(super) fn bump_local(&mut self) {
        if let Some(actor) = self.actor {
            increment_actor(&mut self.vector_clock, actor);
        }
    }
}
