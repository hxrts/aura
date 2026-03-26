use crate::model::UiController;
use aura_app::signal_defs::DiscoveredPeersState;
use aura_app::ui::contract::ConfirmationState;
use aura_app::ui::signals::{CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL};
use aura_app::ui::types::ContactsState;
use aura_app::ui_contract::RuntimeFact;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::AuthorityId;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct ContactsRuntimeContact {
    pub(in crate::app) authority_id: AuthorityId,
    pub(in crate::app) name: String,
    pub(in crate::app) nickname_hint: Option<String>,
    pub(in crate::app) is_guardian: bool,
    pub(in crate::app) is_member: bool,
    pub(in crate::app) is_online: bool,
    pub(in crate::app) confirmation: ConfirmationState,
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
            confirmation: ConfirmationState::Confirmed,
        })
        .collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));

    let mut lan_peers: Vec<_> = discovered_peers
        .peers
        .into_iter()
        .filter(|peer| peer.method == aura_app::ui::signals::DiscoveredPeerMethod::Lan)
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
                )
            })
            .collect(),
        runtime_facts,
    );
    runtime
}
