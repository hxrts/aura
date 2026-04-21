use aura_app::ui::types::EffectiveName;
use aura_app::ui::types::{Contact as AppContact, ContactRelationshipState};
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
    pub relationship_state: ContactRelationshipState,
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

impl EffectiveName for Contact {
    fn nickname(&self) -> Option<&str> {
        (!self.nickname.is_empty()).then_some(self.nickname.as_str())
    }

    fn nickname_suggestion(&self) -> Option<&str> {
        self.nickname_suggestion
            .as_deref()
            .filter(|value| !value.is_empty())
    }

    fn fallback_id(&self) -> String {
        short_id(&self.id, 8)
    }
}

impl From<&AppContact> for Contact {
    fn from(c: &AppContact) -> Self {
        Self {
            id: c.id.to_string(),
            nickname: c.nickname.clone(),
            nickname_suggestion: c.nickname_suggestion.clone(),
            status: if c.relationship_state.is_pending() {
                ContactStatus::Pending
            } else if c.is_online {
                ContactStatus::Active
            } else {
                ContactStatus::Offline
            },
            is_guardian: c.is_guardian,
            relationship_state: c.relationship_state,
            read_receipt_policy: c.read_receipt_policy,
        }
    }
}

/// Format a display name for an authority ID using contact information.
pub fn format_contact_name(authority_id: &str, contacts: &[Contact]) -> String {
    if let Some(contact) = contacts.iter().find(|c| c.id == authority_id) {
        return contact.effective_name();
    }
    short_id(authority_id, 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui::types::ContactRelationshipState;
    use aura_core::types::identifiers::AuthorityId;

    fn app_contact(
        authority_id: AuthorityId,
        nickname: &str,
        nickname_suggestion: Option<&str>,
    ) -> AppContact {
        AppContact {
            id: authority_id,
            nickname: nickname.to_string(),
            nickname_suggestion: nickname_suggestion.map(str::to_string),
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: ReadReceiptPolicy::default(),
            relationship_state: ContactRelationshipState::Contact,
            invitation_code: None,
        }
    }

    #[test]
    fn app_contact_conversion_preserves_nickname_and_suggestion_distinction() {
        let authority_id = AuthorityId::new_from_entropy([9u8; 32]);
        let converted = Contact::from(&app_contact(authority_id, "", Some("Suggested")));

        assert!(converted.nickname.is_empty());
        assert_eq!(converted.nickname_suggestion.as_deref(), Some("Suggested"));
        assert_eq!(converted.effective_name(), "Suggested");
    }

    #[test]
    fn format_contact_name_uses_shared_effective_name_fallback() {
        let suggestion_only = AuthorityId::new_from_entropy([10u8; 32]);
        let fallback_only = AuthorityId::new_from_entropy([11u8; 32]);
        let contacts = vec![
            Contact::from(&app_contact(suggestion_only, "", Some("Suggested"))),
            Contact::from(&app_contact(fallback_only, "", None)),
        ];

        assert_eq!(
            format_contact_name(&suggestion_only.to_string(), &contacts),
            "Suggested"
        );
        assert_eq!(
            format_contact_name(&fallback_only.to_string(), &contacts),
            short_id(&fallback_only.to_string(), 8)
        );
    }
}
