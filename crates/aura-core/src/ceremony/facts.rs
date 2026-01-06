//! Standard ceremony facts and helpers.
//!
//! This module defines the canonical ceremony fact set and a shared reducer helper
//! for updating `CeremonyStatus`. Feature crates should embed these patterns in
//! their domain-specific fact enums.

use crate::ceremony::SupersessionReason;
use crate::domain::status::{CeremonyState, CeremonyStatus, SupersessionReason as StatusReason};
use crate::identifiers::CeremonyId;
use crate::query::ConsensusId;
use crate::threshold::AgreementMode;
use crate::time::PhysicalTime;

/// Common metadata carried by ceremony facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeremonyFactMeta {
    pub ceremony_id: CeremonyId,
    pub agreement_mode: Option<AgreementMode>,
    pub trace_id: Option<String>,
    pub timestamp_ms: u64,
}

/// Canonical ceremony fact set used across Category C protocols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StandardCeremonyFact {
    Initiated { meta: CeremonyFactMeta },
    AcceptanceReceived { meta: CeremonyFactMeta },
    Committed {
        meta: CeremonyFactMeta,
        consensus_id: Option<ConsensusId>,
        committed_at: Option<PhysicalTime>,
    },
    Aborted {
        meta: CeremonyFactMeta,
        reason: String,
    },
    Superseded {
        meta: CeremonyFactMeta,
        superseded_ceremony_id: CeremonyId,
        superseding_ceremony_id: CeremonyId,
        reason: SupersessionReason,
    },
}

/// Apply a standard ceremony fact to a `CeremonyStatus`.
///
/// Callers are responsible for providing the initial `CeremonyStatus` with a
/// valid prestate hash.
pub fn apply_standard_fact(status: &mut CeremonyStatus, fact: &StandardCeremonyFact) {
    match fact {
        StandardCeremonyFact::Initiated { .. } => {
            status.state = CeremonyState::Preparing;
        }
        StandardCeremonyFact::AcceptanceReceived { .. } => {
            status.state = CeremonyState::PendingEpoch {
                pending_epoch: crate::types::Epoch::new(0),
                required_responses: 0,
                received_responses: 0,
            };
        }
        StandardCeremonyFact::Committed {
            consensus_id,
            committed_at,
            ..
        } => {
            let consensus_id = consensus_id.unwrap_or_else(|| ConsensusId::new([0; 32]));
            let committed_at = committed_at.clone().unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });
            status.state = CeremonyState::Committed {
                consensus_id,
                committed_at,
            };
        }
        StandardCeremonyFact::Aborted { reason, .. } => {
            status.state = CeremonyState::Aborted {
                reason: reason.clone(),
                aborted_at: PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                },
            };
        }
        StandardCeremonyFact::Superseded {
            superseding_ceremony_id,
            reason,
            ..
        } => {
            status.state = CeremonyState::Superseded {
                by: superseding_ceremony_id.clone(),
                reason: to_status_reason(reason),
            };
        }
    }
}

fn to_status_reason(reason: &SupersessionReason) -> StatusReason {
    match reason {
        SupersessionReason::PrestateStale => StatusReason::PrestateStale,
        SupersessionReason::NewerRequest => StatusReason::NewerRequest,
        SupersessionReason::ExplicitCancel => StatusReason::ExplicitCancel,
        SupersessionReason::Timeout => StatusReason::Timeout,
        SupersessionReason::Precedence { .. } => StatusReason::Precedence,
    }
}
