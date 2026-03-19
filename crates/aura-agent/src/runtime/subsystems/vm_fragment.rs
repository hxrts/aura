//! Local ownership registry for admitted VM fragments.

use super::choreography::RuntimeChoreographySessionId;
use aura_mpst::CompositionManifest;
use std::collections::{BTreeMap, BTreeSet};

/// Stable identity for one locally owned admitted VM fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VmFragmentId {
    /// Runtime choreography session for this fragment.
    pub session_id: RuntimeChoreographySessionId,
    /// Stable fragment key derived from link metadata or protocol id.
    pub fragment_key: String,
}

/// Metadata for one locally owned VM fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmFragmentOwnerRecord {
    /// Local runtime owner token.
    pub owner_label: String,
    /// Protocol id that admitted this fragment.
    pub protocol_id: String,
    /// Optional bundle id when the fragment is link-scoped.
    pub bundle_id: Option<String>,
}

/// Errors raised while managing local VM fragment ownership.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VmFragmentOwnershipError {
    /// Another local owner already holds the fragment.
    #[error(
        "vm fragment {fragment_key} for session {session_id} is already owned by {existing_owner}; requested owner {requested_owner}"
    )]
    OwnerConflict {
        session_id: RuntimeChoreographySessionId,
        fragment_key: String,
        existing_owner: String,
        requested_owner: String,
    },
    /// A transfer was requested by a non-owner.
    #[error(
        "vm fragment {fragment_key} for session {session_id} is not owned by expected owner {expected_owner}"
    )]
    OwnerMismatch {
        session_id: RuntimeChoreographySessionId,
        fragment_key: String,
        expected_owner: String,
    },
    /// No active fragments remain for the requested session transfer.
    #[error("vm fragment session {session_id} is not owned by expected owner {expected_owner}")]
    SessionMissing {
        session_id: RuntimeChoreographySessionId,
        expected_owner: String,
    },
}

/// Local ownership registry for admitted VM fragments.
#[derive(Debug, Default)]
pub(in crate::runtime) struct VmFragmentRegistry {
    owners: BTreeMap<VmFragmentId, VmFragmentOwnerRecord>,
}

impl VmFragmentRegistry {
    /// Create an empty fragment registry.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim every fragment described by one admitted choreography manifest.
    pub(in crate::runtime) fn claim_manifest(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        owner_label: impl Into<String>,
        manifest: &CompositionManifest,
    ) -> Result<Vec<VmFragmentId>, VmFragmentOwnershipError> {
        let owner_label = owner_label.into();
        let fragments = fragment_records_for_manifest(session_id, &owner_label, manifest);

        for (fragment_id, record) in &fragments {
            if let Some(existing) = self.owners.get(fragment_id) {
                if existing.owner_label != owner_label {
                    return Err(VmFragmentOwnershipError::OwnerConflict {
                        session_id: fragment_id.session_id,
                        fragment_key: fragment_id.fragment_key.clone(),
                        existing_owner: existing.owner_label.clone(),
                        requested_owner: owner_label.clone(),
                    });
                }
            }
            if record.owner_label != owner_label {
                return Err(VmFragmentOwnershipError::OwnerConflict {
                    session_id: fragment_id.session_id,
                    fragment_key: fragment_id.fragment_key.clone(),
                    existing_owner: record.owner_label.clone(),
                    requested_owner: owner_label.clone(),
                });
            }
        }

        let fragment_ids = fragments
            .iter()
            .map(|(fragment_id, _)| fragment_id.clone())
            .collect::<Vec<_>>();
        for (fragment_id, record) in fragments {
            self.owners.insert(fragment_id, record);
        }
        Ok(fragment_ids)
    }

    /// Transfer all fragments for one session from one local owner to another.
    pub(in crate::runtime) fn transfer_session(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        from_owner: &str,
        to_owner: &str,
    ) -> Result<(), VmFragmentOwnershipError> {
        let fragment_ids = self
            .owners
            .keys()
            .filter(|fragment_id| fragment_id.session_id == session_id)
            .cloned()
            .collect::<Vec<_>>();

        if fragment_ids.is_empty() {
            return Err(VmFragmentOwnershipError::SessionMissing {
                session_id,
                expected_owner: from_owner.to_string(),
            });
        }

        for fragment_id in &fragment_ids {
            let Some(record) = self.owners.get(fragment_id) else {
                continue;
            };
            if record.owner_label != from_owner {
                return Err(VmFragmentOwnershipError::OwnerMismatch {
                    session_id,
                    fragment_key: fragment_id.fragment_key.clone(),
                    expected_owner: from_owner.to_string(),
                });
            }
        }

        for fragment_id in fragment_ids {
            if let Some(record) = self.owners.get_mut(&fragment_id) {
                record.owner_label = to_owner.to_string();
            }
        }
        Ok(())
    }

    /// Transfer all fragments for one session when fragments are present.
    ///
    /// Returns the number of transferred fragments. Sessions without locally
    /// owned fragments are not treated as an error because some runtime-owned
    /// choreography sessions never admit linked VM fragments.
    pub(in crate::runtime) fn transfer_session_if_present(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        from_owner: &str,
        to_owner: &str,
    ) -> Result<usize, VmFragmentOwnershipError> {
        let fragment_count = self
            .owners
            .keys()
            .filter(|fragment_id| fragment_id.session_id == session_id)
            .count();
        if fragment_count == 0 {
            return Ok(0);
        }

        self.transfer_session(session_id, from_owner, to_owner)?;
        Ok(fragment_count)
    }

    /// Release all fragments bound to one runtime session.
    pub(in crate::runtime) fn release_session(
        &mut self,
        session_id: RuntimeChoreographySessionId,
    ) -> Vec<VmFragmentId> {
        let fragment_ids = self
            .owners
            .keys()
            .filter(|fragment_id| fragment_id.session_id == session_id)
            .cloned()
            .collect::<Vec<_>>();
        for fragment_id in &fragment_ids {
            self.owners.remove(fragment_id);
        }
        fragment_ids
    }

    /// Snapshot every active fragment owner record.
    #[cfg(test)]
    pub fn snapshot(&self) -> Vec<(VmFragmentId, VmFragmentOwnerRecord)> {
        self.owners
            .iter()
            .map(|(fragment_id, record)| (fragment_id.clone(), record.clone()))
            .collect()
    }
}

fn fragment_records_for_manifest(
    session_id: RuntimeChoreographySessionId,
    owner_label: &str,
    manifest: &CompositionManifest,
) -> Vec<(VmFragmentId, VmFragmentOwnerRecord)> {
    let bundle_ids = manifest
        .link_specs
        .iter()
        .map(|spec| spec.bundle_id.clone())
        .collect::<BTreeSet<_>>();

    if bundle_ids.is_empty() {
        let fragment_key = format!("protocol:{}", manifest.protocol_id);
        return vec![(
            VmFragmentId {
                session_id,
                fragment_key: fragment_key.clone(),
            },
            VmFragmentOwnerRecord {
                owner_label: owner_label.to_string(),
                protocol_id: manifest.protocol_id.clone(),
                bundle_id: None,
            },
        )];
    }

    bundle_ids
        .into_iter()
        .map(|bundle_id| {
            (
                VmFragmentId {
                    session_id,
                    fragment_key: format!("bundle:{bundle_id}"),
                },
                VmFragmentOwnerRecord {
                    owner_label: owner_label.to_string(),
                    protocol_id: manifest.protocol_id.clone(),
                    bundle_id: Some(bundle_id),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::SessionId;
    use aura_mpst::CompositionLinkSpec;
    use uuid::Uuid;

    fn runtime_session(seed: u128) -> RuntimeChoreographySessionId {
        RuntimeChoreographySessionId::from_aura_session_id(SessionId::from_uuid(Uuid::from_u128(
            seed,
        )))
    }

    fn manifest(protocol_id: &str, bundle_ids: &[&str]) -> CompositionManifest {
        CompositionManifest {
            protocol_name: protocol_id.to_string(),
            protocol_namespace: None,
            protocol_qualified_name: protocol_id.to_string(),
            protocol_id: protocol_id.to_string(),
            role_names: vec!["A".to_string()],
            required_capabilities: Vec::new(),
            determinism_policy_ref: None,
            delegation_constraints: Vec::new(),
            link_specs: bundle_ids
                .iter()
                .map(|bundle_id| CompositionLinkSpec {
                    role: "A".to_string(),
                    bundle_id: (*bundle_id).to_string(),
                    imports: Vec::new(),
                    exports: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn claims_protocol_fragment_when_manifest_has_no_link_specs() {
        let mut registry = VmFragmentRegistry::new();
        let session_id = runtime_session(1);
        let manifest = manifest("aura.test.protocol", &[]);

        let claimed = registry
            .claim_manifest(session_id, "owner-a", &manifest)
            .expect("claim succeeds");

        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].fragment_key, "protocol:aura.test.protocol");
    }

    #[test]
    fn rejects_ambiguous_local_owner_for_same_fragment() {
        let mut registry = VmFragmentRegistry::new();
        let session_id = runtime_session(2);
        let manifest = manifest("aura.test.protocol", &["bundle-a"]);

        registry
            .claim_manifest(session_id, "owner-a", &manifest)
            .expect("first claim succeeds");

        let err = registry
            .claim_manifest(session_id, "owner-b", &manifest)
            .expect_err("second owner must fail");
        assert!(matches!(
            err,
            VmFragmentOwnershipError::OwnerConflict { .. }
        ));
    }

    #[test]
    fn transfers_session_fragments_between_local_owners() {
        let mut registry = VmFragmentRegistry::new();
        let session_id = runtime_session(3);
        let manifest = manifest("aura.test.protocol", &["bundle-a", "bundle-b"]);

        registry
            .claim_manifest(session_id, "owner-a", &manifest)
            .expect("claim succeeds");
        registry
            .transfer_session(session_id, "owner-a", "owner-b")
            .expect("transfer succeeds");

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot
            .iter()
            .all(|(_, record)| record.owner_label == "owner-b"));
    }

    #[test]
    fn releasing_session_clears_owned_fragments() {
        let mut registry = VmFragmentRegistry::new();
        let session_id = runtime_session(4);
        let manifest = manifest("aura.test.protocol", &["bundle-a"]);

        registry
            .claim_manifest(session_id, "owner-a", &manifest)
            .expect("claim succeeds");

        let released = registry.release_session(session_id);
        assert_eq!(released.len(), 1);
        assert!(registry.snapshot().is_empty());
    }

    #[test]
    fn released_session_rejects_transfer_with_explicit_error() {
        let mut registry = VmFragmentRegistry::new();
        let session_id = runtime_session(5);
        let manifest = manifest("aura.test.protocol", &["bundle-a"]);

        registry
            .claim_manifest(session_id, "owner-a", &manifest)
            .expect("claim succeeds");
        registry.release_session(session_id);

        let err = registry
            .transfer_session(session_id, "owner-a", "owner-b")
            .expect_err("released session must not transfer silently");
        assert!(matches!(
            err,
            VmFragmentOwnershipError::SessionMissing { .. }
        ));
    }
}
