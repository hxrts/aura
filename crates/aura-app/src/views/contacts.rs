//! # Contacts View State

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

// =============================================================================
// Contact Suggestion Types
// =============================================================================

/// Policy for handling incoming contact name suggestions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SuggestionPolicy {
    /// Automatically accept contact suggestions
    #[default]
    AutoAccept,
    /// Prompt the user before accepting suggestions
    PromptFirst,
    /// Ignore incoming suggestions
    Ignore,
}

/// What the user shares about themselves to contacts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MySuggestion {
    /// Display name to share
    pub display_name: Option<String>,
    /// Status message to share
    pub status: Option<String>,
}

// =============================================================================
// Contact Types
// =============================================================================

/// A contact
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Contact {
    /// Contact identifier (authority ID)
    pub id: AuthorityId,
    /// Nickname (user-assigned name)
    pub nickname: String,
    /// Suggested name (from contact or system)
    pub suggested_name: Option<String>,
    /// Whether this contact is a guardian
    pub is_guardian: bool,
    /// Whether this contact is a block resident
    pub is_resident: bool,
    /// Last interaction time (ms since epoch)
    pub last_interaction: Option<u64>,
    /// Whether contact is online
    pub is_online: bool,
}

/// Contacts state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ContactsState {
    /// All contacts
    pub contacts: Vec<Contact>,
    /// Currently selected contact ID
    pub selected_contact_id: Option<AuthorityId>,
    /// Search filter
    pub search_filter: Option<String>,
}

impl ContactsState {
    /// Get contact by ID
    pub fn contact(&self, id: &AuthorityId) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == *id)
    }

    /// Get contacts matching search filter
    pub fn filtered_contacts(&self) -> Vec<&Contact> {
        match &self.search_filter {
            Some(filter) if !filter.is_empty() => {
                let filter_lower = filter.to_lowercase();
                self.contacts
                    .iter()
                    .filter(|c| {
                        c.nickname.to_lowercase().contains(&filter_lower)
                            || c.suggested_name
                                .as_ref()
                                .map(|n| n.to_lowercase().contains(&filter_lower))
                                .unwrap_or(false)
                    })
                    .collect()
            }
            _ => self.contacts.iter().collect(),
        }
    }

    /// Get guardian contacts
    pub fn guardians(&self) -> Vec<&Contact> {
        self.contacts.iter().filter(|c| c.is_guardian).collect()
    }

    /// Get resident contacts
    pub fn residents(&self) -> Vec<&Contact> {
        self.contacts.iter().filter(|c| c.is_resident).collect()
    }

    /// Get display name for a contact
    ///
    /// Returns nickname if set, otherwise suggested_name, otherwise the ID as fallback.
    pub fn get_display_name(&self, id: &AuthorityId) -> String {
        if let Some(contact) = self.contact(id) {
            if !contact.nickname.is_empty() {
                return contact.nickname.clone();
            }
            if let Some(name) = &contact.suggested_name {
                return name.clone();
            }
        }
        id.to_string()
    }

    /// Set nickname for a contact
    ///
    /// If the contact doesn't exist, creates a new contact entry.
    pub fn set_nickname(&mut self, target: AuthorityId, nickname: String) {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == target) {
            contact.nickname = nickname;
        } else {
            // Create new contact with the nickname
            self.contacts.push(Contact {
                id: target,
                nickname,
                suggested_name: None,
                is_guardian: false,
                is_resident: false,
                last_interaction: None,
                is_online: false,
            });
        }
    }
}
