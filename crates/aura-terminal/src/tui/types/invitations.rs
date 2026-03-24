use aura_app::ui::types::invitations::{
    Invitation as AppInvitation, InvitationDirection as AppInvitationDirection,
    InvitationStatus as AppInvitationStatus, InvitationType as AppInvitationType,
};
use iocraft::prelude::Color;

use crate::tui::theme::Theme;

/// Direction of an invitation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationDirection {
    #[default]
    Outbound,
    Inbound,
}

impl InvitationDirection {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Outbound => "->",
            Self::Inbound => "<-",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Outbound => "Sent to",
            Self::Inbound => "Received from",
        }
    }
}

/// Status of an invitation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationStatus {
    #[default]
    Pending,
    Accepted,
    Declined,
    Expired,
    Cancelled,
}

impl InvitationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Accepted => "Accepted",
            Self::Declined => "Declined",
            Self::Expired => "Expired",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Pending => Theme::WARNING,
            Self::Accepted => Theme::SUCCESS,
            Self::Declined => Theme::ERROR,
            Self::Expired | Self::Cancelled => Theme::LIST_TEXT_MUTED,
        }
    }
}

/// Type of invitation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationType {
    #[default]
    Guardian,
    Contact,
    Channel,
}

impl InvitationType {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Guardian => "◆",
            Self::Contact => "◯",
            Self::Channel => "◈",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Guardian => "Guardian Invitation",
            Self::Contact => "Contact Invitation",
            Self::Channel => "Channel Invitation",
        }
    }
}

/// An invitation presentation model.
#[derive(Clone, Debug, Default)]
pub struct Invitation {
    pub id: String,
    pub direction: InvitationDirection,
    pub other_party_id: String,
    pub other_party_name: String,
    pub invitation_type: InvitationType,
    pub status: InvitationStatus,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub message: Option<String>,
}

impl Invitation {
    pub fn new(
        id: impl Into<String>,
        other_party_name: impl Into<String>,
        direction: InvitationDirection,
    ) -> Self {
        Self {
            id: id.into(),
            other_party_name: other_party_name.into(),
            direction,
            ..Default::default()
        }
    }

    pub fn with_type(mut self, invitation_type: InvitationType) -> Self {
        self.invitation_type = invitation_type;
        self
    }

    pub fn with_status(mut self, status: InvitationStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

impl From<AppInvitationDirection> for InvitationDirection {
    fn from(direction: AppInvitationDirection) -> Self {
        match direction {
            AppInvitationDirection::Sent => Self::Outbound,
            AppInvitationDirection::Received => Self::Inbound,
        }
    }
}

impl From<AppInvitationStatus> for InvitationStatus {
    fn from(status: AppInvitationStatus) -> Self {
        match status {
            AppInvitationStatus::Pending => Self::Pending,
            AppInvitationStatus::Accepted => Self::Accepted,
            AppInvitationStatus::Rejected => Self::Declined,
            AppInvitationStatus::Expired => Self::Expired,
            AppInvitationStatus::Revoked => Self::Cancelled,
        }
    }
}

impl From<AppInvitationType> for InvitationType {
    fn from(invitation_type: AppInvitationType) -> Self {
        match invitation_type {
            AppInvitationType::Guardian => Self::Guardian,
            AppInvitationType::Chat => Self::Channel,
            AppInvitationType::Home => Self::Contact,
        }
    }
}

impl From<&AppInvitation> for Invitation {
    fn from(inv: &AppInvitation) -> Self {
        let (other_party_id, other_party_name) = match inv.direction {
            AppInvitationDirection::Sent => (
                inv.to_id
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                inv.to_name.clone().unwrap_or_default(),
            ),
            AppInvitationDirection::Received => (inv.from_id.to_string(), inv.from_name.clone()),
        };

        Self {
            id: inv.id.clone(),
            direction: inv.direction.into(),
            other_party_id,
            other_party_name,
            invitation_type: inv.invitation_type.into(),
            status: inv.status.into(),
            created_at: inv.created_at,
            expires_at: inv.expires_at,
            message: inv.message.clone(),
        }
    }
}
