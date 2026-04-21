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

use crate::reducer_support::{hashed_generic_binding, physical_time_ms, reduce_typed_envelope};
use aura_core::relational::{GuardianBinding, RecoveryGrant};
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::{hash, Hash32};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Type identifier for contact facts
pub const CONTACT_FACT_TYPE_ID: &str = "contact";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

/// Pure contact-presence index derived from `ContactFact` events.
///
/// The index keeps only the active owner/contact pairs so runtime code can make
/// O(1) existence checks after an initial replay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactExistenceIndex {
    active_contacts: BTreeSet<(AuthorityId, AuthorityId)>,
}

impl ContactExistenceIndex {
    /// Create an empty contact-presence index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a contact fact to the index.
    pub fn apply_fact(&mut self, fact: &ContactFact) {
        match fact {
            ContactFact::Added {
                owner_id,
                contact_id,
                ..
            } => {
                self.active_contacts.insert((*owner_id, *contact_id));
            }
            ContactFact::Removed {
                owner_id,
                contact_id,
                ..
            } => {
                self.active_contacts.remove(&(*owner_id, *contact_id));
            }
            ContactFact::Renamed { .. } | ContactFact::ReadReceiptPolicyUpdated { .. } => {}
        }
    }

    /// Return whether `owner_id` currently has `contact_id` as an active contact.
    pub fn contains(&self, owner_id: AuthorityId, contact_id: AuthorityId) -> bool {
        self.active_contacts.contains(&(owner_id, contact_id))
    }
}

/// Read receipt policy for a contact
///
/// Determines whether read receipts are sent when viewing messages from this contact.
/// Privacy-first default is Disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReadReceiptPolicy {
    /// Do not send read receipts (privacy-first default)
    #[default]
    Disabled,
    /// Send read receipts when messages are viewed
    Enabled,
}

/// Contact domain fact types
///
/// These facts represent contact-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `ContactFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "contact", schema_version = 1, context = "context_id")]
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
        /// Invitation code that was used to establish this contact, if
        /// the link was established via invitation. Captured at the
        /// point of creation/acceptance so it survives across app
        /// restarts. `None` for contacts added through non-invitation
        /// paths.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invitation_code: Option<String>,
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
    /// Read receipt policy updated for a contact
    ReadReceiptPolicyUpdated {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority owning the contact list
        owner_id: AuthorityId,
        /// Contact whose policy is being updated
        contact_id: AuthorityId,
        /// New read receipt policy
        policy: ReadReceiptPolicy,
        /// Timestamp when policy was updated (uses unified time system)
        updated_at: PhysicalTime,
    },
}

impl ContactFact {
    /// Get the contact_id from any variant
    pub fn contact_id(&self) -> AuthorityId {
        match self {
            ContactFact::Added { contact_id, .. } => *contact_id,
            ContactFact::Removed { contact_id, .. } => *contact_id,
            ContactFact::Renamed { contact_id, .. } => *contact_id,
            ContactFact::ReadReceiptPolicyUpdated { contact_id, .. } => *contact_id,
        }
    }

    /// Get the owner_id from any variant
    pub fn owner_id(&self) -> AuthorityId {
        match self {
            ContactFact::Added { owner_id, .. } => *owner_id,
            ContactFact::Removed { owner_id, .. } => *owner_id,
            ContactFact::Renamed { owner_id, .. } => *owner_id,
            ContactFact::ReadReceiptPolicyUpdated { owner_id, .. } => *owner_id,
        }
    }

    /// Get the timestamp in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            ContactFact::Added { added_at, .. } => added_at.ts_ms,
            ContactFact::Removed { removed_at, .. } => removed_at.ts_ms,
            ContactFact::Renamed { renamed_at, .. } => renamed_at.ts_ms,
            ContactFact::ReadReceiptPolicyUpdated { updated_at, .. } => updated_at.ts_ms,
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
            added_at: physical_time_ms(added_at_ms),
            invitation_code: None,
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
            removed_at: physical_time_ms(removed_at_ms),
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
            renamed_at: physical_time_ms(renamed_at_ms),
        }
    }

    /// Create a ReadReceiptPolicyUpdated fact with millisecond timestamp
    pub fn read_receipt_policy_updated_ms(
        context_id: ContextId,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
        policy: ReadReceiptPolicy,
        updated_at_ms: u64,
    ) -> Self {
        Self::ReadReceiptPolicyUpdated {
            context_id,
            owner_id,
            contact_id,
            policy,
            updated_at: physical_time_ms(updated_at_ms),
        }
    }
}

impl ContactFact {
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
    }

    pub fn binding_key(&self) -> ContactFactKey {
        match self {
            ContactFact::Added { contact_id, .. } => ContactFactKey {
                sub_type: "contact-added",
                data: contact_id.to_bytes().to_vec(),
            },
            ContactFact::Removed { contact_id, .. } => ContactFactKey {
                sub_type: "contact-removed",
                data: contact_id.to_bytes().to_vec(),
            },
            ContactFact::Renamed { contact_id, .. } => ContactFactKey {
                sub_type: "contact-renamed",
                data: contact_id.to_bytes().to_vec(),
            },
            ContactFact::ReadReceiptPolicyUpdated { contact_id, .. } => ContactFactKey {
                sub_type: "contact-read-receipt-policy",
                data: contact_id.to_bytes().to_vec(),
            },
        }
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

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_typed_envelope::<ContactFact>(
            context_id,
            envelope,
            CONTACT_FACT_TYPE_ID,
            |fact| fact.validate_for_reduction(context_id),
            |fact| {
                let key = fact.binding_key();
                RelationalBinding {
                    binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
                    context_id,
                    data: key.data,
                }
            },
        )
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(
    type_id = "guardian_binding_details",
    schema_version = 1,
    context = "context_id"
)]
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

    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id == context_id
    }
}

pub struct GuardianBindingDetailsFactReducer;

impl FactReducer for GuardianBindingDetailsFactReducer {
    fn handles_type(&self) -> &'static str {
        GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_typed_envelope::<GuardianBindingDetailsFact>(
            context_id,
            envelope,
            GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID,
            |fact| fact.validate_for_reduction(context_id),
            |_| hashed_generic_binding("guardian-binding-details", context_id, &envelope.payload),
        )
    }
}

/// Type identifier for recovery grant detail facts.
///
/// These facts store the full `RecoveryGrant` payload as `RelationalFact::Generic`.
pub const RECOVERY_GRANT_DETAILS_FACT_TYPE_ID: &str = "recovery_grant_details";

/// Stored recovery grant details for a relational context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(
    type_id = "recovery_grant_details",
    schema_version = 1,
    context = "context_id"
)]
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
        Hash32::from_bytes(&hash::hash(&self.to_envelope().payload))
    }

    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id == context_id
    }
}

pub struct RecoveryGrantDetailsFactReducer;

impl FactReducer for RecoveryGrantDetailsFactReducer {
    fn handles_type(&self) -> &'static str {
        RECOVERY_GRANT_DETAILS_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_typed_envelope::<RecoveryGrantDetailsFact>(
            context_id,
            envelope,
            RECOVERY_GRANT_DETAILS_FACT_TYPE_ID,
            |fact| fact.validate_for_reduction(context_id),
            |_| hashed_generic_binding("recovery-grant-details", context_id, &envelope.payload),
        )
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

    /// ContactFact survives to_bytes/from_bytes roundtrip without loss.
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

        if let aura_journal::RelationalFact::Generic { envelope, .. } = generic {
            assert_eq!(envelope.type_id.as_str(), CONTACT_FACT_TYPE_ID);
            let restored = ContactFact::from_envelope(&envelope);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn contact_added_roundtrips_invitation_code() {
        let fact = ContactFact::Added {
            context_id: test_context_id(),
            owner_id: test_authority_id(1),
            contact_id: test_authority_id(2),
            nickname: "Alice".to_string(),
            added_at: physical_time_ms(1234567890),
            invitation_code: Some("aura:v1:ABC123".to_string()),
        };

        let envelope = fact.to_envelope();
        let Some(restored) = ContactFact::from_envelope(&envelope) else {
            panic!("roundtrip decode should succeed");
        };

        assert_eq!(fact, restored);
        if let ContactFact::Added {
            invitation_code, ..
        } = restored
        {
            assert_eq!(invitation_code, Some("aura:v1:ABC123".to_string()));
        } else {
            panic!("Expected Added variant");
        }
    }

    #[test]
    fn contact_added_without_code_is_none_and_backward_compatible() {
        // Using the legacy helper (no invitation_code parameter) yields
        // None. Also verifies that the envelope encoding of a fact with
        // invitation_code=None matches — important for ensuring that
        // pre-existing journal data stays loadable after the field was
        // added.
        let fact = ContactFact::added_with_timestamp_ms(
            test_context_id(),
            test_authority_id(1),
            test_authority_id(2),
            "Alice".to_string(),
            1234567890,
        );
        match &fact {
            ContactFact::Added {
                invitation_code, ..
            } => assert!(invitation_code.is_none()),
            other => panic!("expected Added, got {other:?}"),
        }

        let envelope = fact.to_envelope();
        let Some(restored) = ContactFact::from_envelope(&envelope) else {
            panic!("roundtrip decode should succeed");
        };
        assert_eq!(fact, restored);
    }

    /// Reducing the same fact twice produces identical bindings — needed
    /// for replay-safe journal reduction.
    #[test]
    fn test_contact_reducer_idempotence() {
        let reducer = ContactFactReducer;
        let context_id = test_context_id();
        let fact = ContactFact::added_with_timestamp_ms(
            context_id,
            test_authority_id(1),
            test_authority_id(2),
            "Alice".to_string(),
            1234567890,
        );

        let envelope = fact.to_envelope();
        let binding1 = reducer.reduce_envelope(context_id, &envelope);
        let binding2 = reducer.reduce_envelope(context_id, &envelope);
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
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

        let envelope = fact.to_envelope();
        let binding = reducer.reduce_envelope(test_context_id(), &envelope);

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

    /// Reducer rejects contact facts whose context_id doesn't match the
    /// reduction context. If broken, contact changes from one relationship
    /// affect another.
    #[test]
    fn test_contact_reducer_rejects_context_mismatch() {
        let reducer = ContactFactReducer;
        let fact = ContactFact::added_with_timestamp_ms(
            test_context_id(),
            test_authority_id(1),
            test_authority_id(2),
            "Alice".to_string(),
            0,
        );

        let other_context = ContextId::new_from_entropy([99u8; 32]);
        let envelope = fact.to_envelope();
        let binding = reducer.reduce_envelope(other_context, &envelope);
        assert!(
            binding.is_none(),
            "Contact fact with mismatched context must be rejected"
        );
    }

    /// Guardian binding reducer rejects facts with mismatched context — prevents
    /// guardian bindings from one relationship leaking into another.
    #[test]
    fn test_guardian_binding_reducer_rejects_context_mismatch() {
        let reducer = GuardianBindingDetailsFactReducer;
        let ctx = test_context_id();
        let account = test_authority_id(1);
        let guardian = test_authority_id(2);
        let binding = GuardianBinding::new(
            Hash32::from_bytes(&hash::hash(&account.to_bytes())),
            Hash32::from_bytes(&hash::hash(&guardian.to_bytes())),
            aura_core::relational::GuardianParameters::default(),
        );

        let fact = GuardianBindingDetailsFact::new(ctx, account, guardian, binding);
        let envelope = fact.to_envelope();

        let other_context = ContextId::new_from_entropy([99u8; 32]);
        let result = reducer.reduce_envelope(other_context, &envelope);
        assert!(
            result.is_none(),
            "Guardian binding fact with mismatched context must be rejected"
        );
    }

    #[test]
    fn contact_existence_index_tracks_add_and_remove() {
        let owner = test_authority_id(1);
        let contact = test_authority_id(2);
        let mut index = ContactExistenceIndex::new();

        index.apply_fact(&ContactFact::added_with_timestamp_ms(
            test_context_id(),
            owner,
            contact,
            "Alice".to_string(),
            1,
        ));
        assert!(index.contains(owner, contact));

        index.apply_fact(&ContactFact::renamed_with_timestamp_ms(
            test_context_id(),
            owner,
            contact,
            "Bob".to_string(),
            2,
        ));
        assert!(index.contains(owner, contact));

        index.apply_fact(&ContactFact::removed_with_timestamp_ms(
            test_context_id(),
            owner,
            contact,
            3,
        ));
        assert!(!index.contains(owner, contact));
    }

    /// GuardianBindingDetailsFact survives serialization roundtrip with all
    /// fields preserved — account, guardian, and binding parameters.
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
