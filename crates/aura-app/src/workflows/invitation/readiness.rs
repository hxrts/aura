#![allow(missing_docs)]

use super::*;
#[cfg(feature = "signals")]
use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;

pub(in crate::workflows) async fn refresh_authoritative_invitation_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    #[cfg(feature = "signals")]
    let signal_has_pending =
        super::accept::invitations_signal_has_pending_home_or_channel_invitation(
            &read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await,
        );
    #[cfg(not(feature = "signals"))]
    let signal_has_pending = false;

    let runtime_has_pending = if signal_has_pending {
        true
    } else if crate::harness_mode_enabled()
        && harness_invitation_accept_operation_in_flight(app_core).await
    {
        false
    } else {
        authoritative_pending_home_or_channel_invitation(&runtime)
            .await?
            .is_some()
    };
    let replacements = if signal_has_pending || runtime_has_pending {
        vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
    } else {
        Vec::new()
    };
    replace_authoritative_semantic_facts_of_kind(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            (
                crate::ui_contract::AuthoritativeSemanticFactKind::PendingHomeInvitationReady,
                replacements,
            ),
        ),
    )
    .await
}

#[cfg(feature = "signals")]
async fn harness_invitation_accept_operation_in_flight(app_core: &Arc<RwLock<AppCore>>) -> bool {
    read_signal_or_default(app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
        .await
        .iter()
        .any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::OperationStatus { status, .. }
                    if matches!(
                        status.kind,
                        SemanticOperationKind::AcceptContactInvitation
                            | SemanticOperationKind::AcceptPendingChannelInvitation
                    ) && !status.phase.is_terminal()
            )
        })
}

#[cfg(not(feature = "signals"))]
async fn harness_invitation_accept_operation_in_flight(_app_core: &Arc<RwLock<AppCore>>) -> bool {
    false
}

pub(in crate::workflows) async fn refresh_authoritative_contact_link_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let contacts = contacts_signal_snapshot(app_core).await?;
    let contact_count = contacts.contact_count() as u32;
    let contact_link_facts = contacts
        .all_contacts()
        .map(|contact| AuthoritativeSemanticFact::ContactLinkReady {
            authority_id: contact.id.to_string(),
            contact_count,
        })
        .collect::<Vec<_>>();
    let invitation_accepted_facts = contacts
        .all_contacts()
        .map(|contact| AuthoritativeSemanticFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: Some(contact.id.to_string()),
            operation_state: Some(OperationState::Succeeded),
        })
        .collect::<Vec<_>>();
    update_authoritative_semantic_facts(app_core, move |facts| {
        facts.retain(|existing| {
            !matches!(
                existing,
                AuthoritativeSemanticFact::ContactLinkReady { .. }
                    | AuthoritativeSemanticFact::InvitationAccepted {
                        invitation_kind: InvitationFactKind::Contact,
                        ..
                    }
            )
        });
        facts.extend(contact_link_facts.clone());
        facts.extend(invitation_accepted_facts);
    })
    .await
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness",
    family = "authorizer"
)]
pub(in crate::workflows) async fn publish_authoritative_contact_invitation_accepted(
    app_core: &Arc<RwLock<AppCore>>,
    authority_id: AuthorityId,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(authority_id.to_string()),
                operation_state: Some(OperationState::Succeeded),
            },
        ),
    )
    .await
}

#[aura_macros::authoritative_source(kind = "signal")]
pub(in crate::workflows) async fn contacts_signal_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<crate::views::contacts::ContactsState, AuraError> {
    read_signal(app_core, &*CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME).await
}
