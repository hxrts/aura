#![allow(missing_docs)]

use crate::core::StateSnapshot;
use std::fmt;

/// Snapshot token used to guarantee single-snapshot command resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotToken(pub(crate) u64);

impl SnapshotToken {
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SnapshotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snapshot-{}", self.0)
    }
}

/// Snapshot captured for command resolution.
#[derive(Debug, Clone)]
pub struct ResolverSnapshot {
    pub(crate) token: SnapshotToken,
    pub(crate) state: StateSnapshot,
}

impl ResolverSnapshot {
    #[must_use]
    pub fn token(&self) -> SnapshotToken {
        self.token
    }

    #[must_use]
    pub fn state(&self) -> &StateSnapshot {
        &self.state
    }
}
