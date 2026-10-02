//! Typed publication boundary for parity-critical observed projections.
//!
//! A projection update is one graph transaction. Callers that construct a full
//! replacement from an earlier snapshot must carry its revision and publish
//! conditionally; a stale replacement is never allowed to erase a newer delta.

use aura_core::effects::reactive::{ReactiveError, Signal};
use aura_core::types::identifiers::AuthorityId;
use aura_invitation::InvitationFact;
use aura_relational::ContactFact;

use crate::effects::reactive::{ConditionalEmit, SignalSnapshot};
use crate::signal_defs::{
    CHAT_SIGNAL, CONTACTS_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL,
    RECOVERY_SIGNAL,
};
use crate::views::contacts::ContactAddedWitness;
use crate::views::home::HomeCreationWitness;
use crate::views::invitations::InvitationCreationWitness;
use crate::views::{
    ChatState, ContactsState, HomesState, InvitationsState, NeighborhoodState, RecoveryState,
};
use crate::ReactiveHandler;
use aura_social::SocialFact;

/// One of the app's observed projection cells. Construction is restricted to
/// the six canonical slots below so publication cannot silently select an
/// unrelated status or semantic-fact signal.
pub struct ProjectionSlot<T: 'static> {
    signal: &'static Signal<T>,
}

impl<T: 'static> Copy for ProjectionSlot<T> {}

impl<T: 'static> Clone for ProjectionSlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> ProjectionSlot<T> {
    /// The canonical reactive signal represented by this slot.
    pub(crate) fn signal(self) -> &'static Signal<T> {
        self.signal
    }
}

impl ProjectionSlot<ChatState> {
    /// The canonical chat projection slot.
    pub fn chat() -> Self {
        Self {
            signal: &CHAT_SIGNAL,
        }
    }
}
impl ProjectionSlot<ContactsState> {
    /// The canonical contacts projection slot.
    pub fn contacts() -> Self {
        Self {
            signal: &CONTACTS_SIGNAL,
        }
    }
}
impl ProjectionSlot<HomesState> {
    /// The canonical homes projection slot.
    pub fn homes() -> Self {
        Self {
            signal: &HOMES_SIGNAL,
        }
    }
}
impl ProjectionSlot<InvitationsState> {
    /// The canonical invitations projection slot.
    pub fn invitations() -> Self {
        Self {
            signal: &INVITATIONS_SIGNAL,
        }
    }
}
impl ProjectionSlot<RecoveryState> {
    /// The canonical recovery projection slot.
    pub fn recovery() -> Self {
        Self {
            signal: &RECOVERY_SIGNAL,
        }
    }
}
impl ProjectionSlot<NeighborhoodState> {
    /// The canonical neighborhood projection slot.
    pub fn neighborhood() -> Self {
        Self {
            signal: &NEIGHBORHOOD_SIGNAL,
        }
    }
}

/// Cloneable handle to the one serialized graph owner for observed projection
/// publication. The graph serializes writes per signal; this handle does not
/// introduce a second state store.
#[derive(Clone)]
pub struct ProjectionOwner {
    reactive: ReactiveHandler,
}

impl ProjectionOwner {
    /// Bind a projection owner to the app/runtime reactive graph.
    pub fn new(reactive: ReactiveHandler) -> Self {
        Self { reactive }
    }

    /// Derive home creation evidence from `SocialFact::HomeCreated` in the
    /// owned projection path. This proves fact shape; journal ingestion owns
    /// commitment and authenticity before this conversion.
    pub fn home_created_witness(&self, fact: &SocialFact) -> Option<HomeCreationWitness> {
        HomeCreationWitness::from_created_fact(fact)
    }

    /// Derive contact creation evidence while processing a canonical Added
    /// fact in the owned projection path. This proves fact shape, not journal
    /// commitment; scheduler ingestion owns that upstream boundary.
    pub fn contact_added_witness(&self, fact: &ContactFact) -> Option<ContactAddedWitness> {
        ContactAddedWitness::from_fact(fact)
    }

    /// Derive invitation creation evidence while processing a canonical Sent
    /// fact in the owned projection path. This proves fact shape, not journal
    /// commitment; scheduler ingestion owns that upstream boundary.
    pub fn invitation_sent_witness(
        &self,
        fact: &InvitationFact,
        own: AuthorityId,
    ) -> Option<InvitationCreationWitness> {
        InvitationCreationWitness::from_sent_fact(fact, own)
    }

    /// Read one projection value and its source revision atomically.
    pub async fn snapshot<T>(
        &self,
        slot: ProjectionSlot<T>,
    ) -> Result<SignalSnapshot<T>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.read_snapshot(slot.signal()).await
    }

    /// Apply a synchronous delta to the current value and return its committed
    /// revision. A failed callback leaves the signal unchanged.
    pub async fn update<T, R, E>(
        &self,
        slot: ProjectionSlot<T>,
        update: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<Result<(R, SignalSnapshot<T>), E>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.update_signal(slot.signal(), update).await
    }

    /// Publish a replacement derived from `expected_revision`; stale input is
    /// rejected without notifying observers.
    pub async fn replace_if_current<T>(
        &self,
        slot: ProjectionSlot<T>,
        expected_revision: u64,
        value: T,
    ) -> Result<ConditionalEmit, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive
            .compare_and_emit(slot.signal(), expected_revision, value)
            .await
    }
}

#[cfg(test)]
mod policy_tests {
    use std::path::Path;

    fn production_prefix(source: &str) -> &str {
        // Workflow test modules are at the end of their files. Their cfg may
        // be `test` or `all(test, feature = ...)`, so cut at the module rather
        // than at one specific attribute spelling.
        source.split("mod tests {").next().unwrap_or(source)
    }

    fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read workflow source directory") {
            let path = entry.expect("workflow source entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn workflow_projection_writes_use_the_typed_owner() {
        let mut files = Vec::new();
        rust_files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workflows"),
            &mut files,
        );
        for path in files {
            let source = std::fs::read_to_string(&path).expect("read workflow source");
            let production = production_prefix(&source);
            let lines: Vec<_> = production.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if !line.contains("emit_signal(") {
                    continue;
                }
                let call_start = index.saturating_sub(1);
                let call_end = (index + 6).min(lines.len());
                let call = lines[call_start..call_end].join(" ");
                for signal in [
                    "CHAT_SIGNAL",
                    "CONTACTS_SIGNAL",
                    "HOMES_SIGNAL",
                    "INVITATIONS_SIGNAL",
                    "RECOVERY_SIGNAL",
                    "NEIGHBORHOOD_SIGNAL",
                ] {
                    assert!(
                        !call.contains(&format!("&*{signal}")),
                        "{} emits {signal} outside ProjectionOwner",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn production_workflows_do_not_insert_raw_homes() {
        let mut files = Vec::new();
        let workflows = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workflows");
        rust_files(&workflows, &mut files);
        for path in files {
            if path.file_name().is_some_and(|name| name == "tests.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read workflow source");
            let production = production_prefix(&source);
            for insertion in [".add_home(", "HomesState::from_parts("] {
                assert!(
                    !production.contains(insertion),
                    "{} inserts a home without creation evidence via {insertion}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn policy_excludes_both_test_module_cfg_forms() {
        for attribute in ["#[cfg(test)]", "#[cfg(all(test, feature = \"signals\"))]"] {
            let source =
                format!("fn live() {{}}\n{attribute}\nmod tests {{ emit_signal(&*CHAT_SIGNAL); }}");
            assert_eq!(
                production_prefix(&source),
                format!("fn live() {{}}\n{attribute}\n")
            );
        }
    }
}
