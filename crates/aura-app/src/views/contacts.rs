//! # Contacts View State

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export ReadReceiptPolicy from aura-relational for convenience
pub use aura_relational::ReadReceiptPolicy;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Whether this contact is a home resident
    pub is_resident: bool,
    /// Last interaction time (ms since epoch)
    pub last_interaction: Option<u64>,
    /// Whether contact is online
    pub is_online: bool,
    /// Read receipt policy for this contact (privacy-first: disabled by default)
    pub read_receipt_policy: ReadReceiptPolicy,
}

// =============================================================================
// Serde Helper for HashMap<AuthorityId, Contact>
// =============================================================================

mod contact_map_serde {
    use super::{AuthorityId, Contact, HashMap};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        map: &HashMap<AuthorityId, Contact>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let vec: Vec<&Contact> = map.values().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<AuthorityId, Contact>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<Contact> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().map(|c| (c.id, c)).collect())
    }
}

// =============================================================================
// ContactsState
// =============================================================================

/// Contacts state
///
/// Stores contacts in a HashMap for O(1) lookup by ID. Selection and filtering
/// are UI concerns and should be stored in TuiState, not here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ContactsState {
    /// All contacts, keyed by authority ID
    #[serde(with = "contact_map_serde")]
    contacts: HashMap<AuthorityId, Contact>,
}

impl ContactsState {
    // =========================================================================
    // Constructors
    // =========================================================================

    /// Create a new empty contacts state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from a collection of contacts.
    pub fn from_contacts(contacts: impl IntoIterator<Item = Contact>) -> Self {
        Self {
            contacts: contacts.into_iter().map(|c| (c.id, c)).collect(),
        }
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get contact by ID.
    pub fn contact(&self, id: &AuthorityId) -> Option<&Contact> {
        self.contacts.get(id)
    }

    /// Get mutable contact by ID.
    pub fn contact_mut(&mut self, id: &AuthorityId) -> Option<&mut Contact> {
        self.contacts.get_mut(id)
    }

    /// Get all contacts as an iterator.
    pub fn all_contacts(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.values()
    }

    /// Get all contacts as a mutable iterator.
    pub fn all_contacts_mut(&mut self) -> impl Iterator<Item = &mut Contact> {
        self.contacts.values_mut()
    }

    /// Get all contact IDs.
    pub fn contact_ids(&self) -> impl Iterator<Item = &AuthorityId> {
        self.contacts.keys()
    }

    /// Get the number of contacts.
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Check if a contact exists.
    pub fn has_contact(&self, id: &AuthorityId) -> bool {
        self.contacts.contains_key(id)
    }

    /// Check if there are no contacts.
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    /// Filter contacts by a search term.
    ///
    /// Matches against nickname and suggested_name (case-insensitive).
    /// Returns all contacts if filter is empty.
    pub fn filter_by(&self, filter: &str) -> Vec<&Contact> {
        if filter.is_empty() {
            return self.contacts.values().collect();
        }
        let filter_lower = filter.to_lowercase();
        self.contacts
            .values()
            .filter(|c| {
                c.nickname.to_lowercase().contains(&filter_lower)
                    || c.suggested_name
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&filter_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Get guardian contacts.
    pub fn guardians(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.values().filter(|c| c.is_guardian)
    }

    /// Get resident contacts.
    pub fn residents(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.values().filter(|c| c.is_resident)
    }

    /// Get display name for a contact.
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

    /// Get read receipt policy for a contact.
    ///
    /// Returns Disabled (privacy-first default) if contact not found.
    pub fn get_read_receipt_policy(&self, contact_id: &AuthorityId) -> ReadReceiptPolicy {
        self.contact(contact_id)
            .map(|c| c.read_receipt_policy)
            .unwrap_or_default()
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Apply a contact (upsert semantics).
    pub fn apply_contact(&mut self, contact: Contact) {
        self.contacts.insert(contact.id, contact);
    }

    /// Remove a contact.
    pub fn remove_contact(&mut self, id: &AuthorityId) -> Option<Contact> {
        self.contacts.remove(id)
    }

    /// Update a contact if it exists.
    pub fn update_contact(&mut self, id: &AuthorityId, f: impl FnOnce(&mut Contact)) -> bool {
        if let Some(contact) = self.contacts.get_mut(id) {
            f(contact);
            true
        } else {
            false
        }
    }

    /// Set guardian status for a contact.
    ///
    /// Updates the is_guardian flag on the contact if it exists.
    pub fn set_guardian_status(&mut self, contact_id: &AuthorityId, is_guardian: bool) {
        self.update_contact(contact_id, |c| c.is_guardian = is_guardian);
    }

    /// Set nickname for a contact.
    ///
    /// If the contact doesn't exist, creates a new contact entry.
    pub fn set_nickname(&mut self, target: AuthorityId, nickname: String) {
        if let Some(contact) = self.contacts.get_mut(&target) {
            contact.nickname = nickname;
        } else {
            // Create new contact with the nickname
            self.contacts.insert(
                target,
                Contact {
                    id: target,
                    nickname,
                    suggested_name: None,
                    is_guardian: false,
                    is_resident: false,
                    last_interaction: None,
                    is_online: false,
                    read_receipt_policy: ReadReceiptPolicy::default(),
                },
            );
        }
    }

    /// Set read receipt policy for a contact.
    ///
    /// If the contact doesn't exist, this is a no-op.
    pub fn set_read_receipt_policy(&mut self, contact_id: &AuthorityId, policy: ReadReceiptPolicy) {
        self.update_contact(contact_id, |c| c.read_receipt_policy = policy);
    }

    /// Clear all contacts.
    pub fn clear(&mut self) {
        self.contacts.clear();
    }
}
