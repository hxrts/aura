//! CRDT Coordinator for Choreographic Protocol Integration
//!
//! This module provides the `CrdtCoordinator` that bridges CRDT handlers
//! with choreographic protocols, enabling distributed state synchronization
//! across all four CRDT types (CvRDT, CmRDT, Delta-CRDT, MvRDT).
//!
//! # Builder Pattern
//!
//! The coordinator uses a clean builder pattern for ergonomic setup:
//!
//! ```ignore
//! // Convergent CRDT with default state
//! let coordinator = CrdtCoordinator::with_cv(authority_id);
//!
//! // Convergent CRDT with initial state
//! let coordinator = CrdtCoordinator::with_cv_state(authority_id, my_state);
//!
//! // Commutative CRDT
//! let coordinator = CrdtCoordinator::with_cm(authority_id, initial_state);
//!
//! // Delta CRDT with compaction threshold
//! let coordinator = CrdtCoordinator::with_delta_threshold(authority_id, 100);
//!
//! // Meet-semilattice CRDT
//! let coordinator = CrdtCoordinator::with_mv(authority_id);
//!
//! // RECOMMENDED: Use pre-composed handlers from aura-composition
//! // let factory = HandlerFactory::for_testing(device_id)?;
//! // let registry = factory.create_registry()?;
//! // Extract handlers from registry for coordination
//!
//! // Compose handlers externally and inject them
//! // let coordinator = CrdtCoordinator::new(authority_id)
//! //     .with_cv_handler(precomposed_cv_handler)
//! //     .with_delta_handler(precomposed_delta_handler);
//! ```
//!
//! # Integration with Choreographies
//!
//! Use the coordinator in anti-entropy and other synchronization protocols:
//!
//! ```ignore
//! let coordinator = CrdtCoordinator::with_cv_state(authority_id, journal_state);
//! let result = execute_anti_entropy(
//!     authority_id,
//!     config,
//!     is_requester,
//!     &effect_system,
//!     coordinator,
//! ).await?;
//! ```

use super::{CmHandler, CvHandler, DeltaHandler, MvHandler};
use crate::choreography::{CrdtSyncData, CrdtSyncRequest, CrdtSyncResponse, CrdtType};
use aura_core::{
    identifiers::DeviceId,
    semilattice::{
        Bottom, CausalOp, CmApply, CvState, Dedup, Delta, DeltaState, MvState, OpWithCtx, Top,
    },
    time::{LogicalTime, VectorClock},
    AuraError, AuthorityId, SessionId,
};
use aura_journal::CausalContext;
use serde::{de::DeserializeOwned, Serialize};
use std::marker::PhantomData;

// ============================================================================
// Error Types
// ============================================================================

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

impl From<CrdtCoordinatorError> for AuraError {
    fn from(err: CrdtCoordinatorError) -> Self {
        AuraError::internal(format!("CRDT coordinator error: {}", err))
    }
}

// ============================================================================
// Vector Clock Utilities
// ============================================================================

/// Merge source vector clock into target, taking the maximum for each actor.
pub fn merge_vector_clocks(target: &mut VectorClock, other: &VectorClock) {
    for (actor, time) in other.iter() {
        let current = match target.get(actor).copied() {
            Some(value) => value,
            None => 0,
        };
        if *time > current {
            target.insert(*actor, *time);
        }
    }
}

/// Get the maximum counter value from a vector clock (Lamport time).
pub fn max_counter(clock: &VectorClock) -> u64 {
    match clock.iter().map(|(_, counter)| *counter).max() {
        Some(value) => value,
        None => 0,
    }
}

/// Increment the counter for a specific actor in the vector clock.
pub fn increment_actor(clock: &mut VectorClock, actor: DeviceId) {
    let current = match clock.get(&actor).copied() {
        Some(value) => value,
        None => 0,
    };
    clock.insert(actor, current.saturating_add(1));
}

// ============================================================================
// CRDT Coordinator
// ============================================================================

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
    DeltaS::Delta: Delta + Serialize + DeserializeOwned,
    MvS: MvState + Top + Serialize + DeserializeOwned + 'static,
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
    /// Authority identifier for this coordinator
    authority_id: AuthorityId,
    /// Optional actor/device identifier to advance local logical time
    actor: Option<DeviceId>,
    /// Current vector clock for causal ordering
    vector_clock: VectorClock,
    /// Type markers
    _phantom: PhantomData<(CvS, CmS, DeltaS, MvS, Op, Id)>,
}

#[allow(dead_code)]
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
    // === Core Constructors ===

    /// Create a new CRDT coordinator
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            cv_handler: None,
            cm_handler: None,
            delta_handler: None,
            mv_handler: None,
            authority_id,
            actor: None,
            vector_clock: VectorClock::new(),
            _phantom: PhantomData,
        }
    }

    /// Create a CRDT coordinator with an explicit actor/device id for logical time advancement.
    pub fn new_with_actor(authority_id: AuthorityId, actor: DeviceId) -> Self {
        Self {
            actor: Some(actor),
            ..Self::new(authority_id)
        }
    }

    // === Builder Methods ===

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

    // === Sync Request Handling ===

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
                                "Failed to deserialize state: {}",
                                e
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
                                        "Failed to deserialize operation: {}",
                                        e
                                    ))
                                })?;
                        let ctx: CausalContext =
                            aura_core::util::serialization::from_slice(&crdt_op.causal_context)
                                .map_err(|e| {
                                    CrdtCoordinatorError::Deserialization(format!(
                                        "Failed to deserialize causal context: {}",
                                        e
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
                                "Failed to deserialize delta: {}",
                                e
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
                                "Failed to deserialize state: {}",
                                e
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

    // === Query Methods ===

    /// Get current vector clock
    pub fn current_vector_clock(&self) -> &VectorClock {
        &self.vector_clock
    }

    /// Expose the logical clock in unified time format (vector + derived lamport).
    pub fn current_logical_time(&self) -> LogicalTime {
        LogicalTime {
            vector: self.vector_clock.clone(),
            lamport: max_counter(&self.vector_clock),
        }
    }

    /// Increment local actor clock if configured.
    fn bump_local(&mut self) {
        if let Some(actor) = self.actor {
            increment_actor(&mut self.vector_clock, actor);
        }
    }

    /// Get authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
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
        aura_core::util::serialization::to_vec(state).map_err(|e| {
            CrdtCoordinatorError::Serialization(format!("Failed to serialize state: {}", e))
        })
    }

    #[allow(dead_code)]
    fn deserialize_state<T: DeserializeOwned>(
        &self,
        bytes: &[u8],
    ) -> Result<T, CrdtCoordinatorError> {
        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!("Failed to deserialize state: {}", e))
        })
    }

    fn serialize_vector_clock(&self, clock: &VectorClock) -> Result<Vec<u8>, CrdtCoordinatorError> {
        aura_core::util::serialization::to_vec(clock).map_err(|e| {
            CrdtCoordinatorError::Serialization(format!("Failed to serialize vector clock: {}", e))
        })
    }

    fn deserialize_vector_clock(&self, bytes: &[u8]) -> Result<VectorClock, CrdtCoordinatorError> {
        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!(
                "Failed to deserialize vector clock: {}",
                e
            ))
        })
    }

    #[allow(dead_code)]
    fn deserialize_operation(&self, bytes: &[u8]) -> Result<Op, CrdtCoordinatorError> {
        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!("Failed to deserialize operation: {}", e))
        })
    }

    #[allow(dead_code)]
    fn deserialize_causal_context(
        &self,
        bytes: &[u8],
    ) -> Result<CausalContext, CrdtCoordinatorError> {
        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!(
                "Failed to deserialize causal context: {}",
                e
            ))
        })
    }

    #[allow(dead_code)]
    fn deserialize_delta(&self, bytes: &[u8]) -> Result<DeltaS::Delta, CrdtCoordinatorError> {
        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!("Failed to deserialize delta: {}", e))
        })
    }

    // === Convenience Builder Methods ===

    /// Create a coordinator with a convergent CRDT handler initialized with given state
    ///
    /// # Example
    /// ```ignore
    /// let coordinator = CrdtCoordinator::with_cv_state(authority_id, my_state);
    /// ```
    pub fn with_cv_state(authority_id: AuthorityId, state: CvS) -> Self {
        Self::new(authority_id).with_cv_handler(CvHandler::with_state(state))
    }

    /// Create a coordinator with a commutative CRDT handler
    ///
    /// # Example
    /// ```ignore
    /// let coordinator = CrdtCoordinator::with_cm(authority_id, initial_state);
    /// ```
    pub fn with_cm(authority_id: AuthorityId, state: CmS) -> Self {
        Self::new(authority_id).with_cm_handler(CmHandler::new(state))
    }

    /// Create a coordinator with a delta CRDT handler with compaction threshold
    ///
    /// # Example
    /// ```ignore
    /// let coordinator = CrdtCoordinator::with_delta_threshold(authority_id, 100);
    /// ```
    pub fn with_delta_threshold(authority_id: AuthorityId, threshold: usize) -> Self
    where
        DeltaS: Bottom,
    {
        Self::new(authority_id).with_delta_handler(DeltaHandler::with_threshold(threshold))
    }

    /// Create a coordinator with a meet-semilattice CRDT handler
    ///
    /// # Example
    /// ```ignore
    /// let coordinator = CrdtCoordinator::with_mv(authority_id);
    /// ```
    pub fn with_mv(authority_id: AuthorityId) -> Self
    where
        MvS: Top,
    {
        Self::new(authority_id).with_mv_handler(MvHandler::default())
    }

    /// Create a coordinator with a meet-semilattice CRDT handler initialized with given state
    ///
    /// # Example
    /// ```ignore
    /// let coordinator = CrdtCoordinator::with_mv_state(authority_id, my_constraints);
    /// ```
    pub fn with_mv_state(authority_id: AuthorityId, state: MvS) -> Self {
        Self::new(authority_id).with_mv_handler(MvHandler::with_state(state))
    }
}
