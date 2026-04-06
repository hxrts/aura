//! Snapshot, runtime-event, and convergence validation surfaces for shared UI state.

use super::ids::{is_override_semantic_id, is_placeholder_semantic_id, is_row_index_semantic_id};
use super::{
    AuthoritativeSemanticFact, ChannelFactKey, ConfirmationState, ControlId, InvitationFactKind,
    ListId, ModalId, OperationId, OperationInstanceId, OperationState, RuntimeEventId, ScreenId,
    ToastId, ToastKind, UiReadiness,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastSnapshot {
    pub id: ToastId,
    pub kind: ToastKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItemSnapshot {
    pub id: String,
    pub selected: bool,
    pub confirmation: ConfirmationState,
    #[serde(default)]
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSnapshot {
    pub id: ListId,
    pub items: Vec<ListItemSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionSnapshot {
    pub list: ListId,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSnapshot {
    pub id: OperationId,
    pub instance_id: OperationInstanceId,
    pub state: OperationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEventKind {
    InvitationAccepted,
    InvitationCodeReady,
    PendingHomeInvitationReady,
    DeviceEnrollmentCodeReady,
    ContactLinkReady,
    HomeCreated,
    HomeEntered,
    ChannelJoined,
    ChannelMembershipReady,
    RecipientPeersResolved,
    MessageCommitted,
    MessageDeliveryReady,
    RemoteFactsPulled,
    ChatSignalUpdated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeFact {
    InvitationAccepted {
        invitation_kind: InvitationFactKind,
        authority_id: Option<String>,
        operation_state: Option<OperationState>,
    },
    InvitationCodeReady {
        receiver_authority_id: Option<String>,
        source_operation: OperationId,
        code: Option<String>,
    },
    PendingHomeInvitationReady,
    DeviceEnrollmentCodeReady {
        device_name: Option<String>,
        code_len: Option<usize>,
        code: Option<String>,
    },
    ContactLinkReady {
        authority_id: Option<String>,
        contact_count: Option<usize>,
    },
    HomeCreated {
        name: String,
    },
    HomeEntered {
        name: String,
        access_depth: Option<String>,
    },
    ChannelJoined {
        channel: Option<ChannelFactKey>,
        source: Option<String>,
    },
    ChannelMembershipReady {
        channel: ChannelFactKey,
        member_count: Option<u32>,
    },
    RecipientPeersResolved {
        channel: ChannelFactKey,
        member_count: u32,
    },
    MessageCommitted {
        channel: ChannelFactKey,
        content: String,
    },
    MessageDeliveryReady {
        channel: ChannelFactKey,
        member_count: u32,
    },
    RemoteFactsPulled {
        contact_count: u32,
        lan_peer_count: u32,
    },
    ChatSignalUpdated {
        active_channel: String,
        channel_count: u32,
        message_count: u32,
    },
}

impl RuntimeFact {
    #[must_use]
    pub fn kind(&self) -> RuntimeEventKind {
        match self {
            Self::InvitationAccepted { .. } => RuntimeEventKind::InvitationAccepted,
            Self::InvitationCodeReady { .. } => RuntimeEventKind::InvitationCodeReady,
            Self::PendingHomeInvitationReady => RuntimeEventKind::PendingHomeInvitationReady,
            Self::DeviceEnrollmentCodeReady { .. } => RuntimeEventKind::DeviceEnrollmentCodeReady,
            Self::ContactLinkReady { .. } => RuntimeEventKind::ContactLinkReady,
            Self::HomeCreated { .. } => RuntimeEventKind::HomeCreated,
            Self::HomeEntered { .. } => RuntimeEventKind::HomeEntered,
            Self::ChannelJoined { .. } => RuntimeEventKind::ChannelJoined,
            Self::ChannelMembershipReady { .. } => RuntimeEventKind::ChannelMembershipReady,
            Self::RecipientPeersResolved { .. } => RuntimeEventKind::RecipientPeersResolved,
            Self::MessageCommitted { .. } => RuntimeEventKind::MessageCommitted,
            Self::MessageDeliveryReady { .. } => RuntimeEventKind::MessageDeliveryReady,
            Self::RemoteFactsPulled { .. } => RuntimeEventKind::RemoteFactsPulled,
            Self::ChatSignalUpdated { .. } => RuntimeEventKind::ChatSignalUpdated,
        }
    }

    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::InvitationAccepted {
                invitation_kind,
                authority_id,
                ..
            } => format!(
                "invitation_accepted:{invitation_kind:?}:{}",
                authority_id.as_deref().unwrap_or("*")
            ),
            Self::InvitationCodeReady {
                receiver_authority_id,
                source_operation,
                ..
            } => format!(
                "invitation_code_ready:{}:{}",
                source_operation.0,
                receiver_authority_id.as_deref().unwrap_or("*")
            ),
            Self::PendingHomeInvitationReady => "pending_home_invitation_ready".to_string(),
            Self::DeviceEnrollmentCodeReady { device_name, .. } => format!(
                "device_enrollment_code_ready:{}",
                device_name.as_deref().unwrap_or("*")
            ),
            Self::ContactLinkReady {
                authority_id,
                contact_count,
            } => format!(
                "contact_link_ready:{}:{}",
                authority_id.as_deref().unwrap_or("*"),
                contact_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "*".to_string())
            ),
            Self::HomeCreated { name } => format!("home_created:{name}"),
            Self::HomeEntered { name, .. } => format!("home_entered:{name}"),
            Self::ChannelJoined { channel, source } => format!(
                "channel_joined:{}:{}",
                channel
                    .as_ref()
                    .and_then(|channel| channel.name.clone().or(channel.id.clone()))
                    .unwrap_or_else(|| "*".to_string()),
                source.as_deref().unwrap_or("*")
            ),
            Self::ChannelMembershipReady { channel, .. } => format!(
                "channel_membership_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::RecipientPeersResolved { channel, .. } => format!(
                "recipient_peers_resolved:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::MessageCommitted { channel, content } => format!(
                "message_committed:{}:{content}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::MessageDeliveryReady { channel, .. } => format!(
                "message_delivery_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::RemoteFactsPulled {
                contact_count,
                lan_peer_count,
            } => format!("remote_facts_pulled:{contact_count}:{lan_peer_count}"),
            Self::ChatSignalUpdated { active_channel, .. } => {
                format!("chat_signal_updated:{active_channel}")
            }
        }
    }

    #[must_use]
    pub fn matches_needle(&self, needle: &str) -> bool {
        match self {
            Self::InvitationAccepted {
                authority_id,
                operation_state,
                ..
            } => {
                authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || operation_state.is_some_and(|state| format!("{state:?}").contains(needle))
            }
            Self::InvitationCodeReady {
                receiver_authority_id,
                source_operation,
                code,
            } => {
                receiver_authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || source_operation.0.contains(needle)
                    || code.as_deref().is_some_and(|value| value.contains(needle))
            }
            Self::PendingHomeInvitationReady => needle.contains("pending_home_invitation"),
            Self::DeviceEnrollmentCodeReady {
                device_name,
                code_len,
                code,
            } => {
                device_name
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || code_len.is_some_and(|value| value.to_string().contains(needle))
                    || code.as_deref().is_some_and(|value| value.contains(needle))
            }
            Self::ContactLinkReady {
                authority_id,
                contact_count,
            } => {
                authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || contact_count.is_some_and(|value| value.to_string().contains(needle))
            }
            Self::HomeCreated { name } | Self::HomeEntered { name, .. } => name.contains(needle),
            Self::ChannelJoined { channel, source } => {
                channel
                    .as_ref()
                    .is_some_and(|channel| channel.matches_needle(needle))
                    || source
                        .as_deref()
                        .is_some_and(|value| value.contains(needle))
            }
            Self::ChannelMembershipReady {
                channel,
                member_count,
            } => {
                channel.matches_needle(needle)
                    || member_count.is_some_and(|value| value.to_string().contains(needle))
            }
            Self::RecipientPeersResolved {
                channel,
                member_count,
            }
            | Self::MessageDeliveryReady {
                channel,
                member_count,
            } => channel.matches_needle(needle) || member_count.to_string().contains(needle),
            Self::MessageCommitted { channel, content } => {
                channel.matches_needle(needle) || content.contains(needle)
            }
            Self::RemoteFactsPulled {
                contact_count,
                lan_peer_count,
            } => {
                contact_count.to_string().contains(needle)
                    || lan_peer_count.to_string().contains(needle)
            }
            Self::ChatSignalUpdated {
                active_channel,
                channel_count,
                message_count,
            } => {
                active_channel.contains(needle)
                    || channel_count.to_string().contains(needle)
                    || message_count.to_string().contains(needle)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEventSnapshot {
    pub id: RuntimeEventId,
    pub fact: RuntimeFact,
}

impl RuntimeEventSnapshot {
    #[must_use]
    pub fn kind(&self) -> RuntimeEventKind {
        self.fact.kind()
    }

    #[must_use]
    pub fn matches_needle(&self, needle: &str) -> bool {
        self.fact.matches_needle(needle)
    }

    #[must_use]
    pub fn key(&self) -> String {
        self.fact.key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSnapshot {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderHeartbeat {
    pub screen: ScreenId,
    pub open_modal: Option<ModalId>,
    pub render_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessShellMode {
    App,
    Onboarding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessShellStructureSnapshot {
    pub screen: ScreenId,
    pub app_root_count: u32,
    pub modal_region_count: u32,
    pub onboarding_root_count: u32,
    pub toast_region_count: u32,
    pub active_screen_root_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectionRevision {
    pub semantic_seq: u64,
    pub render_seq: Option<u64>,
}

impl ProjectionRevision {
    #[must_use]
    pub const fn has_sequence_metadata(self) -> bool {
        self.semantic_seq > 0 || self.render_seq.is_some()
    }

    #[must_use]
    pub const fn is_newer_than(self, previous: Self) -> bool {
        let self_render_seq = match self.render_seq {
            Some(value) => value,
            None => 0,
        };
        let previous_render_seq = match previous.render_seq {
            Some(value) => value,
            None => 0,
        };
        self.semantic_seq > previous.semantic_seq
            || (self.semantic_seq == previous.semantic_seq && self_render_seq > previous_render_seq)
    }

    #[must_use]
    pub const fn is_stale_against(self, baseline: Self) -> bool {
        !self.is_newer_than(baseline)
    }
}

static SEMANTIC_REVISION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[must_use]
pub fn next_projection_revision(render_seq: Option<u64>) -> ProjectionRevision {
    let counter = SEMANTIC_REVISION_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    ProjectionRevision {
        semantic_seq: counter,
        render_seq,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthoritativeSemanticFactsSnapshot {
    pub revision: ProjectionRevision,
    pub facts: Vec<AuthoritativeSemanticFact>,
}

impl Deref for AuthoritativeSemanticFactsSnapshot {
    type Target = [AuthoritativeSemanticFact];

    fn deref(&self) -> &Self::Target {
        &self.facts
    }
}

pub fn validate_render_convergence(
    snapshot: &UiSnapshot,
    heartbeat: &RenderHeartbeat,
) -> Result<(), String> {
    let Some(snapshot_render_seq) = snapshot.revision.render_seq else {
        return Err(format!(
            "semantic snapshot {:?} is missing render_seq metadata",
            snapshot.screen
        ));
    };
    if heartbeat.render_seq < snapshot_render_seq {
        return Err(format!(
            "semantic snapshot {:?} is ahead of renderer heartbeat {} < {}",
            snapshot.screen, heartbeat.render_seq, snapshot_render_seq
        ));
    }
    if heartbeat.screen != snapshot.screen {
        return Err(format!(
            "semantic snapshot screen {:?} diverges from renderer {:?}",
            snapshot.screen, heartbeat.screen
        ));
    }
    if heartbeat.open_modal != snapshot.open_modal {
        return Err(format!(
            "semantic snapshot modal {:?} diverges from renderer {:?}",
            snapshot.open_modal, heartbeat.open_modal
        ));
    }
    Ok(())
}

pub fn validate_harness_shell_structure(
    snapshot: &HarnessShellStructureSnapshot,
) -> Result<HarnessShellMode, String> {
    let onboarding_valid = snapshot.onboarding_root_count == 1
        && snapshot.app_root_count == 0
        && snapshot.modal_region_count == 0
        && snapshot.toast_region_count == 0
        && snapshot.active_screen_root_count == 0;
    if onboarding_valid {
        return Ok(HarnessShellMode::Onboarding);
    }

    let app_shell_valid = snapshot.app_root_count == 1
        && snapshot.modal_region_count == 1
        && snapshot.toast_region_count == 1
        && snapshot.active_screen_root_count == 1
        && snapshot.onboarding_root_count == 0;
    if app_shell_valid {
        return Ok(HarnessShellMode::App);
    }

    Err(format!(
        "invalid harness shell structure for {:?}: app_root_count={}, modal_region_count={}, onboarding_root_count={}, toast_region_count={}, active_screen_root_count={}",
        snapshot.screen,
        snapshot.app_root_count,
        snapshot.modal_region_count,
        snapshot.onboarding_root_count,
        snapshot.toast_region_count,
        snapshot.active_screen_root_count
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuiescenceState {
    Settled,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuiescenceSnapshot {
    pub state: QuiescenceState,
    pub reason_codes: Vec<String>,
}

impl QuiescenceSnapshot {
    #[must_use]
    pub fn settled() -> Self {
        Self {
            state: QuiescenceState::Settled,
            reason_codes: Vec::new(),
        }
    }

    #[must_use]
    pub fn derive(
        readiness: UiReadiness,
        open_modal: Option<ModalId>,
        operations: &[OperationSnapshot],
    ) -> Self {
        let mut reason_codes = Vec::new();
        if readiness != UiReadiness::Ready {
            reason_codes.push("readiness_loading".to_string());
        }
        if let Some(modal_id) = open_modal {
            if modal_id.blocks_quiescence() {
                reason_codes.push(format!("modal_open:{modal_id:?}").to_ascii_lowercase());
            }
        }
        for operation in operations {
            if operation.state == OperationState::Submitting {
                reason_codes.push(format!("operation_submitting:{}", operation.id.0));
            }
        }
        if reason_codes.is_empty() {
            Self::settled()
        } else {
            Self {
                state: QuiescenceState::Busy,
                reason_codes,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSnapshot {
    pub screen: ScreenId,
    pub focused_control: Option<ControlId>,
    pub open_modal: Option<ModalId>,
    pub readiness: UiReadiness,
    pub revision: ProjectionRevision,
    pub quiescence: QuiescenceSnapshot,
    pub selections: Vec<SelectionSnapshot>,
    pub lists: Vec<ListSnapshot>,
    pub messages: Vec<MessageSnapshot>,
    pub operations: Vec<OperationSnapshot>,
    pub toasts: Vec<ToastSnapshot>,
    pub runtime_events: Vec<RuntimeEventSnapshot>,
}

impl UiSnapshot {
    #[must_use]
    pub fn loading(screen: ScreenId) -> Self {
        Self {
            screen,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Loading,
            revision: ProjectionRevision {
                semantic_seq: 0,
                render_seq: None,
            },
            quiescence: QuiescenceSnapshot {
                state: QuiescenceState::Busy,
                reason_codes: vec!["readiness_loading".to_string()],
            },
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        }
    }

    pub fn validate_invariants(&self) -> Result<(), String> {
        let mut list_ids = HashSet::new();
        for list in &self.lists {
            if !list_ids.insert(list.id) {
                return Err(format!("duplicate list snapshot for {:?}", list.id));
            }
            if list.items.iter().any(|item| item.id.trim().is_empty()) {
                return Err(format!("list {:?} contains empty item id", list.id));
            }
            if let Some(item) = list.items.iter().find(|item| {
                is_placeholder_semantic_id(&item.id)
                    || is_override_semantic_id(&item.id)
                    || is_row_index_semantic_id(&item.id)
            }) {
                return Err(format!(
                    "list {:?} contains placeholder, override, or row-index item id {}",
                    list.id, item.id
                ));
            }
            if list.items.iter().filter(|item| item.selected).count() > 1 {
                return Err(format!(
                    "list {:?} exported multiple selected items",
                    list.id
                ));
            }

            let selected_row = list.items.iter().find(|item| item.selected);
            let exported_selection = self
                .selections
                .iter()
                .find(|selection| selection.list == list.id);
            match (selected_row, exported_selection) {
                (Some(row), Some(selection)) if row.id == selection.item_id => {}
                (Some(row), Some(selection)) => {
                    return Err(format!(
                        "list {:?} selected row {} diverges from exported selection {}",
                        list.id, row.id, selection.item_id
                    ));
                }
                (Some(row), None) => {
                    return Err(format!(
                        "list {:?} selected row {} without exported selection",
                        list.id, row.id
                    ));
                }
                (None, Some(selection)) => {
                    return Err(format!(
                        "list {:?} exported selection {} without matching selected row",
                        list.id, selection.item_id
                    ));
                }
                (None, None) => {}
            }
        }

        for selection in &self.selections {
            let Some(list) = self.lists.iter().find(|list| list.id == selection.list) else {
                return Err(format!(
                    "selection for {:?} has no corresponding list export",
                    selection.list
                ));
            };
            if !list.items.iter().any(|item| item.id == selection.item_id) {
                return Err(format!(
                    "selection for {:?} references missing item {}",
                    selection.list, selection.item_id
                ));
            }
            if is_placeholder_semantic_id(&selection.item_id)
                || is_override_semantic_id(&selection.item_id)
                || is_row_index_semantic_id(&selection.item_id)
            {
                return Err(format!(
                    "selection for {:?} references placeholder, override, or row-index item {}",
                    selection.list, selection.item_id
                ));
            }
        }

        if let Some(ControlId::Modal(modal)) = self.focused_control {
            if self.open_modal != Some(modal) {
                return Err(format!(
                    "focused modal {:?} does not match open modal {:?}",
                    modal, self.open_modal
                ));
            }
        }
        if let Some(ControlId::Screen(focused_screen)) = self.focused_control {
            if focused_screen != self.screen {
                return Err(format!(
                    "focused screen {:?} does not match current screen {:?}",
                    focused_screen, self.screen
                ));
            }
        }
        if self.open_modal.is_some() && matches!(self.focused_control, Some(ControlId::Screen(_))) {
            return Err("modal cannot be open while focus remains on a screen root".to_string());
        }
        if let Some(event) = self.runtime_events.iter().find(|event| {
            event.id.0.starts_with("inferred:") || event.id.0.starts_with("synthetic:")
        }) {
            return Err(format!(
                "runtime event {} uses inferred/synthetic success id",
                event.id.0
            ));
        }

        Ok(())
    }

    #[must_use]
    pub fn message_contains(&self, needle: &str) -> bool {
        self.messages
            .iter()
            .any(|message| message.content.contains(needle))
    }

    #[must_use]
    pub fn selected_item_id(&self, list: ListId) -> Option<&str> {
        self.selections
            .iter()
            .find(|selection| selection.list == list)
            .map(|selection| selection.item_id.as_str())
    }

    #[must_use]
    pub fn has_runtime_event(&self, kind: RuntimeEventKind, detail_needle: Option<&str>) -> bool {
        self.runtime_events.iter().any(|event| {
            event.kind() == kind
                && detail_needle
                    .map(|needle| event.matches_needle(needle))
                    .unwrap_or(true)
        })
    }

    #[must_use]
    pub fn operation_state(&self, operation_id: &OperationId) -> Option<OperationState> {
        self.operations
            .iter()
            .find(|candidate| &candidate.id == operation_id)
            .map(|operation| operation.state)
    }

    #[must_use]
    pub fn operation_state_for_instance(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
    ) -> Option<OperationState> {
        self.operations
            .iter()
            .find(|candidate| {
                &candidate.id == operation_id && &candidate.instance_id == instance_id
            })
            .map(|operation| operation.state)
    }
}
