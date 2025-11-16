//! CRDT Synchronization Types and Messages
//!
//! This module provides common types and message structures for CRDT
//! synchronization across choreographic protocols.

use aura_core::SessionId;
use serde::{Deserialize, Serialize};

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
