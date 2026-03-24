//! # ViewState Snapshot Helper
//!
//! Provides **non-reactive** snapshot helpers for:
//! - Best-effort role lookup for authorization gating
//!
//! Screens should subscribe directly to AppCore signals (two-phase pattern:
//! read current → subscribe) for reactive updates.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use crate::tui::hooks::{
    ChatSnapshot, ContactsSnapshot, DevicesSnapshot, GuardiansSnapshot, HomeSnapshot,
    InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};

#[derive(Debug, Clone)]
pub enum StateSnapshotAvailability {
    Available(Box<aura_app::ui::types::StateSnapshot>),
    Contended,
}

/// Helper for reading a best-effort snapshot of AppCore state.
///
/// This is intentionally **best-effort** (uses `try_read`) so UI dispatch paths
/// don't block if the lock is contended.
#[derive(Clone)]
pub struct SnapshotHelper {
    app_core: Arc<RwLock<AppCore>>,
    device_id_str: String,
}

impl SnapshotHelper {
    pub fn new(app_core: Arc<RwLock<AppCore>>, device_id_str: impl Into<String>) -> Self {
        Self {
            app_core,
            device_id_str: device_id_str.into(),
        }
    }

    /// Get an explicit snapshot-access result.
    #[must_use]
    pub fn state_snapshot_availability(&self) -> StateSnapshotAvailability {
        match self.app_core.try_read() {
            Some(core) => StateSnapshotAvailability::Available(Box::new(core.snapshot())),
            None => StateSnapshotAvailability::Contended,
        }
    }

    fn with_snapshot_or_default<T: Default>(
        &self,
        build: impl FnOnce(&aura_app::ui::types::StateSnapshot) -> T,
    ) -> T {
        match self.state_snapshot_availability() {
            StateSnapshotAvailability::Available(snapshot) => build(&snapshot),
            StateSnapshotAvailability::Contended => T::default(),
        }
    }
}

// ─── Snapshot Accessors ────────────────────────────────────────────────────
//
// These methods are best-effort and intentionally non-reactive.
//
// Production TUI screens should subscribe to signals (reactive) rather than
// relying on snapshots for rendering; snapshot accessors exist primarily for
// deterministic tests and a few explicit non-reactive helpers.
impl SnapshotHelper {
    #[must_use]
    pub fn snapshot_chat(&self) -> ChatSnapshot {
        self.with_snapshot_or_default(|snapshot| {
            ChatSnapshot {
                channels: snapshot.chat.all_channels().cloned().collect(),
                // Selection and messages are managed at a different level now
                selected_channel: None,
                messages: Vec::new(),
            }
        })
    }

    #[must_use]
    pub fn snapshot_guardians(&self) -> GuardiansSnapshot {
        self.with_snapshot_or_default(|snapshot| {
            let guardian_count = snapshot.recovery.guardian_count();
            GuardiansSnapshot {
                guardians: snapshot.recovery.all_guardians().cloned().collect(),
                threshold: aura_core::threshold::ThresholdConfig::new(
                    snapshot.recovery.threshold() as u16,
                    guardian_count as u16,
                )
                .ok(),
            }
        })
    }

    #[must_use]
    pub fn snapshot_recovery(&self) -> RecoverySnapshot {
        self.with_snapshot_or_default(|snapshot| {
            let (progress_percent, is_in_progress) = snapshot
                .recovery
                .active_recovery()
                .as_ref()
                .map(|r| {
                    (
                        r.progress,
                        !matches!(
                            r.status,
                            aura_app::ui::types::recovery::RecoveryProcessStatus::Idle
                        ),
                    )
                })
                .unwrap_or((0, false));

            RecoverySnapshot {
                status: snapshot.recovery.clone(),
                progress_percent,
                is_in_progress,
            }
        })
    }

    #[must_use]
    pub fn snapshot_invitations(&self) -> InvitationsSnapshot {
        self.with_snapshot_or_default(|snapshot| {
            let pending_count = snapshot.invitations.pending_count();
            let invitations = snapshot
                .invitations
                .all_pending()
                .iter()
                .chain(snapshot.invitations.all_sent().iter())
                .chain(snapshot.invitations.all_history().iter())
                .cloned()
                .collect();
            InvitationsSnapshot {
                invitations,
                pending_count,
            }
        })
    }

    #[must_use]
    pub fn snapshot_home(&self) -> HomeSnapshot {
        use aura_app::ui::types::home::HomeRole;

        self.with_snapshot_or_default(|snapshot| {
            let home_state = snapshot.homes.current_home().cloned();
            let my_role = home_state.as_ref().map(|b| b.my_role);
            HomeSnapshot {
                home_state,
                is_member: my_role.is_some(),
                is_moderator: matches!(my_role, Some(HomeRole::Moderator | HomeRole::Member)),
            }
        })
    }

    #[must_use]
    pub fn snapshot_contacts(&self) -> ContactsSnapshot {
        self.with_snapshot_or_default(|snapshot| ContactsSnapshot {
            contacts: snapshot.contacts.all_contacts().cloned().collect(),
        })
    }

    #[must_use]
    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        self.with_snapshot_or_default(|snapshot| {
            let home_id = snapshot.neighborhood.home_home_id.clone();
            let home_name = snapshot.neighborhood.home_name.clone();
            // Collect neighbors first before moving position
            let homes: Vec<_> = snapshot.neighborhood.all_neighbors().cloned().collect();
            let position = snapshot.neighborhood.position.clone().unwrap_or_else(|| {
                aura_app::ui::types::neighborhood::TraversalPosition {
                    current_home_id: home_id.clone(),
                    current_home_name: home_name.clone(),
                    depth: 0,
                    path: Vec::new(),
                }
            });
            NeighborhoodSnapshot {
                neighborhood_id: Some(home_id.to_string()),
                neighborhood_name: Some(home_name),
                homes,
                position,
            }
        })
    }

    #[must_use]
    pub fn snapshot_devices(&self) -> DevicesSnapshot {
        use crate::tui::types::Device;

        let current_device_id = self.device_id_str.clone();
        let devices = vec![Device::new(&current_device_id, "Current Device").current()];

        DevicesSnapshot {
            devices,
            current_device_id: Some(current_device_id),
        }
    }
}

#[allow(clippy::expect_used)] // Default is only used in tests
impl Default for SnapshotHelper {
    fn default() -> Self {
        // Default is only used in tests that construct helpers directly; it is
        // not valid for production.
        let core = aura_app::ui::types::AppCore::new(aura_app::ui::types::AppConfig::default())
            .expect("Failed to create default AppCore for SnapshotHelper");
        Self::new(Arc::new(RwLock::new(core)), "default-device")
    }
}
