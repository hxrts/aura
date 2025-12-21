//! Contact and relationship domain facts
//!
//! This module defines contact-specific fact types that implement the `DomainFact`
//! trait from `aura-journal`. These facts are stored as `RelationalFact::Generic`
//! in the journal and reduced using the `ContactFactReducer`.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - `aura-relational` defines domain-specific fact types without modifying `aura-journal`
//! - Runtime registers `ContactFactReducer` with the `FactRegistry`
//!
//! # Example
//!
//! ```ignore
//! use aura_relational::facts::{ContactFact, ContactFactReducer};
//! use aura_journal::{FactRegistry, DomainFact};
//!
//! // Create a contact fact
//! let fact = ContactFact::Added {
//!     context_id,
//!     owner_id,
//!     contact_id,
//!     nickname: "Alice".to_string(),
//!     added_at_ms: 1234567890,
//! };
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Register reducer at runtime
//! registry.register::<ContactFact>("contact", Box::new(ContactFactReducer));
//! ```

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::relational::{GuardianBinding, RecoveryGrant};
use aura_core::time::PhysicalTime;
use aura_core::{hash, Hash32};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use serde::{Deserialize, Serialize};

/// Type identifier for contact facts
pub const CONTACT_FACT_TYPE_ID: &str = "contact";

/// Contact domain fact types
///
/// These facts represent contact-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `ContactFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContactFact {
    /// Contact added to authority's contact list
    Added {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority adding the contact (owner)
        owner_id: AuthorityId,
        /// Authority being added as a contact
        contact_id: AuthorityId,
        /// User-assigned nickname for the contact
        nickname: String,
        /// Timestamp when contact was added (uses unified time system)
        added_at: PhysicalTime,
    },
    /// Contact removed from authority's contact list
    Removed {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority removing the contact (owner)
        owner_id: AuthorityId,
        /// Authority being removed as a contact
        contact_id: AuthorityId,
        /// Timestamp when contact was removed (uses unified time system)
        removed_at: PhysicalTime,
    },
    /// Contact nickname updated
    Renamed {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority owning the contact list
        owner_id: AuthorityId,
        /// Contact being renamed
        contact_id: AuthorityId,
        /// New nickname for the contact
        new_nickname: String,
        /// Timestamp when contact was renamed (uses unified time system)
        renamed_at: PhysicalTime,
    },
}

impl ContactFact {
    /// Get the contact_id from any variant
    pub fn contact_id(&self) -> AuthorityId {
        match self {
            ContactFact::Added { contact_id, .. } => *contact_id,
            ContactFact::Removed { contact_id, .. } => *contact_id,
            ContactFact::Renamed { contact_id, .. } => *contact_id,
        }
    }

    /// Get the owner_id from any variant
    pub fn owner_id(&self) -> AuthorityId {
        match self {
            ContactFact::Added { owner_id, .. } => *owner_id,
            ContactFact::Removed { owner_id, .. } => *owner_id,
            ContactFact::Renamed { owner_id, .. } => *owner_id,
        }
    }

    /// Get the timestamp in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            ContactFact::Added { added_at, .. } => added_at.ts_ms,
            ContactFact::Removed { removed_at, .. } => removed_at.ts_ms,
            ContactFact::Renamed { renamed_at, .. } => renamed_at.ts_ms,
        }
    }

    /// Create an Added fact with millisecond timestamp (backward compatibility)
    pub fn added_with_timestamp_ms(
        context_id: ContextId,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
        nickname: String,
        added_at_ms: u64,
    ) -> Self {
        Self::Added {
            context_id,
            owner_id,
            contact_id,
            nickname,
            added_at: PhysicalTime {
                ts_ms: added_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a Removed fact with millisecond timestamp (backward compatibility)
    pub fn removed_with_timestamp_ms(
        context_id: ContextId,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
        removed_at_ms: u64,
    ) -> Self {
        Self::Removed {
            context_id,
            owner_id,
            contact_id,
            removed_at: PhysicalTime {
                ts_ms: removed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a Renamed fact with millisecond timestamp (backward compatibility)
    pub fn renamed_with_timestamp_ms(
        context_id: ContextId,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
        new_nickname: String,
        renamed_at_ms: u64,
    ) -> Self {
        Self::Renamed {
            context_id,
            owner_id,
            contact_id,
            new_nickname,
            renamed_at: PhysicalTime {
                ts_ms: renamed_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for ContactFact {
    fn type_id(&self) -> &'static str {
        CONTACT_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            ContactFact::Added { context_id, .. } => *context_id,
            ContactFact::Removed { context_id, .. } => *context_id,
            ContactFact::Renamed { context_id, .. } => *context_id,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("ContactFact must serialize")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        serde_json::from_slice(bytes).ok()
    }
}

/// Reducer for contact facts
///
/// Converts contact facts to relational bindings during journal reduction.
pub struct ContactFactReducer;

impl FactReducer for ContactFactReducer {
    fn handles_type(&self) -> &'static str {
        CONTACT_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != CONTACT_FACT_TYPE_ID {
            return None;
        }

        let fact: ContactFact = serde_json::from_slice(binding_data).ok()?;

        let (sub_type, data) = match &fact {
            ContactFact::Added { contact_id, .. } => {
                ("contact-added".to_string(), contact_id.to_bytes().to_vec())
            }
            ContactFact::Removed { contact_id, .. } => (
                "contact-removed".to_string(),
                contact_id.to_bytes().to_vec(),
            ),
            ContactFact::Renamed { contact_id, .. } => (
                "contact-renamed".to_string(),
                contact_id.to_bytes().to_vec(),
            ),
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub_type),
            context_id,
            data,
        })
    }
}

// =============================================================================
// Relational detail facts (guardian bindings, recovery grants)
// =============================================================================

/// Type identifier for guardian binding detail facts.
///
/// These facts store the full `GuardianBinding` payload as `RelationalFact::Generic`.
pub const GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID: &str = "guardian_binding_details";

/// Stored guardian binding details for a relational context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianBindingDetailsFact {
    pub context_id: ContextId,
    pub account_id: AuthorityId,
    pub guardian_id: AuthorityId,
    pub binding: GuardianBinding,
}

impl GuardianBindingDetailsFact {
    pub fn new(
        context_id: ContextId,
        account_id: AuthorityId,
        guardian_id: AuthorityId,
        binding: GuardianBinding,
    ) -> Self {
        Self {
            context_id,
            account_id,
            guardian_id,
            binding,
        }
    }
}

impl DomainFact for GuardianBindingDetailsFact {
    fn type_id(&self) -> &'static str {
        GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("GuardianBindingDetailsFact must serialize")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        bincode::deserialize(bytes).ok()
    }
}

pub struct GuardianBindingDetailsFactReducer;

impl FactReducer for GuardianBindingDetailsFactReducer {
    fn handles_type(&self) -> &'static str {
        GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID {
            return None;
        }

        let fact: GuardianBindingDetailsFact = bincode::deserialize(binding_data).ok()?;
        if fact.context_id != context_id {
            return None;
        }

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic("guardian-binding-details".to_string()),
            context_id,
            data: hash::hash(binding_data).to_vec(),
        })
    }
}

/// Type identifier for recovery grant detail facts.
///
/// These facts store the full `RecoveryGrant` payload as `RelationalFact::Generic`.
pub const RECOVERY_GRANT_DETAILS_FACT_TYPE_ID: &str = "recovery_grant_details";

/// Stored recovery grant details for a relational context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryGrantDetailsFact {
    pub context_id: ContextId,
    pub account_id: AuthorityId,
    pub grant: RecoveryGrant,
}

impl RecoveryGrantDetailsFact {
    pub fn new(context_id: ContextId, account_id: AuthorityId, grant: RecoveryGrant) -> Self {
        Self {
            context_id,
            account_id,
            grant,
        }
    }

    pub fn grant_hash(&self) -> Hash32 {
        Hash32::from_bytes(&hash::hash(&self.to_bytes()))
    }
}

impl DomainFact for RecoveryGrantDetailsFact {
    fn type_id(&self) -> &'static str {
        RECOVERY_GRANT_DETAILS_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("RecoveryGrantDetailsFact must serialize")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        bincode::deserialize(bytes).ok()
    }
}

pub struct RecoveryGrantDetailsFactReducer;

impl FactReducer for RecoveryGrantDetailsFactReducer {
    fn handles_type(&self) -> &'static str {
        RECOVERY_GRANT_DETAILS_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != RECOVERY_GRANT_DETAILS_FACT_TYPE_ID {
            return None;
        }

        let fact: RecoveryGrantDetailsFact = bincode::deserialize(binding_data).ok()?;
        if fact.context_id != context_id {
            return None;
        }

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic("recovery-grant-details".to_string()),
            context_id,
            data: hash::hash(binding_data).to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_contact_fact_serialization() {
        let fact = ContactFact::added_with_timestamp_ms(
            test_context_id(),
            test_authority_id(1),
            test_authority_id(2),
            "Alice".to_string(),
            1234567890,
        );

        let bytes = fact.to_bytes();
        let restored = ContactFact::from_bytes(&bytes);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn test_contact_fact_to_generic() {
        let fact = ContactFact::renamed_with_timestamp_ms(
            test_context_id(),
            test_authority_id(1),
            test_authority_id(2),
            "Bob".to_string(),
            1234567899,
        );

        let generic = fact.to_generic();

        if let aura_journal::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        {
            assert_eq!(binding_type, CONTACT_FACT_TYPE_ID);
            let restored = ContactFact::from_bytes(&binding_data);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_contact_fact_reducer() {
        let reducer = ContactFactReducer;
        assert_eq!(reducer.handles_type(), CONTACT_FACT_TYPE_ID);

        let fact = ContactFact::added_with_timestamp_ms(
            test_context_id(),
            test_authority_id(1),
            test_authority_id(2),
            "Test".to_string(),
            0,
        );

        let bytes = fact.to_bytes();
        let binding = reducer.reduce(test_context_id(), CONTACT_FACT_TYPE_ID, &bytes);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert!(matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref s) if s == "contact-added"
        ));
    }

    #[test]
    fn test_contact_id_extraction() {
        let contact = test_authority_id(99);

        let facts = vec![
            ContactFact::added_with_timestamp_ms(
                test_context_id(),
                test_authority_id(1),
                contact,
                "Alice".to_string(),
                0,
            ),
            ContactFact::removed_with_timestamp_ms(
                test_context_id(),
                test_authority_id(1),
                contact,
                0,
            ),
            ContactFact::renamed_with_timestamp_ms(
                test_context_id(),
                test_authority_id(1),
                contact,
                "Bob".to_string(),
                0,
            ),
        ];

        for fact in facts {
            assert_eq!(fact.contact_id(), contact);
        }
    }

    #[test]
    fn test_type_id_consistency() {
        let facts: Vec<ContactFact> = vec![
            ContactFact::added_with_timestamp_ms(
                test_context_id(),
                test_authority_id(1),
                test_authority_id(2),
                "x".to_string(),
                0,
            ),
            ContactFact::removed_with_timestamp_ms(
                test_context_id(),
                test_authority_id(1),
                test_authority_id(2),
                0,
            ),
            ContactFact::renamed_with_timestamp_ms(
                test_context_id(),
                test_authority_id(1),
                test_authority_id(2),
                "y".to_string(),
                0,
            ),
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), CONTACT_FACT_TYPE_ID);
        }
    }

    #[test]
    fn guardian_binding_details_roundtrip() {
        let ctx = test_context_id();
        let account = test_authority_id(1);
        let guardian = test_authority_id(2);
        let binding = GuardianBinding::new(
            Hash32::from_bytes(&hash::hash(&account.to_bytes())),
            Hash32::from_bytes(&hash::hash(&guardian.to_bytes())),
            aura_core::relational::GuardianParameters::default(),
        );

        let fact = GuardianBindingDetailsFact::new(ctx, account, guardian, binding);
        let bytes = fact.to_bytes();
        let restored = GuardianBindingDetailsFact::from_bytes(&bytes).unwrap();
        assert_eq!(restored.context_id, ctx);
        assert_eq!(restored.account_id, account);
        assert_eq!(restored.guardian_id, guardian);
    }
}
