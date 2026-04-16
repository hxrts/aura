use crate::model::UiController;
use aura_app::signal_defs::DiscoveredPeersState;
use aura_app::ui::contract::ConfirmationState;
use aura_app::ui::signals::{CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL};
use aura_app::ui::types::{ContactRelationshipState, ContactsState};
use aura_app::ui_contract::RuntimeFact;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::AuthorityId;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct ContactsRuntimeContact {
    pub(in crate::app) authority_id: AuthorityId,
    pub(in crate::app) name: String,
    pub(in crate::app) nickname_hint: Option<String>,
    pub(in crate::app) is_guardian: bool,
    pub(in crate::app) is_member: bool,
    pub(in crate::app) is_online: bool,
    pub(in crate::app) relationship_state: ContactRelationshipState,
    pub(in crate::app) confirmation: ConfirmationState,
    /// Invitation code that established this contact, from the
    /// authoritative Contact view. Sourced from
    /// `ContactFact::Added.invitation_code`; `None` for contacts added
    /// through non-invitation paths or legacy facts.
    pub(in crate::app) invitation_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct ContactsRuntimePeer {
    pub(in crate::app) authority_id: AuthorityId,
    pub(in crate::app) address: String,
    pub(in crate::app) invited: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ContactsRuntimeView {
    pub(in crate::app) loaded: bool,
    pub(in crate::app) contacts: Vec<ContactsRuntimeContact>,
    pub(in crate::app) lan_peers: Vec<ContactsRuntimePeer>,
}

fn display_contact_name(contact: &aura_app::ui::types::Contact) -> String {
    if !contact.nickname.trim().is_empty() {
        return contact.nickname.clone();
    }
    if let Some(suggestion) = contact
        .nickname_suggestion
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return suggestion.clone();
    }
    contact.id.to_string().chars().take(8).collect()
}

fn build_contacts_runtime_view(
    contacts: ContactsState,
    discovered_peers: DiscoveredPeersState,
) -> ContactsRuntimeView {
    let known_contact_ids: HashSet<_> = contacts.contact_ids().copied().collect();
    let mut rows: Vec<_> = contacts
        .all_contacts()
        .map(|contact| ContactsRuntimeContact {
            authority_id: contact.id,
            name: display_contact_name(contact),
            nickname_hint: contact
                .nickname_suggestion
                .clone()
                .filter(|value| !value.trim().is_empty()),
            is_guardian: contact.is_guardian,
            is_member: contact.is_member,
            is_online: contact.is_online,
            relationship_state: contact.relationship_state,
            confirmation: match contact.relationship_state {
                ContactRelationshipState::PendingOutbound => ConfirmationState::PendingLocal,
                _ => ConfirmationState::Confirmed,
            },
            invitation_code: contact.invitation_code.clone(),
        })
        .collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));

    let mut lan_peers: Vec<_> = discovered_peers
        .peers
        .into_iter()
        .filter(|peer| {
            peer.method == aura_app::ui::signals::DiscoveredPeerMethod::BootstrapCandidate
                && !known_contact_ids.contains(&peer.authority_id)
        })
        .map(|peer| ContactsRuntimePeer {
            authority_id: peer.authority_id,
            address: peer.address,
            invited: peer.invited,
        })
        .collect();
    lan_peers.sort_by(|left, right| {
        left.authority_id
            .to_string()
            .cmp(&right.authority_id.to_string())
    });

    ContactsRuntimeView {
        loaded: true,
        contacts: rows,
        lan_peers,
    }
}

pub(in crate::app) async fn load_contacts_runtime_view(
    controller: Arc<UiController>,
) -> ContactsRuntimeView {
    let contacts = {
        let core = controller.app_core().read().await;
        core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
    };
    let discovered_peers = {
        let core = controller.app_core().read().await;
        core.read(&*DISCOVERED_PEERS_SIGNAL)
            .await
            .unwrap_or_default()
    };
    let runtime = build_contacts_runtime_view(contacts, discovered_peers);
    let runtime_facts = vec![RuntimeFact::RemoteFactsPulled {
        contact_count: u32::try_from(runtime.contacts.len()).unwrap_or(u32::MAX),
        lan_peer_count: u32::try_from(runtime.lan_peers.len()).unwrap_or(u32::MAX),
    }];
    controller.publish_runtime_contacts_projection(
        runtime
            .contacts
            .iter()
            .map(|contact| {
                (
                    contact.authority_id,
                    contact.name.clone(),
                    contact.is_guardian,
                    contact.relationship_state,
                    contact.invitation_code.clone(),
                )
            })
            .collect(),
        runtime_facts,
    );
    runtime
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::signal_defs::{DiscoveredPeer, DiscoveredPeerMethod};
    use aura_app::ui::types::{Contact, ContactRelationshipState};
    use aura_app::views::ReadReceiptPolicy;

    #[test]
    fn build_contacts_runtime_view_preserves_relationship_projection() {
        let alice = AuthorityId::new_from_entropy([1u8; 32]);
        let bob = AuthorityId::new_from_entropy([2u8; 32]);
        let carol = AuthorityId::new_from_entropy([3u8; 32]);
        let dave = AuthorityId::new_from_entropy([4u8; 32]);
        let eve = AuthorityId::new_from_entropy([5u8; 32]);

        let contacts = ContactsState::from_contacts([
            Contact {
                id: alice,
                nickname: "Alice".to_string(),
                nickname_suggestion: None,
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: true,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::Contact,
                invitation_code: None,
            },
            Contact {
                id: bob,
                nickname: "Bob".to_string(),
                nickname_suggestion: None,
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: false,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::PendingOutbound,
                invitation_code: None,
            },
            Contact {
                id: carol,
                nickname: "Carol".to_string(),
                nickname_suggestion: None,
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: false,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::PendingInbound,
                invitation_code: None,
            },
            Contact {
                id: dave,
                nickname: "Dave".to_string(),
                nickname_suggestion: None,
                is_guardian: true,
                is_member: false,
                last_interaction: None,
                is_online: true,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::Friend,
                invitation_code: None,
            },
        ]);
        let discovered = DiscoveredPeersState {
            peers: vec![
                DiscoveredPeer {
                    authority_id: bob,
                    address: "192.0.2.2:9000".to_string(),
                    method: DiscoveredPeerMethod::BootstrapCandidate,
                    invited: false,
                },
                DiscoveredPeer {
                    authority_id: eve,
                    address: "192.0.2.5:9000".to_string(),
                    method: DiscoveredPeerMethod::BootstrapCandidate,
                    invited: true,
                },
                DiscoveredPeer {
                    authority_id: carol,
                    address: String::new(),
                    method: DiscoveredPeerMethod::Rendezvous,
                    invited: false,
                },
            ],
            last_updated_ms: 0,
        };

        let runtime = build_contacts_runtime_view(contacts, discovered);

        assert_eq!(runtime.contacts.len(), 4);
        assert_eq!(runtime.lan_peers.len(), 1);
        assert_eq!(
            runtime.lan_peers.first(),
            Some(&ContactsRuntimePeer {
                authority_id: eve,
                address: "192.0.2.5:9000".to_string(),
                invited: true,
            })
        );
        assert_eq!(
            runtime
                .contacts
                .iter()
                .find(|contact| contact.authority_id == alice)
                .map(|contact| (contact.relationship_state, contact.confirmation)),
            Some((
                ContactRelationshipState::Contact,
                ConfirmationState::Confirmed,
            ))
        );
        assert_eq!(
            runtime
                .contacts
                .iter()
                .find(|contact| contact.authority_id == bob)
                .map(|contact| (contact.relationship_state, contact.confirmation)),
            Some((
                ContactRelationshipState::PendingOutbound,
                ConfirmationState::PendingLocal,
            ))
        );
        assert_eq!(
            runtime
                .contacts
                .iter()
                .find(|contact| contact.authority_id == carol)
                .map(|contact| (contact.relationship_state, contact.confirmation)),
            Some((
                ContactRelationshipState::PendingInbound,
                ConfirmationState::Confirmed,
            ))
        );
        assert_eq!(
            runtime
                .contacts
                .iter()
                .find(|contact| contact.authority_id == dave)
                .map(|contact| (contact.relationship_state, contact.is_guardian)),
            Some((ContactRelationshipState::Friend, true))
        );
    }
}
