use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryPath {
    AcceptContactInvitationContactsFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPathClass {
    Authoritative,
    BoundedSecondary,
    DiagnosticsOnly,
}

impl RecoveryPath {
    pub const ALL: [Self; 1] = [Self::AcceptContactInvitationContactsFallback];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AcceptContactInvitationContactsFallback => {
                "accept_contact_invitation_contacts_fallback"
            }
        }
    }

    #[must_use]
    pub const fn owner_module(self) -> &'static str {
        match self {
            Self::AcceptContactInvitationContactsFallback => "crates/aura-harness/src/executor.rs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryMetadata {
    pub path: RecoveryPath,
    pub code: &'static str,
    pub owner_module: &'static str,
    pub class: FallbackPathClass,
}

pub const REGISTERED_RECOVERIES: &[RecoveryMetadata] = &[
    RecoveryMetadata {
        path: RecoveryPath::AcceptContactInvitationContactsFallback,
        code: RecoveryPath::AcceptContactInvitationContactsFallback.code(),
        owner_module: RecoveryPath::AcceptContactInvitationContactsFallback.owner_module(),
        class: FallbackPathClass::BoundedSecondary,
    },
];

pub fn run_registered_recovery<T>(
    path: RecoveryPath,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    action().with_context(|| format!("registered recovery path {} failed", path.code()))
}

#[cfg(test)]
mod tests {
    use super::{FallbackPathClass, RecoveryPath, REGISTERED_RECOVERIES};
    use std::collections::HashSet;

    #[test]
    fn registered_recoveries_cover_all_paths() {
        let registered = REGISTERED_RECOVERIES
            .iter()
            .map(|metadata| metadata.path)
            .collect::<HashSet<_>>();
        let all = RecoveryPath::ALL.into_iter().collect::<HashSet<_>>();

        assert_eq!(registered, all);
        assert!(REGISTERED_RECOVERIES
            .iter()
            .all(|metadata| !metadata.code.trim().is_empty()));
        assert!(REGISTERED_RECOVERIES.iter().all(|metadata| metadata
            .owner_module
            .starts_with("crates/aura-harness/src/")));
        assert!(REGISTERED_RECOVERIES
            .iter()
            .all(|metadata| metadata.class == FallbackPathClass::BoundedSecondary));
    }
}
