//! # ViewState Snapshot Helper
//!
//! Provides **non-reactive** snapshot helpers for:
//! - Building `IntentContext` for `command_to_intent` mapping
//! - Best-effort role lookup for authorization gating
//!
//! Screens should subscribe directly to AppCore signals (two-phase pattern:
//! read current → subscribe) for reactive updates.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use crate::tui::effects::IntentContext;

use crate::tui::hooks::{
    BlockSnapshot, ChatSnapshot, ContactsSnapshot, DevicesSnapshot, GuardiansSnapshot,
    InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};

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

    /// Get a best-effort `StateSnapshot` (returns `None` if lock is contended).
    pub fn try_state_snapshot(&self) -> Option<aura_app::StateSnapshot> {
        self.app_core.try_read().map(|core| core.snapshot())
    }

    /// Build an `IntentContext` from the latest available snapshot.
    pub fn intent_context(&self) -> IntentContext {
        self.try_state_snapshot()
            .as_ref()
            .map(IntentContext::from_snapshot)
            .unwrap_or_else(IntentContext::empty)
    }
}

// ─── Snapshot Accessors ────────────────────────────────────────────────────
//
// These methods are best-effort and intentionally non-reactive.
//
// Production TUI screens should subscribe to signals (reactive) rather than
// relying on snapshots for rendering; snapshot accessors exist primarily for
// deterministic tests and a few legacy utilities.
impl SnapshotHelper {
    pub fn snapshot_chat(&self) -> ChatSnapshot {
        if let Some(snapshot) = self.try_state_snapshot() {
            ChatSnapshot {
                channels: snapshot.chat.channels,
                selected_channel: snapshot.chat.selected_channel_id.map(|id| id.to_string()),
                messages: snapshot.chat.messages,
            }
        } else {
            ChatSnapshot::default()
        }
    }

    pub fn snapshot_guardians(&self) -> GuardiansSnapshot {
        if let Some(snapshot) = self.try_state_snapshot() {
            GuardiansSnapshot {
                guardians: snapshot.recovery.guardians.clone(),
                threshold: aura_core::threshold::ThresholdConfig::new(
                    snapshot.recovery.threshold as u16,
                    snapshot.recovery.guardian_count as u16,
                )
                .ok(),
            }
        } else {
            GuardiansSnapshot::default()
        }
    }

    pub fn snapshot_recovery(&self) -> RecoverySnapshot {
        if let Some(snapshot) = self.try_state_snapshot() {
            let (progress_percent, is_in_progress) = snapshot
                .recovery
                .active_recovery
                .as_ref()
                .map(|r| {
                    (
                        r.progress,
                        !matches!(
                            r.status,
                            aura_app::views::recovery::RecoveryProcessStatus::Idle
                        ),
                    )
                })
                .unwrap_or((0, false));

            RecoverySnapshot {
                status: snapshot.recovery,
                progress_percent,
                is_in_progress,
            }
        } else {
            RecoverySnapshot::default()
        }
    }

    pub fn snapshot_invitations(&self) -> InvitationsSnapshot {
        if let Some(snapshot) = self.try_state_snapshot() {
            let pending_count = snapshot.invitations.pending_count as usize;
            let invitations = snapshot
                .invitations
                .pending
                .iter()
                .chain(snapshot.invitations.sent.iter())
                .chain(snapshot.invitations.history.iter())
                .cloned()
                .collect();
            InvitationsSnapshot {
                invitations,
                pending_count,
            }
        } else {
            InvitationsSnapshot::default()
        }
    }

    pub fn snapshot_block(&self) -> BlockSnapshot {
        use aura_app::views::block::ResidentRole;
        use aura_core::identifiers::ChannelId;

        if let Some(snapshot) = self.try_state_snapshot() {
            let block = snapshot.blocks.current_block().cloned().or_else(|| {
                // Check if block has default (empty) ID
                if snapshot.block.id == ChannelId::default() {
                    None
                } else {
                    Some(snapshot.block)
                }
            });

            let my_role = block.as_ref().map(|b| b.my_role);
            BlockSnapshot {
                block,
                is_resident: my_role.is_some(),
                is_steward: matches!(my_role, Some(ResidentRole::Admin | ResidentRole::Owner)),
            }
        } else {
            BlockSnapshot::default()
        }
    }

    pub fn snapshot_contacts(&self) -> ContactsSnapshot {
        if let Some(snapshot) = self.try_state_snapshot() {
            ContactsSnapshot {
                contacts: snapshot.contacts.contacts,
            }
        } else {
            ContactsSnapshot::default()
        }
    }

    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        if let Some(snapshot) = self.try_state_snapshot() {
            let home_id = snapshot.neighborhood.home_block_id.clone();
            let home_name = snapshot.neighborhood.home_block_name.clone();
            let position = snapshot.neighborhood.position.unwrap_or_else(|| {
                aura_app::views::neighborhood::TraversalPosition {
                    current_block_id: home_id.clone(),
                    current_block_name: home_name.clone(),
                    depth: 0,
                    path: Vec::new(),
                }
            });
            NeighborhoodSnapshot {
                neighborhood_id: Some(home_id.to_string()),
                neighborhood_name: Some(home_name),
                blocks: snapshot.neighborhood.neighbors,
                position,
            }
        } else {
            NeighborhoodSnapshot::default()
        }
    }

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
        let core = aura_app::AppCore::new(aura_app::AppConfig::default())
            .expect("Failed to create default AppCore for SnapshotHelper");
        Self::new(Arc::new(RwLock::new(core)), "default-device")
    }
}
