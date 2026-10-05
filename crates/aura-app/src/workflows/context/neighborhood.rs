use std::sync::Arc;

use async_lock::RwLock;
use aura_core::{
    crypto::hash::hash,
    effects::{ChannelCreateParams, ChannelJoinParams},
    types::{AuthorityId, ChannelId, ContextId},
    AuraError, OperationContext, TraceContext,
};
use aura_journal::DomainFact;

use crate::{
    projection_owner::ProjectionSlot,
    ui_contract::{
        OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
        SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
    },
    views::{
        home::HomesState,
        neighborhood::{NeighborHome, NeighborhoodState, OneHopLinkType, TraversalPosition},
    },
    workflows::channel_ref::HomeSelector,
    workflows::observed_projection::{
        try_update_neighborhood_projection_observed, update_homes_projection_with_revision,
        update_neighborhood_projection_observed,
    },
    workflows::semantic_facts::{prove_home_created, SemanticWorkflowOwner},
    AppCore,
};

fn resolve_target_home_id(
    neighborhood: &NeighborhoodState,
    home_id: HomeSelector,
) -> Result<ChannelId, AuraError> {
    match home_id {
        HomeSelector::Home => Ok(neighborhood.home_home_id),
        HomeSelector::Current => Ok(neighborhood
            .position
            .as_ref()
            .map(|position| position.current_home_id)
            .unwrap_or(neighborhood.home_home_id)),
        HomeSelector::Id(home_id) => Ok(home_id),
    }
}

fn resolve_home_name(
    homes: &HomesState,
    neighborhood: &NeighborhoodState,
    home_id: ChannelId,
) -> String {
    if let Some(home) = homes.home_state(&home_id) {
        if !home.name.trim().is_empty() {
            return home.name.clone();
        }
    }

    if home_id == neighborhood.home_home_id {
        return neighborhood.home_name.clone();
    }

    neighborhood
        .neighbor(&home_id)
        .map(|neighbor| neighbor.name.clone())
        .unwrap_or_else(|| home_id.to_string())
}

/// Highest entry depth (0 limited, 1 partial, 2 full) the viewer may use for
/// a home: an explicit access override wins, otherwise the hop-based default
/// (own/member home full, 1-hop neighbor partial, anything else limited).
fn allowed_entry_depth(
    neighborhood: &NeighborhoodState,
    homes: &HomesState,
    target: &ChannelId,
    viewer: Option<&AuthorityId>,
) -> u32 {
    if let (Some(home), Some(viewer)) = (homes.home_state(target), viewer) {
        if let Some(level) = home.access_override(viewer) {
            return match level {
                aura_social::AccessLevel::Limited => 0,
                aura_social::AccessLevel::Partial => 1,
                aura_social::AccessLevel::Full => 2,
            };
        }
        if home.member(viewer).is_some() {
            return 2;
        }
    }
    if *target == neighborhood.home_home_id {
        2
    } else if neighborhood.neighbor(target).is_some() {
        1
    } else {
        0
    }
}

/// Move position in neighborhood view. The requested depth is clamped to the
/// viewer\x27s allowed access level for the target home; returns the granted depth
/// (0 limited, 1 partial, 2 full).
pub async fn move_position(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
    depth: &str,
) -> Result<u32, AuraError> {
    let depth_value = match depth.to_lowercase().as_str() {
        "limited" => 0,
        "partial" => 1,
        "full" => 2,
        _ => 1,
    };

    let selector = HomeSelector::parse(home_id)?;
    let homes = crate::workflows::observed_projection::homes_signal_snapshot(app_core).await?;
    let viewer = {
        let core = app_core.read().await;
        core.authority()
            .copied()
            .or_else(|| core.runtime().map(|runtime| runtime.authority_id()))
    };
    let gate = app_core.read().await.navigation_projection_gate();
    let _navigation = gate.lock().await;
    let (target_home_id, granted_depth) =
        try_update_neighborhood_projection_observed(app_core, move |neighborhood| {
            let target_home_id = resolve_target_home_id(neighborhood, selector)?;
            let granted_depth = depth_value.min(allowed_entry_depth(
                neighborhood,
                &homes,
                &target_home_id,
                viewer.as_ref(),
            ));
            let home_name = neighborhood
                .neighbor(&target_home_id)
                .map(|neighbor| neighbor.name.clone())
                .unwrap_or_else(|| {
                    if target_home_id == neighborhood.home_home_id {
                        neighborhood.home_name.clone()
                    } else {
                        target_home_id.to_string()
                    }
                });
            neighborhood.position = Some(TraversalPosition {
                current_home_id: target_home_id,
                current_home_name: home_name,
                depth: granted_depth,
                path: vec![target_home_id],
            });
            Ok((target_home_id, granted_depth))
        })
        .await?;
    let (selected, revision) = update_homes_projection_with_revision(app_core, move |homes| {
        if homes.has_home(&target_home_id) {
            homes.select_home(Some(target_home_id));
            true
        } else {
            false
        }
    })
    .await?;
    if selected {
        app_core
            .write()
            .await
            .set_active_home_selection_if_projection_current(revision, target_home_id);
    }
    crate::workflows::observed_projection::mirror_homes_signal_into_view_locked(app_core).await?;
    Ok(granted_depth)
}

/// Create or select the active neighborhood.
pub async fn create_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    name: String,
) -> Result<String, AuraError> {
    let timestamp_ms =
        crate::workflows::time::local_first_timestamp_ms(app_core, "context-local-first", &[])
            .await?;
    let neighborhood_name = if name.trim().is_empty() {
        "Neighborhood".to_string()
    } else {
        name.trim().to_string()
    };

    let authority = {
        let core = app_core.read().await;
        core.runtime()
            .map(|runtime| runtime.authority_id())
            .or_else(|| core.authority().copied())
    }
    .ok_or_else(|| AuraError::permission_denied("Authority not set"))?;

    let neighborhood_channel = ChannelId::from_bytes(hash(
        format!("neighborhood:{authority}:{neighborhood_name}:{timestamp_ms}").as_bytes(),
    ));
    let neighborhood_id = neighborhood_channel.to_string();

    // The active home joins the new neighborhood. Charge its neighborhood
    // budget on a copy first so an over-budget home fails before anything is
    // committed.
    let homes = crate::workflows::observed_projection::homes_signal_snapshot(app_core).await?;
    let home = homes
        .current_home()
        .ok_or_else(|| AuraError::invalid("Create a home before creating a neighborhood"))?;
    let home_id = home.id;
    let context_id = home
        .context_id
        .ok_or_else(|| AuraError::invalid("The active home has no context"))?;
    home.clone()
        .join_neighborhood(&neighborhood_id, &neighborhood_name)
        .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;

    persist_created_neighborhood(
        app_core,
        neighborhood_channel,
        home_id,
        context_id,
        &neighborhood_name,
        timestamp_ms,
    )
    .await?;
    let (joined_id, joined_name) = (neighborhood_id.clone(), neighborhood_name.clone());
    crate::workflows::observed_projection::try_update_homes_projection_observed(
        app_core,
        move |homes| {
            if let Some(home) = homes.home_mut(&home_id) {
                home.join_neighborhood(&joined_id, &joined_name)
                    .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;
            }
            Ok(())
        },
    )
    .await?;
    let publication_id = neighborhood_id.clone();
    update_neighborhood_projection_observed(app_core, move |neighborhood| {
        neighborhood.neighborhood_id = Some(publication_id);
        neighborhood.neighborhood_name = Some(neighborhood_name);
        neighborhood.add_member_home(home_id);
    })
    .await?;
    Ok(neighborhood_id)
}

/// Add a home as a member of the active neighborhood and apply allocation budget.
pub async fn add_home_to_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
) -> Result<(), AuraError> {
    let homes = crate::workflows::observed_projection::homes_signal_snapshot(app_core).await?;
    let owner = app_core.read().await.projection_owner();
    let neighborhood = owner
        .snapshot(ProjectionSlot::neighborhood())
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?
        .value;
    let target_home_id = resolve_target_home_id(&neighborhood, HomeSelector::parse(home_id)?)?;
    if neighborhood.is_member_home(&target_home_id) {
        return Ok(());
    }
    let target_home_name = resolve_home_name(&homes, &neighborhood, target_home_id);
    let target_member_count = homes
        .home_state(&target_home_id)
        .map(|home| home.member_count);

    // A materialized home records a join of the active neighborhood (charging
    // its neighborhood budget once per neighborhood) and commits it in its own
    // context; without an active neighborhood only the storage is reserved.
    let active = neighborhood
        .neighborhood_id
        .clone()
        .zip(neighborhood.neighborhood_name.clone());
    let reservation = active.clone();
    // Reserve the fallible storage allocation before publishing membership.
    let reserved = crate::workflows::observed_projection::try_update_homes_projection_observed(
        app_core,
        move |homes| {
            let Some(home) = homes.home_mut(&target_home_id) else {
                return Ok(None);
            };
            match reservation {
                Some((neighborhood_id, name)) => {
                    let joined = home
                        .join_neighborhood(&neighborhood_id, &name)
                        .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;
                    Ok(joined.then_some(home.context_id))
                }
                None => {
                    home.storage
                        .join_neighborhood()
                        .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;
                    Ok(Some(None))
                }
            }
        },
    )
    .await?;
    let durable_context = reserved.flatten();
    let reserved = reserved.is_some();
    let inserted = update_neighborhood_projection_observed(app_core, move |neighborhood| {
        if target_home_id != neighborhood.home_home_id
            && neighborhood.neighbor(&target_home_id).is_none()
        {
            neighborhood.add_neighbor(NeighborHome {
                id: target_home_id,
                name: target_home_name,
                one_hop_link: OneHopLinkType::Direct,
                shared_contacts: 0,
                member_count: target_member_count,
                can_traverse: true,
            });
        }
        neighborhood.add_member_home(target_home_id)
    })
    .await;
    let rollback = active.clone();
    if reserved && (inserted.is_err() || inserted.as_ref().is_ok_and(|inserted| !inserted)) {
        // Publication failed or another writer joined while storage was being
        // reserved. Remove only this attempt's allocation.
        crate::workflows::observed_projection::try_update_homes_projection_observed(
            app_core,
            move |homes| {
                if let Some(home) = homes.home_mut(&target_home_id) {
                    home.storage
                        .leave_neighborhood()
                        .map_err(|error| AuraError::internal(error.to_string()))?;
                    if let Some((neighborhood_id, _)) = &rollback {
                        home.neighborhoods.remove(neighborhood_id);
                    }
                }
                Ok(())
            },
        )
        .await?;
    }
    if inserted? {
        if let (Some(context_id), Some((neighborhood_id, _))) = (durable_context, active) {
            let timestamp_ms = crate::workflows::time::local_first_timestamp_ms(
                app_core,
                "context-local-first",
                &[],
            )
            .await?;
            persist_home_joined_neighborhood(
                app_core,
                target_home_id,
                context_id,
                &neighborhood_id,
                timestamp_ms,
            )
            .await?;
        }
    }
    Ok(())
}

/// Commits a created neighborhood and the home's membership in it. Without a
/// runtime the neighborhood stays local.
async fn persist_created_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    neighborhood: ChannelId,
    home_id: ChannelId,
    context_id: ContextId,
    name: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let Some(runtime) = runtime else {
        return Ok(());
    };
    let neighborhood_id = aura_social::NeighborhoodId::from_bytes(*neighborhood.as_bytes());
    let facts = [
        aura_social::SocialFact::neighborhood_created_ms(
            neighborhood_id,
            context_id,
            timestamp_ms,
            name.to_string(),
        )
        .to_generic(),
        aura_social::SocialFact::home_joined_neighborhood_ms(
            aura_social::HomeId::from_bytes(*home_id.as_bytes()),
            neighborhood_id,
            context_id,
            timestamp_ms,
        )
        .to_generic(),
    ];
    runtime
        .commit_relational_facts(&facts)
        .await
        .map_err(|error| AuraError::Storage {
            message: "persist neighborhood".to_owned(),
            source: Some(Arc::new(error)),
        })
}

/// Commits a home's join of an existing neighborhood. Without a runtime the
/// join stays local.
async fn persist_home_joined_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: ChannelId,
    context_id: ContextId,
    neighborhood_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let Some(runtime) = runtime else {
        return Ok(());
    };
    let neighborhood: ChannelId = neighborhood_id
        .parse()
        .map_err(|_| AuraError::invalid("The active neighborhood id is malformed"))?;
    let fact = aura_social::SocialFact::home_joined_neighborhood_ms(
        aura_social::HomeId::from_bytes(*home_id.as_bytes()),
        aura_social::NeighborhoodId::from_bytes(*neighborhood.as_bytes()),
        context_id,
        timestamp_ms,
    )
    .to_generic();
    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|error| AuraError::Storage {
            message: "persist neighborhood membership".to_owned(),
            source: Some(Arc::new(error)),
        })
}

/// Force direct one_hop_link between local home and the target home in the active neighborhood.
pub async fn link_home_one_hop_link(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
) -> Result<(), AuraError> {
    let homes = crate::workflows::observed_projection::homes_signal_snapshot(app_core).await?;
    let selector = HomeSelector::parse(home_id)?;
    try_update_neighborhood_projection_observed(app_core, move |neighborhood| {
        let target_home_id = resolve_target_home_id(neighborhood, selector)?;
        if target_home_id == neighborhood.home_home_id {
            return Err(AuraError::invalid(
                "Cannot create one_hop_link from home to itself",
            ));
        }
        let target_home_name = resolve_home_name(&homes, neighborhood, target_home_id);
        let target_member_count = homes
            .home_state(&target_home_id)
            .map(|home| home.member_count);
        neighborhood.add_neighbor(NeighborHome {
            id: target_home_id,
            name: target_home_name,
            one_hop_link: OneHopLinkType::Direct,
            shared_contacts: 0,
            member_count: target_member_count,
            can_traverse: true,
        });
        Ok(())
    })
    .await
}

async fn create_home_with_creator(
    app_core: &Arc<RwLock<AppCore>>,
    creator: AuthorityId,
    name: Option<String>,
    description: Option<String>,
) -> Result<ChannelId, AuraError> {
    let timestamp_ms =
        crate::workflows::time::local_first_timestamp_ms(app_core, "context-local-first", &[])
            .await?;
    let home_name = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Home")
        .to_string();

    let home_id = ChannelId::from_bytes(hash(
        format!("home:{creator}:{home_name}:{timestamp_ms}").as_bytes(),
    ));
    let context_id =
        ContextId::new_from_entropy(hash(format!("home-context:{creator}:{home_id}").as_bytes()));

    let created = persist_created_home(
        app_core,
        home_id,
        context_id,
        &home_name,
        creator,
        timestamp_ms,
    )
    .await?;
    let witness = app_core
        .read()
        .await
        .projection_owner()
        .home_created_witness(&created)
        .ok_or_else(|| AuraError::internal("created home fact has invalid shape"))?;

    let gate = app_core.read().await.navigation_projection_gate();
    let _navigation = gate.lock().await;
    let (should_promote_to_primary, revision) =
        update_homes_projection_with_revision(app_core, move |homes| {
            let should_promote_to_primary = homes.is_empty()
                || homes
                    .current_home()
                    .map(|current| current.id == ChannelId::default())
                    .unwrap_or(true);
            let result = homes.materialize_created_home(witness, creator);
            if should_promote_to_primary {
                if let Some(home) = homes.home_mut(&result.home_id) {
                    home.is_primary = true;
                }
            }
            homes.select_home(Some(result.home_id));
            should_promote_to_primary
        })
        .await?;
    app_core
        .write()
        .await
        .set_active_home_selection_if_projection_current(revision, home_id);

    update_neighborhood_projection_observed(app_core, move |neighborhood| {
        if neighborhood.home_name.is_empty() || neighborhood.home_home_id == ChannelId::default() {
            neighborhood.home_home_id = home_id;
            neighborhood.home_name = home_name.clone();
            neighborhood.position = Some(TraversalPosition {
                current_home_id: home_id,
                current_home_name: home_name.clone(),
                depth: 2,
                path: vec![home_id],
            });
        } else if should_promote_to_primary {
            neighborhood.position = Some(TraversalPosition {
                current_home_id: home_id,
                current_home_name: home_name.clone(),
                depth: 2,
                path: vec![home_id],
            });
        } else if neighborhood.home_home_id != home_id && neighborhood.neighbor(&home_id).is_none()
        {
            neighborhood.add_neighbor(NeighborHome {
                id: home_id,
                name: home_name,
                one_hop_link: OneHopLinkType::Direct,
                shared_contacts: 0,
                member_count: Some(1),
                can_traverse: true,
            });
        }
    })
    .await?;
    crate::workflows::observed_projection::mirror_homes_signal_into_view_locked(app_core).await?;

    let _ = description;
    Ok(home_id)
}

/// Makes a created home durable and invitable: its AMP channel exists in the
/// home's context with the creator joined, and `SocialFact::HomeCreated` plus
/// the creator's `MemberJoined` are committed, so the home projection is
/// rebuilt from facts after a restart. Without a runtime the home stays local.
async fn persist_created_home(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: ChannelId,
    context_id: ContextId,
    home_name: &str,
    creator: AuthorityId,
    timestamp_ms: u64,
) -> Result<aura_social::SocialFact, AuraError> {
    let social_home_id = aura_social::HomeId::from_bytes(*home_id.as_bytes());
    let created = aura_social::SocialFact::home_created_ms(
        social_home_id,
        context_id,
        timestamp_ms,
        creator,
        home_name.to_string(),
    );
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let Some(runtime) = runtime else {
        return Ok(created);
    };
    runtime
        .amp_create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(home_id),
            skip_window: None,
            topic: Some(home_name.to_string()),
        })
        .await
        .map_err(|error| AuraError::Internal {
            message: "create home channel".to_owned(),
            source: Some(Arc::new(error)),
        })?;
    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: home_id,
            participant: creator,
        })
        .await
        .map_err(|error| AuraError::Internal {
            message: "join home channel".to_owned(),
            source: Some(Arc::new(error)),
        })?;
    let facts = [
        created.to_generic(),
        aura_social::SocialFact::member_joined_ms(
            creator,
            social_home_id,
            context_id,
            timestamp_ms,
            creator.to_string(),
        )
        .to_generic(),
    ];
    runtime
        .commit_relational_facts(&facts)
        .await
        .map_err(|error| AuraError::Storage {
            message: "persist home".to_owned(),
            source: Some(Arc::new(error)),
        })?;
    Ok(created)
}

async fn fail_create_home<T>(
    owner: &SemanticWorkflowOwner,
    detail: impl Into<String>,
) -> Result<T, AuraError> {
    let error = SemanticOperationError::new(
        SemanticFailureDomain::Internal,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    owner.publish_failure(error.clone()).await?;
    Err(AuraError::agent(
        error
            .detail
            .unwrap_or_else(|| "create home failed".to_string()),
    ))
}

#[aura_macros::semantic_owner(
    owner = "create_home_owned",
    wrapper = "create_home",
    terminal = "publish_success_with",
    postcondition = "home_created",
    proof = crate::workflows::semantic_facts::HomeCreatedProof,
    authoritative_inputs = "homes,authoritative_source",
    depends_on = "home_projection_published",
    child_ops = "",
    category = "move_owned"
)]
async fn create_home_owned(
    app_core: &Arc<RwLock<AppCore>>,
    creator: AuthorityId,
    name: Option<String>,
    description: Option<String>,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<ChannelId, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;

    let home_id = match create_home_with_creator(app_core, creator, name, description).await {
        Ok(home_id) => home_id,
        Err(error) => return fail_create_home(owner, error.to_string()).await,
    };

    owner
        .publish_success_with(prove_home_created(app_core, home_id).await?)
        .await?;
    Ok(home_id)
}

/// Create a home for the active authority and return its channel id.
pub async fn create_home(
    app_core: &Arc<RwLock<AppCore>>,
    name: Option<String>,
    description: Option<String>,
) -> Result<ChannelId, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::create_home(),
        None,
        SemanticOperationKind::CreateHome,
    );
    let creator = {
        let core = app_core.read().await;
        core.runtime()
            .map(|runtime| runtime.authority_id())
            .or_else(|| core.authority().copied())
    }
    .ok_or_else(|| AuraError::permission_denied("Authority not set"));

    let creator = match creator {
        Ok(creator) => creator,
        Err(error) => return fail_create_home(&owner, error.to_string()).await,
    };
    create_home_owned(app_core, creator, name, description, &owner, None).await
}

/// Create a home for a specific authority and return its channel id.
pub async fn create_home_for_authority(
    app_core: &Arc<RwLock<AppCore>>,
    creator: AuthorityId,
    name: Option<String>,
    description: Option<String>,
) -> Result<ChannelId, AuraError> {
    create_home_with_creator(app_core, creator, name, description).await
}

/// Get current neighborhood state.
pub async fn get_neighborhood_state(app_core: &Arc<RwLock<AppCore>>) -> NeighborhoodState {
    let core = app_core.read().await;
    core.views().get_neighborhood()
}

/// Get current traversal position.
pub async fn get_current_position(app_core: &Arc<RwLock<AppCore>>) -> Option<TraversalPosition> {
    let core = app_core.read().await;
    let neighborhood = core.views().get_neighborhood();
    neighborhood.position
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    use crate::views::home::HomeCreationWitness;
    use crate::workflows::observed_projection::{
        mirror_homes_signal_into_view, update_homes_projection_observed,
    };
    use futures::FutureExt;

    #[tokio::test]
    async fn queued_moves_keep_homes_selection_and_traversal_position_paired() {
        let app_core = crate::testing::default_test_app_core();
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();
        let first_id = ChannelId::from_bytes(hash(b"navigation-two-moves-first"));
        let second_id = ChannelId::from_bytes(hash(b"navigation-two-moves-second"));
        update_homes_projection_observed(&app_core, |homes| {
            let creator = AuthorityId::new_from_entropy([51u8; 32]);
            let context = ContextId::new_from_entropy([52u8; 32]);
            for (home_id, name) in [(first_id, "First"), (second_id, "Second")] {
                let created = aura_social::SocialFact::home_created_ms(
                    aura_social::HomeId::from_bytes(*home_id.as_bytes()),
                    context,
                    1,
                    creator,
                    name.to_string(),
                );
                let witness = HomeCreationWitness::from_created_fact(&created).unwrap();
                homes.materialize_created_home(witness, creator);
            }
            homes.select_home(Some(first_id));
        })
        .await
        .unwrap();
        mirror_homes_signal_into_view(&app_core).await.unwrap();

        let gate = app_core.read().await.navigation_projection_gate();
        let held = gate.lock().await;
        let first_target = second_id.to_string();
        let second_target = first_id.to_string();
        let first = move_position(&app_core, &first_target, "full");
        let second = move_position(&app_core, &second_target, "full");
        futures::pin_mut!(first, second);
        assert!((&mut first).now_or_never().is_none());
        assert!((&mut second).now_or_never().is_none());
        drop(held);
        let (first_result, second_result) = futures::join!(first, second);
        first_result.unwrap();
        second_result.unwrap();

        let core = app_core.read().await;
        let selected = core.views().get_homes().current_home_id().copied();
        let position = core
            .views()
            .get_neighborhood()
            .position
            .as_ref()
            .map(|position| position.current_home_id);
        assert!(matches!(selected, Some(id) if id == first_id || id == second_id));
        assert_eq!(position, selected);
        assert_eq!(core.active_home_selection(), selected);
    }
}
