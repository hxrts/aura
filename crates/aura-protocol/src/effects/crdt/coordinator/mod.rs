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

mod clock;
mod error;
mod sync;

pub use clock::{increment_actor, max_counter, merge_vector_clocks};
pub use error::CrdtCoordinatorError;

use crate::choreography::CrdtType;
use aura_core::{
    identifiers::DeviceId,
    semilattice::{Bottom, CausalOp, CmApply, CvState, Dedup, Delta, DeltaState, MvState, Top},
    time::{LogicalTime, VectorClock},
    AuthorityId,
};
use aura_journal::crdt::{CmHandler, CvHandler, DeltaHandler, MvHandler};
use aura_journal::CausalContext;
use serde::{de::DeserializeOwned, Serialize};
use std::marker::PhantomData;

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
    pub(super) cv_handler: Option<CvHandler<CvS>>,
    /// Commutative (operation-based) CRDT handler
    pub(super) cm_handler: Option<CmHandler<CmS, Op, Id>>,
    /// Delta-based CRDT handler
    pub(super) delta_handler: Option<DeltaHandler<DeltaS, DeltaS::Delta>>,
    /// Meet-semilattice CRDT handler
    pub(super) mv_handler: Option<MvHandler<MvS>>,
    /// Authority identifier for this coordinator
    pub(super) authority_id: AuthorityId,
    /// Optional actor/device identifier to advance local logical time
    pub(super) actor: Option<DeviceId>,
    /// Current vector clock for causal ordering
    pub(super) vector_clock: VectorClock,
    /// Type markers
    _phantom: PhantomData<(CvS, CmS, DeltaS, MvS, Op, Id)>,
}

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

    pub(super) fn serialize_state<T: Serialize>(
        &self,
        state: &T,
    ) -> Result<Vec<u8>, CrdtCoordinatorError> {
        aura_core::util::serialization::to_vec(state).map_err(|e| {
            CrdtCoordinatorError::Serialization(format!("Failed to serialize state: {e}"))
        })
    }

    pub(super) fn serialize_vector_clock(
        &self,
        clock: &VectorClock,
    ) -> Result<Vec<u8>, CrdtCoordinatorError> {
        aura_core::util::serialization::to_vec(clock).map_err(|e| {
            CrdtCoordinatorError::Serialization(format!("Failed to serialize vector clock: {e}"))
        })
    }

    pub(super) fn deserialize_vector_clock(
        &self,
        bytes: &[u8],
    ) -> Result<VectorClock, CrdtCoordinatorError> {
        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
            CrdtCoordinatorError::Deserialization(format!(
                "Failed to deserialize vector clock: {e}"
            ))
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
    pub fn with_delta_threshold(authority_id: AuthorityId, threshold: u32) -> Self
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
