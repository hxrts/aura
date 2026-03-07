//! Context command handlers
//!
//! Handlers for SetContext, MovePosition, AcceptPendingHomeInvitation.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::context
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::{HOMES_SIGNAL, NEIGHBORHOOD_SIGNAL, NEIGHBORHOOD_SIGNAL_NAME};
use aura_app::ui::types::{NeighborHome, OneHopLinkType};
use aura_app::ui::workflows::demo_config::DEMO_SEED_2024;
use aura_app::ui::workflows::signals::{emit_signal, read_signal_or_default};
use aura_core::identifiers::ChannelId;
use aura_core::AuraError;
use std::str::FromStr;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::ui::workflows::context::{
    add_home_to_neighborhood, create_home, create_neighborhood, link_home_one_hop_link,
    move_position, set_context,
};
pub use aura_app::ui::workflows::invitation::accept_pending_home_invitation;

fn is_demo_mode() -> bool {
    std::env::var("AURA_DEMO_DEVICE_ID")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn formatted_home_name(home: &aura_app::ui::types::HomeState, fallback_id: ChannelId) -> String {
    let trimmed = home.name.trim();
    if trimmed.is_empty() {
        fallback_id.to_string()
    } else {
        trimmed.to_string()
    }
}

async fn maybe_auto_accept_demo_neighborhood_join(
    app_core: &Arc<RwLock<AppCore>>,
    requested_home_id: &str,
) -> Result<Option<String>, AuraError> {
    if !is_demo_mode() {
        return Ok(None);
    }

    let requested_home_id = match ChannelId::from_str(requested_home_id) {
        Ok(id) => id,
        Err(_) => return Ok(None),
    };

    let alice_id =
        crate::ids::authority_id(&format!("demo:{}:{}:authority", DEMO_SEED_2024, "Alice"));
    let carol_id = crate::ids::authority_id(&format!(
        "demo:{}:{}:authority",
        DEMO_SEED_2024 + 1,
        "Carol"
    ));

    let homes = read_signal_or_default(app_core, &*HOMES_SIGNAL).await;
    let neighborhood = read_signal_or_default(app_core, &*NEIGHBORHOOD_SIGNAL).await;

    let Some((alice_bob_carol_home_id, _)) = homes.iter().find(|(_, home)| {
        let has_alice = home.members.iter().any(|member| member.id == alice_id);
        let has_carol = home.members.iter().any(|member| member.id == carol_id);
        has_alice && has_carol && home.member_count >= 3
    }) else {
        return Ok(None);
    };

    if *alice_bob_carol_home_id == requested_home_id {
        return Ok(None);
    }

    if !neighborhood.is_member_home(&requested_home_id) {
        return Ok(None);
    }

    if neighborhood.is_member_home(alice_bob_carol_home_id) {
        return Ok(None);
    }

    add_home_to_neighborhood(app_core, &alice_bob_carol_home_id.to_string()).await?;

    // Update neighbor metadata to reflect that existing homes accepted the join.
    let homes = read_signal_or_default(app_core, &*HOMES_SIGNAL).await;
    let mut neighborhood = read_signal_or_default(app_core, &*NEIGHBORHOOD_SIGNAL).await;
    if let Some(home) = homes.home_state(alice_bob_carol_home_id) {
        let acceptance_count = neighborhood
            .member_home_ids()
            .iter()
            .filter(|id| **id != *alice_bob_carol_home_id)
            .count() as u32;

        if *alice_bob_carol_home_id != neighborhood.home_home_id {
            neighborhood.add_neighbor(NeighborHome {
                id: *alice_bob_carol_home_id,
                name: formatted_home_name(home, *alice_bob_carol_home_id),
                one_hop_link: OneHopLinkType::Direct,
                shared_contacts: acceptance_count,
                member_count: Some(home.member_count),
                can_traverse: true,
            });
        }

        emit_signal(
            app_core,
            &*NEIGHBORHOOD_SIGNAL,
            neighborhood,
            NEIGHBORHOOD_SIGNAL_NAME,
        )
        .await?;

        return Ok(Some(format!(
            "Join request accepted by {acceptance_count} neighborhood homes"
        )));
    }

    Ok(None)
}

/// Handle context commands
pub async fn handle_context(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::SetContext { context_id } => {
            // Delegate to workflow
            let new_context = if context_id.is_empty() {
                None
            } else {
                Some(context_id.clone())
            };

            match set_context(app_core, new_context.clone()).await {
                Ok(context) => Some(Ok(OpResponse::ContextChanged {
                    context_id: context,
                })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::MovePosition {
            neighborhood_id: _,
            home_id,
            depth,
        } => {
            // Delegate to workflow
            match move_position(app_core, home_id, depth).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::AcceptPendingHomeInvitation => {
            // Accept a pending home invitation via workflow
            match accept_pending_home_invitation(app_core).await {
                Ok(invitation_id) => Some(Ok(OpResponse::HomeInvitationAccepted {
                    invitation_id: invitation_id.to_string(),
                })),
                Err(e) => {
                    let message = format!("Failed to accept home invitation: {e}");
                    let lowered = message.to_lowercase();
                    if lowered.contains("no pending home invitation")
                        || lowered.contains("invalid invitation")
                        || lowered.contains("already accepted")
                    {
                        Some(Err(super::types::OpError::InvalidArgument(message)))
                    } else {
                        Some(Err(super::types::OpError::Failed(message)))
                    }
                }
            }
        }

        EffectCommand::CreateHome { name } => match create_home(app_core, name.clone(), None).await
        {
            Ok(home_id) => Some(Ok(OpResponse::HomeCreated {
                home_id: home_id.to_string(),
            })),
            Err(e) => Some(Err(super::types::OpError::Failed(format!(
                "Failed to create home: {e}"
            )))),
        },

        EffectCommand::CreateNeighborhood { name } => {
            match create_neighborhood(app_core, name.clone()).await {
                Ok(neighborhood_id) => {
                    Some(Ok(OpResponse::NeighborhoodCreated { neighborhood_id }))
                }
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to create neighborhood: {e}"
                )))),
            }
        }

        EffectCommand::AddHomeToNeighborhood { home_id } => {
            match add_home_to_neighborhood(app_core, home_id).await {
                Ok(()) => match maybe_auto_accept_demo_neighborhood_join(app_core, home_id).await {
                    Ok(Some(message)) => Some(Ok(OpResponse::HomeAddedToNeighborhood {
                        target_home_id: home_id.clone(),
                        message: Some(message),
                    })),
                    Ok(None) => Some(Ok(OpResponse::HomeAddedToNeighborhood {
                        target_home_id: home_id.clone(),
                        message: None,
                    })),
                    Err(e) => Some(Err(super::types::OpError::Failed(format!(
                        "Failed to auto-accept demo neighborhood join: {e}"
                    )))),
                },
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to add home to neighborhood: {e}"
                )))),
            }
        }

        EffectCommand::LinkHomeOneHopLink { home_id } => {
            match link_home_one_hop_link(app_core, home_id).await {
                Ok(()) => Some(Ok(OpResponse::HomeOneHopLinkSet {
                    target_home_id: home_id.clone(),
                })),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to link home one_hop_link: {e}"
                )))),
            }
        }

        _ => None,
    }
}
