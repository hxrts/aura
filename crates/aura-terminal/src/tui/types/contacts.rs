use aura_app::ui::types::Contact as AppContact;
use iocraft::prelude::Color;

use crate::tui::theme::Theme;

use super::shared::short_id;

pub use aura_app::ui::types::contacts::ReadReceiptPolicy;

/// Contact status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContactStatus {
    #[default]
    Active,
    Offline,
    Pending,
    Blocked,
}

impl ContactStatus {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Active => "●",
            Self::Offline => "○",
            Self::Pending => "○",
            Self::Blocked => "⊗",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Active => Theme::SUCCESS,
            Self::Offline => Theme::TEXT_DISABLED,
            Self::Pending => Theme::WARNING,
            Self::Blocked => Theme::ERROR,
        }
    }
}

pub trait ReadReceiptPolicyExt {
    fn label(self) -> &'static str;
    fn toggle(self) -> Self;
}

impl ReadReceiptPolicyExt for ReadReceiptPolicy {
    fn label(self) -> &'static str {
        match self {
            ReadReceiptPolicy::Disabled => "Disabled",
            ReadReceiptPolicy::Enabled => "Enabled",
        }
    }

    fn toggle(self) -> Self {
        match self {
            ReadReceiptPolicy::Disabled => ReadReceiptPolicy::Enabled,
            ReadReceiptPolicy::Enabled => ReadReceiptPolicy::Disabled,
        }
    }
}

/// A contact.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contact {
    pub id: String,
    pub nickname: String,
    pub nickname_suggestion: Option<String>,
    pub status: ContactStatus,
    pub is_guardian: bool,
    /// Read receipt policy for this contact.
    pub read_receipt_policy: ReadReceiptPolicy,
}

impl Contact {
    pub fn new(id: impl Into<String>, nickname: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            nickname: nickname.into(),
            ..Default::default()
        }
    }

    pub fn with_status(mut self, status: ContactStatus) -> Self {
        self.status = status;
        self
    }

    pub fn guardian(mut self) -> Self {
        self.is_guardian = true;
        self
    }

    pub fn with_suggestion(mut self, name: impl Into<String>) -> Self {
        self.nickname_suggestion = Some(name.into());
        self
    }
}

impl From<&AppContact> for Contact {
    fn from(c: &AppContact) -> Self {
        let nickname = if !c.nickname.is_empty() {
            c.nickname.clone()
        } else if let Some(suggested) = &c.nickname_suggestion {
            suggested.clone()
        } else {
            String::new()
        };

        Self {
            id: c.id.to_string(),
            nickname,
            nickname_suggestion: c.nickname_suggestion.clone(),
            status: if c.is_online {
                ContactStatus::Active
            } else {
                ContactStatus::Offline
            },
            is_guardian: c.is_guardian,
            read_receipt_policy: c.read_receipt_policy,
        }
    }
}

/// Format a display name for an authority ID using contact information.
pub fn format_contact_name(authority_id: &str, contacts: &[Contact]) -> String {
    if let Some(contact) = contacts.iter().find(|c| c.id == authority_id) {
        if !contact.nickname.is_empty() {
            return contact.nickname.clone();
        }
        if let Some(name) = contact.nickname_suggestion.as_ref() {
            if !name.is_empty() {
                return name.clone();
            }
        }
    }
    short_id(authority_id, 8)
}
