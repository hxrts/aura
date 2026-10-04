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
    ui_contract::{
        OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
        SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
    },
    views::{
        home::{HomeState, HomesState},
        neighborhood::{NeighborHome, NeighborhoodState, OneHopLinkType, TraversalPosition},
    },
    workflows::channel_ref::HomeSelector,
    workflows::observed_projection::{
        replace_homes_projection_observed, update_homes_projection_observed,
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

async fn publish_homes_projection(
    app_core: &Arc<RwLock<AppCore>>,
    homes_state: HomesState,
) -> Result<(), AuraError> {
    update_homes_projection_observed(app_core, move |state| {
        *state = homes_state;
    })
    .await
}

async fn publish_neighborhood_projection(
    app_core: &Arc<RwLock<AppCore>>,
    neighborhood_state: NeighborhoodState,
) -> Result<(), AuraError> {
    update_neighborhood_projection_observed(app_core, move |state| {
        *state = neighborhood_state;
    })
    .await
}

async fn publish_homes_and_neighborhood_projection(
    app_core: &Arc<RwLock<AppCore>>,
    homes_state: HomesState,
    neighborhood_state: NeighborhoodState,
) -> Result<(), AuraError> {
    publish_homes_projection(app_core, homes_state).await?;
    publish_neighborhood_projection(app_core, neighborhood_state).await
}

/// Highest entry depth (0 limited, 1 partial, 2 full) the viewer may use for
/// a home: an explicit access override wins, otherwise the hop-based default
/// (own/member home full, 1-hop neighbor partial, anything else limited).
fn allowed_entry_depth(
    neighborhood: &crate::views::neighborhood::NeighborhoodState,
    homes: &crate::views::home::HomesState,
    target: &ChannelId,
    viewer: Option<&aura_core::types::identifiers::AuthorityId>,
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
/// viewer's allowed access level for the target home.
pub async fn move_position(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
    depth: &str,
) -> Result<(), AuraError> {
    let depth_value = match depth.to_lowercase().as_str() {
        "limited" => 0,
        "partial" => 1,
        "full" => 2,
        _ => 1,
    };

    let mut homes = crate::workflows::observed_projection::homes_signal_snapshot(app_core).await?;
    let mut publish_homes = false;
    let neighborhood = {
        let mut core = app_core.write().await;
        let mut neighborhood = core.views().get_neighborhood();
        let target_home_id = resolve_target_home_id(&neighborhood, HomeSelector::parse(home_id)?)?;

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

        let viewer = core
            .runtime()
            .map(|runtime| runtime.authority_id())
            .or_else(|| core.authority().copied());
        let allowed_depth =
            allowed_entry_depth(&neighborhood, &homes, &target_home_id, viewer.as_ref());
        neighborhood.position = Some(TraversalPosition {
            current_home_id: target_home_id,
            current_home_name: home_name,
            depth: depth_value.min(allowed_depth),
            path: vec![target_home_id],
        });

        if homes.has_home(&target_home_id) {
            homes.select_home(Some(target_home_id));
            core.set_active_home_selection(Some(target_home_id));
            publish_homes = true;
        }

        neighborhood
    };

    if publish_homes {
        publish_homes_projection(app_core, homes).await?;
    }

    publish_neighborhood_projection(app_core, neighborhood).await
}

/// Create a neighborhood joined by the active home and return its id.
///
/// The neighborhood is durable: `SocialFact::NeighborhoodCreated` and the
/// home's `HomeJoinedNeighborhood` are committed in the home's context, so it
/// survives restart and the home's members learn of it. Joining charges the
/// home's neighborhood budget, so a home past `MAX_NEIGHBORHOODS` gets the
/// typed budget error before anything is committed.
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

    let (home_id, context_id, homes, neighborhood_state) = {
        let core = app_core.read().await;
        let mut homes = core.views().get_homes();
        let home = homes
            .current_home()
            .ok_or_else(|| AuraError::invalid("Create a home before creating a neighborhood"))?;
        let home_id = home.id;
        let context_id = home
            .context_id
            .ok_or_else(|| AuraError::invalid("The active home has no context"))?;
        homes
            .home_mut(&home_id)
            .ok_or_else(|| AuraError::invalid("The active home is not materialized"))?
            .join_neighborhood(&neighborhood_id, &neighborhood_name)
            .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;

        let mut neighborhood = core.views().get_neighborhood();
        neighborhood.neighborhood_id = Some(neighborhood_id.clone());
        neighborhood.neighborhood_name = Some(neighborhood_name.clone());
        neighborhood.add_member_home(home_id);
        (home_id, context_id, homes, neighborhood)
    };

    persist_created_neighborhood(
        app_core,
        neighborhood_channel,
        home_id,
        context_id,
        &neighborhood_name,
        timestamp_ms,
    )
    .await?;
    publish_homes_and_neighborhood_projection(app_core, homes, neighborhood_state).await?;
    Ok(neighborhood_id)
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

/// Add a home as a member of the active neighborhood and apply allocation budget.
pub async fn add_home_to_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
) -> Result<(), AuraError> {
    let timestamp_ms =
        crate::workflows::time::local_first_timestamp_ms(app_core, "context-local-first", &[])
            .await?;
    let (homes_state, neighborhood_state, durable_join) = {
        let core = app_core.read().await;
        let mut homes = core.views().get_homes();
        let mut neighborhood = core.views().get_neighborhood();

        let target_home_id = resolve_target_home_id(&neighborhood, HomeSelector::parse(home_id)?)?;
        let target_home_name = resolve_home_name(&homes, &neighborhood, target_home_id);
        let target_member_count = homes
            .home_state(&target_home_id)
            .map(|home| home.member_count);
        let (neighborhood_id, neighborhood_name) = neighborhood
            .neighborhood_id
            .clone()
            .zip(neighborhood.neighborhood_name.clone())
            .ok_or_else(|| AuraError::invalid("Create a neighborhood before adding homes"))?;

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
        neighborhood.add_member_home(target_home_id);

        // A materialized home records the join (charging its neighborhood
        // budget once per neighborhood) and commits it in its own context.
        let durable_join = match homes.home_mut(&target_home_id) {
            Some(home) => {
                let joined = home
                    .join_neighborhood(&neighborhood_id, &neighborhood_name)
                    .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;
                home.context_id
                    .filter(|_| joined)
                    .map(|context_id| (context_id, neighborhood_id))
            }
            None => None,
        };

        (
            homes,
            neighborhood,
            durable_join.map(|join| (target_home_id, join)),
        )
    };

    if let Some((home_id, (context_id, neighborhood_id))) = durable_join {
        persist_home_joined_neighborhood(
            app_core,
            home_id,
            context_id,
            &neighborhood_id,
            timestamp_ms,
        )
        .await?;
    }
    publish_homes_and_neighborhood_projection(app_core, homes_state, neighborhood_state).await
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
    let neighborhood_state = {
        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let mut neighborhood = core.views().get_neighborhood();

        let target_home_id = resolve_target_home_id(&neighborhood, HomeSelector::parse(home_id)?)?;
        if target_home_id == neighborhood.home_home_id {
            return Err(AuraError::invalid(
                "Cannot create one_hop_link from home to itself",
            ));
        }

        let target_home_name = resolve_home_name(&homes, &neighborhood, target_home_id);
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
        neighborhood
    };

    publish_neighborhood_projection(app_core, neighborhood_state).await
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

    let mut home = HomeState::new(
        home_id,
        Some(home_name.clone()),
        creator,
        timestamp_ms,
        context_id,
    );

    let (homes, neighborhood) = {
        let mut core = app_core.write().await;
        let mut homes = core.views().get_homes();
        let should_promote_to_primary = homes.is_empty()
            || homes
                .current_home()
                .map(|current| current.id == ChannelId::default())
                .unwrap_or(true);
        if should_promote_to_primary {
            home.is_primary = true;
        }
        let result = homes.add_home(home);
        homes.select_home(Some(result.home_id));
        core.set_active_home_selection(Some(result.home_id));

        let mut neighborhood = core.views().get_neighborhood();
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
                name: home_name.clone(),
                one_hop_link: OneHopLinkType::Direct,
                shared_contacts: 0,
                member_count: Some(1),
                can_traverse: true,
            });
        }

        (homes, neighborhood)
    };

    persist_created_home(
        app_core,
        home_id,
        context_id,
        &home_name,
        creator,
        timestamp_ms,
    )
    .await?;
    publish_homes_and_neighborhood_projection(app_core, homes, neighborhood).await?;

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
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let Some(runtime) = runtime else {
        return Ok(());
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
    let social_home_id = aura_social::HomeId::from_bytes(*home_id.as_bytes());
    let facts = [
        aura_social::SocialFact::home_created_ms(
            social_home_id,
            context_id,
            timestamp_ms,
            creator,
            home_name.to_string(),
        )
        .to_generic(),
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
        })
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

/// Initialize HOMES_SIGNAL with a default test home.
pub async fn initialize_test_home(
    app_core: &Arc<RwLock<AppCore>>,
    name: &str,
    authority_id: AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    let home_id = ChannelId::from_bytes(hash(format!("test-home:{name}").as_bytes()));
    let context_id = ContextId::new_from_entropy(hash(format!("test-context:{name}").as_bytes()));

    let home_state = HomeState::new(
        home_id,
        Some(name.to_string()),
        authority_id,
        timestamp_ms,
        context_id,
    );

    let homes = {
        let core = app_core.read().await;
        let mut homes = core.views().get_homes();
        homes.add_home(home_state);
        homes
    };

    replace_homes_projection_observed(app_core, homes).await?;
    Ok(home_id)
}
