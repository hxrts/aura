//! Shared typed runtime errors for boundary, routing, and delegation checks.

use super::contracts::AuraLinkBoundary;
use super::subsystems::choreography::{RuntimeChoreographySessionId, SessionOwnerCapabilityScope};
use aura_core::SessionId;
use thiserror::Error;

/// Typed boundary/routing failures across runtime ownership and delegation paths.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeBoundaryError {
    #[error("no choreography session is bound to the current task")]
    MissingTaskBinding,
    #[error(
        "current task is bound to session {bound_session_id} instead of expected session {expected_session_id}"
    )]
    SessionBindingMismatch {
        expected_session_id: RuntimeChoreographySessionId,
        bound_session_id: RuntimeChoreographySessionId,
    },
    #[error("current capability does not authorize full-session ingress")]
    FullSessionCapabilityRequired,
    #[error("session owner capability rejected: {details}")]
    CapabilityRejected { details: String },
    #[error(
        "current capability scope {capability_scope:?} does not authorize routing boundary {boundary:?}"
    )]
    BoundaryScopeRejected {
        boundary: AuraLinkBoundary,
        capability_scope: SessionOwnerCapabilityScope,
    },
    #[error(
        "link boundary bundle {boundary_bundle_id:?} does not match transfer bundle `{bundle_id}` for session {session_id}"
    )]
    LinkBoundaryBundleMismatch {
        session_id: SessionId,
        bundle_id: String,
        boundary_bundle_id: Option<String>,
    },
    #[error(
        "link boundary scope {boundary_scope:?} does not match transfer capability scope {capability_scope:?} for session {session_id} bundle `{bundle_id}`"
    )]
    LinkBoundaryScopeMismatch {
        session_id: SessionId,
        bundle_id: String,
        boundary_scope: SessionOwnerCapabilityScope,
        capability_scope: SessionOwnerCapabilityScope,
    },
}
