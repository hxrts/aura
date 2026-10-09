//! Typed command responses.
//!
//! Every [`Request`](super::Request) succeeds with one [`Response`]. The CLI
//! renders it as text (or prints it under `--json`); `aura rpc` sends it as
//! the `result` of a response line.

use aura_app::ui::contract::{
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase, WorkflowTerminalStatus,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Terminal lifecycle of the semantic operation a command ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OperationView {
    /// snake_case `SemanticOperationKind`.
    #[schemars(with = "String")]
    pub kind: SemanticOperationKind,
    /// snake_case `SemanticOperationPhase`.
    #[schemars(with = "String")]
    pub phase: SemanticOperationPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<serde_json::Value>")]
    pub error: Option<SemanticOperationError>,
}

impl From<WorkflowTerminalStatus> for OperationView {
    fn from(terminal: WorkflowTerminalStatus) -> Self {
        Self {
            kind: terminal.status.kind,
            phase: terminal.status.phase,
            error: terminal.status.error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountView {
    pub authority_id: String,
    pub nickname: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub devices: u64,
    pub contacts: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuthorityView {
    pub authority_id: String,
    pub nickname: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContactView {
    pub authority_id: String,
    pub nickname: String,
    pub is_guardian: bool,
    pub is_member: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsView {
    pub nickname: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub mfa_policy: String,
    pub devices: Vec<String>,
    pub contacts: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CeremonyStatusView {
    pub ceremony_id: String,
    /// The runtime's record of the ceremony kind, e.g. `GuardianRotation`.
    pub kind: String,
    pub accepted: u16,
    pub total: u16,
    pub threshold: u16,
    pub complete: bool,
    pub failed: bool,
    pub error: Option<String>,
    pub pending_epoch: Option<u64>,
    /// `Provisional`, `CoordinatorSoftSafe` or `ConsensusFinalized`.
    pub agreement_mode: String,
    /// Whether the outcome can still be reverted (not consensus-finalized).
    pub reversion_risk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OtaReleaseView {
    pub release_id: String,
    pub series_id: String,
    pub version: String,
    pub declared_by: Vec<String>,
    pub artifacts: u64,
    pub certificates: u64,
    /// Recommended to this account.
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OtaUpgradeView {
    pub scope: String,
    pub to_release_id: String,
    pub from_release_id: Option<String>,
    /// `staged`, `cutover_approved`, `cutover_completed` or `rolled_back`.
    pub stage: String,
    pub approvals: u64,
    pub rollbacks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NotificationView {
    /// `friend_request`, `invitation_received`, `invitation_sent` or
    /// `recovery_request`.
    pub kind: String,
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelView {
    pub channel_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub is_dm: bool,
    pub member_count: u32,
    pub unread: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MessageView {
    pub message_id: String,
    pub channel_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub content: String,
    pub timestamp_ms: u64,
    pub is_own: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InvitationView {
    pub invitation_id: String,
    pub kind: String,
    pub sender_id: String,
    pub receiver_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryView {
    pub guardians: Vec<String>,
    pub threshold: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_ceremony: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AmpChannelView {
    pub context_id: String,
    pub channel_id: String,
    pub epoch: u64,
    pub generation: u64,
    pub last_checkpoint_generation: u64,
    pub skip_window: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_bump: Option<String>,
}

/// The successful outcome of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Account(AccountView),
    Authorities(Vec<AuthorityView>),
    Contacts(Vec<ContactView>),
    Contact(ContactView),
    /// A channel's participants.
    Members(Vec<String>),
    Settings(SettingsView),
    /// Known peer authorities.
    PeerList(Vec<String>),
    CeremonyStarted {
        ceremony_id: String,
    },
    CeremonyStatus(CeremonyStatusView),
    OtaReleases(Vec<OtaReleaseView>),
    OtaUpgrades(Vec<OtaUpgradeView>),
    OtaPublished {
        release_id: String,
    },
    NeighborhoodCreated {
        neighborhood_id: String,
    },
    /// Home storage budget, as text.
    Budget {
        summary: String,
    },
    Notifications(Vec<NotificationView>),
    Channels(Vec<ChannelView>),
    Channel(ChannelView),
    Messages(Vec<MessageView>),
    MessageSent {
        channel_id: String,
        message_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation: Option<OperationView>,
    },
    ChannelCreated {
        channel_id: String,
        context_id: String,
    },
    InvitationCreated {
        invitation_id: String,
        code: String,
    },
    Invitations(Vec<InvitationView>),
    Invitation(InvitationView),
    InvitationCode {
        invitation_id: String,
        code: String,
    },
    Export {
        format: super::ExportFormat,
        body: String,
    },
    HomeCreated {
        home_id: String,
    },
    RecoveryStarted {
        ceremony_id: String,
    },
    Recovery(RecoveryView),
    Sync {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Peers {
        connected: u64,
    },
    AmpChannel(AmpChannelView),
    AmpBumpProposed {
        parent_epoch: u64,
        new_epoch: u64,
        bump_id: String,
    },
    AmpCheckpoint {
        epoch: u64,
        base_generation: u64,
    },
    SnapshotProposed {
        proposal_id: String,
    },
    /// The request ran; `summary` says what happened.
    Done {
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation: Option<OperationView>,
    },
}

#[cfg(test)]
mod tests {
    use super::Response;

    #[test]
    fn response_counts_preserve_the_full_published_wire_range() {
        let response = Response::Peers {
            connected: u64::MAX,
        };
        let encoded = serde_json::to_value(&response).unwrap();
        assert_eq!(encoded["data"]["connected"].as_u64(), Some(u64::MAX));
        assert_eq!(
            serde_json::from_value::<Response>(encoded).unwrap(),
            response
        );
    }
}
