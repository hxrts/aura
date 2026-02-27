//! Protocol reconfiguration types shared across runtime and simulator layers.

use crate::time::ProvenancedTime;
use crate::{AuthorityId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Schema version for reconfiguration artifacts.
pub const RECONFIGURATION_SCHEMA_V1: &str = "aura.reconfiguration.v1";

fn default_schema_version() -> String {
    RECONFIGURATION_SCHEMA_V1.to_string()
}

/// Native/delegated session ownership footprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionFootprint {
    /// Schema version for compatibility checks.
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    /// Sessions currently hosted by this authority.
    pub native_sessions: BTreeSet<SessionId>,
    /// Sessions delegated into this authority.
    pub delegated_in_sessions: BTreeSet<SessionId>,
    /// Sessions delegated out from this authority.
    pub delegated_out_sessions: BTreeSet<SessionId>,
}

impl SessionFootprint {
    /// Create an empty session footprint.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema_version: default_schema_version(),
            native_sessions: BTreeSet::new(),
            delegated_in_sessions: BTreeSet::new(),
            delegated_out_sessions: BTreeSet::new(),
        }
    }

    /// Add a native-hosted session.
    pub fn add_native(&mut self, session_id: SessionId) {
        self.native_sessions.insert(session_id);
        self.delegated_in_sessions.remove(&session_id);
        self.delegated_out_sessions.remove(&session_id);
    }

    /// Mark a session as delegated in.
    pub fn add_delegated_in(&mut self, session_id: SessionId) {
        self.delegated_in_sessions.insert(session_id);
        self.native_sessions.remove(&session_id);
        self.delegated_out_sessions.remove(&session_id);
    }

    /// Mark a session as delegated out.
    pub fn add_delegated_out(&mut self, session_id: SessionId) {
        self.delegated_out_sessions.insert(session_id);
        self.native_sessions.remove(&session_id);
        self.delegated_in_sessions.remove(&session_id);
    }

    /// Remove a session from all footprint sets.
    pub fn remove(&mut self, session_id: SessionId) {
        self.native_sessions.remove(&session_id);
        self.delegated_in_sessions.remove(&session_id);
        self.delegated_out_sessions.remove(&session_id);
    }

    /// Return true when this footprint references the session.
    #[must_use]
    pub fn contains(&self, session_id: SessionId) -> bool {
        self.native_sessions.contains(&session_id)
            || self.delegated_in_sessions.contains(&session_id)
            || self.delegated_out_sessions.contains(&session_id)
    }

    /// Return all sessions represented in this footprint.
    #[must_use]
    pub fn all_sessions(&self) -> BTreeSet<SessionId> {
        self.native_sessions
            .iter()
            .copied()
            .chain(self.delegated_in_sessions.iter().copied())
            .chain(self.delegated_out_sessions.iter().copied())
            .collect()
    }
}

/// Linked choreography bundle produced by `link` composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposedBundle {
    /// Schema version for compatibility checks.
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    /// Stable bundle id.
    pub bundle_id: String,
    /// Protocol bundle ids merged into this composition.
    pub protocol_ids: Vec<String>,
    /// Exported interfaces (labels/capabilities).
    pub exports: BTreeSet<String>,
    /// Imported interfaces (labels/capabilities).
    pub imports: BTreeSet<String>,
    /// Sessions included in this bundle.
    pub session_footprint: SessionFootprint,
}

impl ComposedBundle {
    /// Create a bundle from explicit metadata.
    #[must_use]
    pub fn new(
        bundle_id: impl Into<String>,
        protocol_ids: Vec<String>,
        exports: BTreeSet<String>,
        imports: BTreeSet<String>,
        session_footprint: SessionFootprint,
    ) -> Self {
        Self {
            schema_version: default_schema_version(),
            bundle_id: bundle_id.into(),
            protocol_ids,
            exports,
            imports,
            session_footprint,
        }
    }

    /// Determine if this bundle can be linked with another bundle.
    #[must_use]
    pub fn compatible_with(&self, other: &Self) -> bool {
        // Imports that are not self-exported must be provided by the other side.
        let self_missing = self
            .imports
            .difference(&self.exports)
            .all(|required| other.exports.contains(required));
        let other_missing = other
            .imports
            .difference(&other.exports)
            .all(|required| self.exports.contains(required));
        self_missing && other_missing
    }
}

/// Receipt describing a successful session delegation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationReceipt {
    /// Schema version for compatibility checks.
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    /// Session transferred by this delegation.
    pub session_id: SessionId,
    /// Authority transferring ownership.
    pub from_authority: AuthorityId,
    /// Authority receiving delegated endpoint ownership.
    pub to_authority: AuthorityId,
    /// Bundle where the delegated session belongs (if known).
    pub bundle_id: Option<String>,
    /// Footprint snapshot before delegation.
    pub from_before: SessionFootprint,
    /// Footprint snapshot after delegation.
    pub from_after: SessionFootprint,
    /// Receiver footprint before delegation.
    pub to_before: SessionFootprint,
    /// Receiver footprint after delegation.
    pub to_after: SessionFootprint,
    /// Timestamp/provenance for this receipt.
    pub delegated_at: ProvenancedTime,
}

impl DelegationReceipt {
    /// Construct a delegation receipt.
    #[must_use]
    pub fn new(
        session_id: SessionId,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: Option<String>,
        from_before: SessionFootprint,
        from_after: SessionFootprint,
        to_before: SessionFootprint,
        to_after: SessionFootprint,
        delegated_at: ProvenancedTime,
    ) -> Self {
        Self {
            schema_version: default_schema_version(),
            session_id,
            from_authority,
            to_authority,
            bundle_id,
            from_before,
            from_after,
            to_before,
            to_after,
            delegated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{PhysicalTime, TimeStamp};
    use uuid::Uuid;

    fn sid(n: u8) -> SessionId {
        SessionId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    #[test]
    fn footprint_moves_session_between_sets() {
        let mut footprint = SessionFootprint::new();
        let session_id = sid(1);

        footprint.add_native(session_id);
        assert!(footprint.native_sessions.contains(&session_id));
        assert!(footprint.contains(session_id));

        footprint.add_delegated_out(session_id);
        assert!(footprint.delegated_out_sessions.contains(&session_id));
        assert!(!footprint.native_sessions.contains(&session_id));
    }

    #[test]
    fn bundle_compatibility_requires_missing_imports_to_be_exported() {
        let left = ComposedBundle::new(
            "left",
            vec!["p1".to_string()],
            BTreeSet::from(["chat.send".to_string()]),
            BTreeSet::from(["sync.pull".to_string()]),
            SessionFootprint::new(),
        );
        let right = ComposedBundle::new(
            "right",
            vec!["p2".to_string()],
            BTreeSet::from(["sync.pull".to_string()]),
            BTreeSet::from(["chat.send".to_string()]),
            SessionFootprint::new(),
        );
        assert!(left.compatible_with(&right));
        assert!(right.compatible_with(&left));
    }

    #[test]
    fn delegation_receipt_preserves_schema_version() {
        let from = AuthorityId::new_from_entropy([1u8; 32]);
        let to = AuthorityId::new_from_entropy([2u8; 32]);
        let session_id = sid(9);
        let footprint = SessionFootprint::new();
        let receipt = DelegationReceipt::new(
            session_id,
            from,
            to,
            Some("bundle-a".to_string()),
            footprint.clone(),
            footprint.clone(),
            footprint.clone(),
            footprint,
            ProvenancedTime {
                stamp: TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: 1,
                    uncertainty: None,
                }),
                proofs: vec![],
                origin: None,
            },
        );
        assert_eq!(receipt.schema_version, RECONFIGURATION_SCHEMA_V1);
    }
}
