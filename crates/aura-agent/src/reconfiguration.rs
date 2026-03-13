//! Runtime reconfiguration controller for protocol link/delegate operations.

use aura_core::{
    time::ProvenancedTime, AuthorityId, ComposedBundle, DelegationReceipt, SessionFootprint,
    SessionId,
};
use std::collections::{BTreeSet, HashMap};

/// Reconfiguration controller errors.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReconfigurationError {
    /// Attempted to register a bundle id that already exists.
    #[error("bundle already exists: {bundle_id}")]
    DuplicateBundle { bundle_id: String },
    /// Required bundle id does not exist.
    #[error("bundle not found: {bundle_id}")]
    BundleNotFound { bundle_id: String },
    /// Linked bundles contain overlapping sessions.
    #[error("cannot link bundles with overlapping sessions")]
    OverlappingSessions,
    /// Bundle interfaces are incompatible for linking.
    #[error("bundle interfaces are incompatible for link")]
    IncompatibleInterfaces,
    /// Requested delegation references an unknown session owner.
    #[error("session {session_id} not owned by authority {authority}")]
    SessionNotOwned {
        session_id: SessionId,
        authority: AuthorityId,
    },
    /// Delegation produced a coherence violation.
    #[error("reconfiguration coherence violation: {reason}")]
    CoherenceViolation { reason: String },
}

/// Coherence result for session footprints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoherenceStatus {
    /// No coherence violations detected.
    Coherent,
    /// One or more coherence violations detected.
    Violations(Vec<String>),
}

/// Target footprint class for lifecycle session updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFootprintClass {
    /// Session is hosted natively by this authority.
    Native,
    /// Session is delegated into this authority.
    DelegatedIn,
    /// Session is delegated out from this authority.
    DelegatedOut,
}

/// Mutable runtime controller for link/delegate operations.
#[derive(Debug, Clone, Default)]
pub struct ReconfigurationController {
    bundles: HashMap<String, ComposedBundle>,
    footprints: HashMap<AuthorityId, SessionFootprint>,
    delegation_log: Vec<DelegationReceipt>,
}

impl ReconfigurationController {
    /// Create an empty controller.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an existing bundle before link/delegate operations.
    pub fn register_bundle(&mut self, bundle: ComposedBundle) -> Result<(), ReconfigurationError> {
        if self.bundles.contains_key(&bundle.bundle_id) {
            return Err(ReconfigurationError::DuplicateBundle {
                bundle_id: bundle.bundle_id,
            });
        }
        self.bundles.insert(bundle.bundle_id.clone(), bundle);
        Ok(())
    }

    /// Snapshot a registered bundle by id.
    #[must_use]
    pub fn bundle(&self, bundle_id: &str) -> Option<&ComposedBundle> {
        self.bundles.get(bundle_id)
    }

    /// Snapshot per-authority session footprint.
    #[must_use]
    pub fn footprint(&self, authority: &AuthorityId) -> Option<&SessionFootprint> {
        self.footprints.get(authority)
    }

    /// Append/replace one authority footprint.
    pub fn set_footprint(&mut self, authority: AuthorityId, footprint: SessionFootprint) {
        self.footprints.insert(authority, footprint);
    }

    /// Extend an authority footprint with one session classification.
    pub fn footprint_extend(
        &mut self,
        authority: AuthorityId,
        session_id: SessionId,
        class: SessionFootprintClass,
    ) {
        let footprint = self.footprints.entry(authority).or_default();
        match class {
            SessionFootprintClass::Native => footprint.add_native(session_id),
            SessionFootprintClass::DelegatedIn => footprint.add_delegated_in(session_id),
            SessionFootprintClass::DelegatedOut => footprint.add_delegated_out(session_id),
        }
    }

    /// Remove a session from all ownership classes for one authority.
    pub fn footprint_remove(&mut self, authority: AuthorityId, session_id: SessionId) {
        if let Some(footprint) = self.footprints.get_mut(&authority) {
            footprint.remove(session_id);
        }
    }

    /// Read delegation receipts in insertion order.
    #[must_use]
    pub fn delegation_log(&self) -> &[DelegationReceipt] {
        &self.delegation_log
    }

    /// Statically compose two bundles into one linked bundle.
    pub fn link(
        &mut self,
        bundle_a: &str,
        bundle_b: &str,
        linked_bundle_id: impl Into<String>,
    ) -> Result<ComposedBundle, ReconfigurationError> {
        let left = self.bundles.get(bundle_a).cloned().ok_or_else(|| {
            ReconfigurationError::BundleNotFound {
                bundle_id: bundle_a.to_string(),
            }
        })?;
        let right = self.bundles.get(bundle_b).cloned().ok_or_else(|| {
            ReconfigurationError::BundleNotFound {
                bundle_id: bundle_b.to_string(),
            }
        })?;

        if !left.compatible_with(&right) || !right.compatible_with(&left) {
            return Err(ReconfigurationError::IncompatibleInterfaces);
        }

        let left_sessions = left.session_footprint.all_sessions();
        let right_sessions = right.session_footprint.all_sessions();
        if !left_sessions.is_disjoint(&right_sessions) {
            return Err(ReconfigurationError::OverlappingSessions);
        }

        let linked_bundle_id = linked_bundle_id.into();
        let mut protocol_ids = left.protocol_ids;
        protocol_ids.extend(right.protocol_ids);

        let mut exports = left.exports;
        exports.extend(right.exports);
        let mut imports = left.imports;
        imports.extend(right.imports);

        let mut session_footprint = SessionFootprint::new();
        for session_id in left.session_footprint.native_sessions {
            session_footprint.add_native(session_id);
        }
        for session_id in right.session_footprint.native_sessions {
            session_footprint.add_native(session_id);
        }
        for session_id in left.session_footprint.delegated_in_sessions {
            session_footprint.add_delegated_in(session_id);
        }
        for session_id in right.session_footprint.delegated_in_sessions {
            session_footprint.add_delegated_in(session_id);
        }
        for session_id in left.session_footprint.delegated_out_sessions {
            session_footprint.add_delegated_out(session_id);
        }
        for session_id in right.session_footprint.delegated_out_sessions {
            session_footprint.add_delegated_out(session_id);
        }

        let linked = ComposedBundle::new(
            linked_bundle_id.clone(),
            protocol_ids,
            exports,
            imports,
            session_footprint,
        );
        self.register_bundle(linked.clone())?;
        Ok(linked)
    }

    /// Dynamically delegate one session endpoint from `from_authority` to `to_authority`.
    pub fn delegate(
        &mut self,
        session_id: SessionId,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: Option<String>,
        delegated_at: ProvenancedTime,
    ) -> Result<DelegationReceipt, ReconfigurationError> {
        if let Some(bundle_id) = &bundle_id {
            if !self.bundles.contains_key(bundle_id) {
                return Err(ReconfigurationError::BundleNotFound {
                    bundle_id: bundle_id.clone(),
                });
            }
        }

        let from_before = self
            .footprints
            .get(&from_authority)
            .cloned()
            .unwrap_or_else(SessionFootprint::new);
        if !from_before.contains(session_id) {
            return Err(ReconfigurationError::SessionNotOwned {
                session_id,
                authority: from_authority,
            });
        }
        let to_before = self
            .footprints
            .get(&to_authority)
            .cloned()
            .unwrap_or_else(SessionFootprint::new);

        let mut candidate = self.clone_without_log();

        // If from_authority had delegated_in for this session, they received it from
        // a previous delegator. When re-delegating, the previous delegator's
        // delegated_out should be cleared since the session has moved on.
        if from_before.delegated_in_sessions.contains(&session_id) {
            // Find the previous delegator (who has delegated_out for this session)
            // and clear their delegated_out since the chain is being extended
            for (authority, footprint) in &self.footprints {
                if footprint.delegated_out_sessions.contains(&session_id)
                    && *authority != from_authority
                {
                    candidate.footprint_remove(*authority, session_id);
                }
            }
        }

        candidate.footprint_remove(from_authority, session_id);
        candidate.footprint_extend(
            from_authority,
            session_id,
            SessionFootprintClass::DelegatedOut,
        );
        candidate.footprint_extend(to_authority, session_id, SessionFootprintClass::DelegatedIn);

        if let CoherenceStatus::Violations(violations) = verify_coherence_map(&candidate.footprints)
        {
            return Err(ReconfigurationError::CoherenceViolation {
                reason: violations.join("; "),
            });
        }
        let from_after = candidate
            .footprints
            .get(&from_authority)
            .cloned()
            .unwrap_or_else(SessionFootprint::new);
        let to_after = candidate
            .footprints
            .get(&to_authority)
            .cloned()
            .unwrap_or_else(SessionFootprint::new);

        self.footprints = candidate.footprints;
        let receipt = DelegationReceipt::new(
            session_id,
            from_authority,
            to_authority,
            bundle_id,
            from_before,
            from_after,
            to_before,
            to_after,
            delegated_at,
        );
        self.delegation_log.push(receipt.clone());
        Ok(receipt)
    }

    /// Verify global reconfiguration coherence across all tracked footprints.
    #[must_use]
    pub fn verify_coherence(&self) -> CoherenceStatus {
        verify_coherence_map(&self.footprints)
    }

    fn clone_without_log(&self) -> Self {
        Self {
            bundles: self.bundles.clone(),
            footprints: self.footprints.clone(),
            delegation_log: Vec::new(),
        }
    }
}

fn verify_coherence_map(footprints: &HashMap<AuthorityId, SessionFootprint>) -> CoherenceStatus {
    let mut violations = Vec::new();
    let mut active_owners: HashMap<SessionId, Vec<AuthorityId>> = HashMap::new();
    let mut delegated_out: HashMap<SessionId, BTreeSet<AuthorityId>> = HashMap::new();
    let mut delegated_in: HashMap<SessionId, BTreeSet<AuthorityId>> = HashMap::new();

    for (authority, footprint) in footprints {
        for session_id in footprint
            .native_sessions
            .iter()
            .chain(footprint.delegated_in_sessions.iter())
        {
            active_owners
                .entry(*session_id)
                .or_default()
                .push(*authority);
        }
        for session_id in &footprint.delegated_out_sessions {
            delegated_out
                .entry(*session_id)
                .or_default()
                .insert(*authority);
        }
        for session_id in &footprint.delegated_in_sessions {
            delegated_in
                .entry(*session_id)
                .or_default()
                .insert(*authority);
        }
    }

    for (session_id, owners) in active_owners {
        if owners.len() > 1 {
            violations.push(format!(
                "session {session_id} has multiple active owners ({})",
                owners.len()
            ));
        }
    }

    for (session_id, from_authorities) in delegated_out {
        match delegated_in.get(&session_id) {
            Some(to_authorities) if to_authorities.len() == 1 => {
                if from_authorities.len() != 1 {
                    violations.push(format!(
                        "session {session_id} delegated out by {} authorities",
                        from_authorities.len()
                    ));
                }
            }
            Some(to_authorities) => {
                violations.push(format!(
                    "session {session_id} delegated in to {} authorities",
                    to_authorities.len()
                ));
            }
            None => violations.push(format!(
                "session {session_id} delegated out without delegated-in receiver"
            )),
        }
    }

    if violations.is_empty() {
        CoherenceStatus::Coherent
    } else {
        CoherenceStatus::Violations(violations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::{PhysicalTime, TimeStamp};
    use std::collections::BTreeSet;
    use uuid::Uuid;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn session(seed: u8) -> SessionId {
        SessionId::from_uuid(Uuid::from_bytes([seed; 16]))
    }

    fn test_time(ts_ms: u64) -> ProvenancedTime {
        ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
            proofs: vec![],
            origin: None,
        }
    }

    #[test]
    fn link_rejects_overlapping_session_footprints() {
        let shared = session(9);
        let mut controller = ReconfigurationController::new();
        let mut left_fp = SessionFootprint::new();
        left_fp.add_native(shared);
        let mut right_fp = SessionFootprint::new();
        right_fp.add_native(shared);

        controller
            .register_bundle(ComposedBundle::new(
                "left",
                vec!["p.left".to_string()],
                BTreeSet::from(["x".to_string()]),
                BTreeSet::new(),
                left_fp,
            ))
            .expect("left bundle should register");
        controller
            .register_bundle(ComposedBundle::new(
                "right",
                vec!["p.right".to_string()],
                BTreeSet::from(["y".to_string()]),
                BTreeSet::new(),
                right_fp,
            ))
            .expect("right bundle should register");

        let err = controller
            .link("left", "right", "linked")
            .expect_err("overlapping sessions must be rejected");
        assert_eq!(err, ReconfigurationError::OverlappingSessions);
    }

    #[test]
    fn delegate_updates_footprints_and_appends_receipt() {
        let from = authority(1);
        let to = authority(2);
        let sid = session(7);
        let mut from_fp = SessionFootprint::new();
        from_fp.add_native(sid);

        let mut controller = ReconfigurationController::new();
        controller.set_footprint(from, from_fp);

        let receipt = controller
            .delegate(sid, from, to, None, test_time(100))
            .expect("delegation should succeed");

        assert!(receipt.from_after.delegated_out_sessions.contains(&sid));
        assert!(receipt.to_after.delegated_in_sessions.contains(&sid));
        assert_eq!(controller.delegation_log().len(), 1);
        assert_eq!(controller.verify_coherence(), CoherenceStatus::Coherent);
    }

    #[test]
    fn footprint_extend_and_remove_updates_classification() {
        let authority = authority(4);
        let sid = session(6);
        let mut controller = ReconfigurationController::new();

        controller.footprint_extend(authority, sid, SessionFootprintClass::Native);
        let footprint = controller
            .footprint(&authority)
            .expect("footprint should exist after extend");
        assert!(footprint.native_sessions.contains(&sid));

        controller.footprint_remove(authority, sid);
        let footprint = controller
            .footprint(&authority)
            .expect("footprint should remain allocated");
        assert!(!footprint.contains(sid));
    }

    #[test]
    fn coherence_detects_orphaned_delegated_out_session() {
        let sid = session(5);
        let authority = authority(3);
        let mut footprint = SessionFootprint::new();
        footprint.add_delegated_out(sid);

        let mut controller = ReconfigurationController::new();
        controller.set_footprint(authority, footprint);

        match controller.verify_coherence() {
            CoherenceStatus::Coherent => panic!("orphaned delegated_out must be flagged"),
            CoherenceStatus::Violations(violations) => assert!(!violations.is_empty()),
        }
    }

    #[test]
    fn repeated_delegation_under_churn_preserves_coherence() {
        let a = authority(1);
        let b = authority(2);
        let c = authority(3);
        let sid = session(10);
        let mut controller = ReconfigurationController::new();
        let mut footprint = SessionFootprint::new();
        footprint.add_native(sid);
        controller.set_footprint(a, footprint);

        controller
            .delegate(sid, a, b, None, test_time(1))
            .expect("a->b delegation must succeed");
        assert_eq!(controller.verify_coherence(), CoherenceStatus::Coherent);

        controller
            .delegate(sid, b, c, None, test_time(2))
            .expect("b->c delegation must succeed");
        assert_eq!(controller.verify_coherence(), CoherenceStatus::Coherent);

        controller
            .delegate(sid, c, a, None, test_time(3))
            .expect("c->a delegation must succeed");
        assert_eq!(controller.verify_coherence(), CoherenceStatus::Coherent);
    }
}
