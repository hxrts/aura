//! Domain Fact Registry
//!
//! This module assembles the central registry for domain-specific fact types.
//!
//! # Protocol-Level vs Domain-Level Facts
//!
//! Aura's fact system has two categories:
//!
//! ## Protocol-Level Facts (in `aura-journal/src/fact.rs`)
//!
//! Core protocol constructs with complex reduction logic that stay in `aura-journal`:
//! - `GuardianBinding`, `RecoveryGrant`, `Consensus` (core protocol)
//! - `AmpChannelCheckpoint`, `AmpProposed/CommittedChannelEpochBump`, `AmpChannelPolicy` (AMP)
//!
//! These facts have specialized handling in `reduce_context()` and should NOT be migrated.
//!
//! ## Domain-Level Facts (registered here)
//!
//! Application-specific facts that use `RelationalFact::Generic` and are reduced by
//! `FactReducer` implementations in their respective domain crates:
//!
//! | Domain Crate | Fact Type | Purpose |
//! |-------------|-----------|---------|
//! | `aura-chat` | `ChatFact` | Channels, messages |
//! | `aura-invitation` | `InvitationFact` | Invitation lifecycle |
//! | `aura-relational` | `ContactFact` | Contact management |
//! | `aura-recovery` | `RecoveryFact` | Guardian setup, membership, key recovery |
//! | `aura-social/moderation` | `Block*Fact` | Block, mute, ban, kick |
//!
//! Domain crates implement the `DomainFact` trait and provide a `FactReducer`.

use aura_authentication::{AuthFact, AuthFactReducer, AUTH_FACT_TYPE_ID};
use aura_chat::{ChatFact, ChatFactReducer, CHAT_FACT_TYPE_ID};
use aura_invitation::{InvitationFact, InvitationFactReducer, INVITATION_FACT_TYPE_ID};
use aura_journal::FactRegistry;
use aura_social::moderation::register_moderation_facts;
use aura_recovery::{RecoveryFact, RecoveryFactReducer, RECOVERY_FACT_TYPE_ID};
use aura_relational::{
    ContactFact, ContactFactReducer, GuardianBindingDetailsFact, GuardianBindingDetailsFactReducer,
    GuardianRequestFact, GuardianRequestFactReducer, RecoveryGrantDetailsFact,
    RecoveryGrantDetailsFactReducer, CONTACT_FACT_TYPE_ID, GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID,
    GUARDIAN_REQUEST_FACT_TYPE_ID, RECOVERY_GRANT_DETAILS_FACT_TYPE_ID,
};
use aura_rendezvous::{RendezvousFact, RendezvousFactReducer, RENDEZVOUS_FACT_TYPE_ID};

/// Assembles the journal fact registry with all domain reducers.
///
/// This is the central registration point for domain-specific fact types.
/// Protocol-level facts (Guardian, Recovery, Consensus, AMP) are handled
/// directly in `aura-journal/src/reduction.rs` and don't need registration.
pub fn build_fact_registry() -> FactRegistry {
    let mut registry = FactRegistry::new();

    // Domain-level facts: application-specific, reduced via registered FactReducer
    registry.register::<ChatFact>(CHAT_FACT_TYPE_ID, Box::new(ChatFactReducer));
    registry.register::<AuthFact>(AUTH_FACT_TYPE_ID, Box::new(AuthFactReducer));
    registry.register::<InvitationFact>(INVITATION_FACT_TYPE_ID, Box::new(InvitationFactReducer));
    registry.register::<ContactFact>(CONTACT_FACT_TYPE_ID, Box::new(ContactFactReducer));
    registry.register::<GuardianRequestFact>(
        GUARDIAN_REQUEST_FACT_TYPE_ID,
        Box::new(GuardianRequestFactReducer),
    );
    registry.register::<GuardianBindingDetailsFact>(
        GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID,
        Box::new(GuardianBindingDetailsFactReducer),
    );
    registry.register::<RecoveryGrantDetailsFact>(
        RECOVERY_GRANT_DETAILS_FACT_TYPE_ID,
        Box::new(RecoveryGrantDetailsFactReducer),
    );
    registry.register::<RendezvousFact>(RENDEZVOUS_FACT_TYPE_ID, Box::new(RendezvousFactReducer));
    registry.register::<RecoveryFact>(RECOVERY_FACT_TYPE_ID, Box::new(RecoveryFactReducer));
    register_moderation_facts(&mut registry);

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_domain_fact_types() {
        let registry = build_fact_registry();
        assert!(registry.is_registered(CHAT_FACT_TYPE_ID));
        assert!(registry.is_registered(AUTH_FACT_TYPE_ID));
        assert!(registry.is_registered(INVITATION_FACT_TYPE_ID));
        assert!(registry.is_registered(CONTACT_FACT_TYPE_ID));
        assert!(registry.is_registered(GUARDIAN_REQUEST_FACT_TYPE_ID));
        assert!(registry.is_registered(GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID));
        assert!(registry.is_registered(RECOVERY_GRANT_DETAILS_FACT_TYPE_ID));
        assert!(registry.is_registered(RENDEZVOUS_FACT_TYPE_ID));
        assert!(registry.is_registered(RECOVERY_FACT_TYPE_ID));
        assert!(registry.is_registered("moderation:block-mute"));
        assert!(registry.is_registered("moderation:block-unmute"));
    }
}
