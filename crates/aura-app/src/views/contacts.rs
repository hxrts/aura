//! # Contacts View State

use serde::{Deserialize, Serialize};

/// A contact
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Contact {
    /// Contact identifier (authority ID)
    pub id: String,
    /// Petname (user-assigned name)
    pub petname: String,
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
    pub selected_contact_id: Option<String>,
    /// Search filter
    pub search_filter: Option<String>,
}

impl ContactsState {
    /// Get contact by ID
    pub fn contact(&self, id: &str) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == id)
    }

    /// Get contacts matching search filter
    pub fn filtered_contacts(&self) -> Vec<&Contact> {
        match &self.search_filter {
            Some(filter) if !filter.is_empty() => {
                let filter_lower = filter.to_lowercase();
                self.contacts
                    .iter()
                    .filter(|c| {
                        c.petname.to_lowercase().contains(&filter_lower)
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
    /// Returns petname if set, otherwise suggested_name, otherwise the ID as fallback.
    pub fn get_display_name(&self, id: &str) -> String {
        if let Some(contact) = self.contact(id) {
            if !contact.petname.is_empty() {
                return contact.petname.clone();
            }
            if let Some(name) = &contact.suggested_name {
                return name.clone();
            }
        }
        id.to_string()
    }

    /// Set petname for a contact
    ///
    /// If the contact doesn't exist, creates a new contact entry.
    pub fn set_petname(&mut self, target: String, petname: String) {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == target) {
            contact.petname = petname;
        } else {
            // Create new contact with the petname
            self.contacts.push(Contact {
                id: target,
                petname,
                suggested_name: None,
                is_guardian: false,
                is_resident: false,
                last_interaction: None,
                is_online: false,
            });
        }
    }
}
