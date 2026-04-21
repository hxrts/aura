//! Shared semantic scenario identity and value surfaces.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(pub String);

const RESERVED_FRONTEND_ACTOR_IDS: &[&str] =
    &["web", "tui", "browser", "local", "playwright", "pty"];

pub(crate) fn is_row_index_item_id(raw: &str) -> bool {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.chars().all(|ch| ch.is_ascii_digit())
        || trimmed
            .strip_prefix("row-")
            .or_else(|| trimmed.strip_prefix("row_"))
            .or_else(|| trimmed.strip_prefix("row:"))
            .or_else(|| trimmed.strip_prefix("idx-"))
            .or_else(|| trimmed.strip_prefix("idx_"))
            .or_else(|| trimmed.strip_prefix("idx:"))
            .or_else(|| trimmed.strip_prefix("index-"))
            .or_else(|| trimmed.strip_prefix("index_"))
            .or_else(|| trimmed.strip_prefix("index:"))
            .map(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or(false)
}

impl ActorId {
    #[must_use]
    pub fn is_frontend_binding_label(&self) -> bool {
        let normalized = self.0.trim().to_ascii_lowercase();
        RESERVED_FRONTEND_ACTOR_IDS.contains(&normalized.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SharedActionId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    OpenScreen,
    CreateAccount,
    CreateHome,
    CreateChannel,
    StartDeviceEnrollment,
    ImportDeviceEnrollmentCode,
    OpenSettingsSection,
    RemoveSelectedDevice,
    SwitchAuthority,
    CreateContactInvitation,
    AcceptContactInvitation,
    AcceptPendingChannelInvitation,
    JoinChannel,
    InviteActorToChannel,
    SendChatMessage,
    SendFriendRequest,
    AcceptFriendRequest,
    DeclineFriendRequest,
    PublishAmpTransitionFixture,
}

impl IntentKind {
    pub const ALL: [Self; 19] = [
        Self::OpenScreen,
        Self::CreateAccount,
        Self::CreateHome,
        Self::CreateChannel,
        Self::StartDeviceEnrollment,
        Self::ImportDeviceEnrollmentCode,
        Self::OpenSettingsSection,
        Self::RemoveSelectedDevice,
        Self::SwitchAuthority,
        Self::CreateContactInvitation,
        Self::AcceptContactInvitation,
        Self::AcceptPendingChannelInvitation,
        Self::JoinChannel,
        Self::InviteActorToChannel,
        Self::SendChatMessage,
        Self::SendFriendRequest,
        Self::AcceptFriendRequest,
        Self::DeclineFriendRequest,
        Self::PublishAmpTransitionFixture,
    ];
}
