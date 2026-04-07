use std::sync::Arc;

use async_lock::RwLock;
use aura_core::{
    crypto::hash::hash,
    types::{AuthorityId, ChannelId, ContextId},
    AuraError, OperationContext, TraceContext,
};

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

/// Move position in neighborhood view.
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

        neighborhood.position = Some(TraversalPosition {
            current_home_id: target_home_id,
            current_home_name: home_name,
            depth: depth_value,
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

    let neighborhood_id = ChannelId::from_bytes(hash(
        format!("neighborhood:{authority}:{neighborhood_name}:{timestamp_ms}").as_bytes(),
    ))
    .to_string();

    let neighborhood_state = {
        let core = app_core.read().await;
        let mut neighborhood = core.views().get_neighborhood();
        neighborhood.neighborhood_id = Some(neighborhood_id.clone());
        neighborhood.neighborhood_name = Some(neighborhood_name);
        neighborhood
    };

    publish_neighborhood_projection(app_core, neighborhood_state).await?;
    Ok(neighborhood_id)
}

/// Add a home as a member of the active neighborhood and apply allocation budget.
pub async fn add_home_to_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
) -> Result<(), AuraError> {
    let (homes_state, neighborhood_state) = {
        let core = app_core.read().await;
        let mut homes = core.views().get_homes();
        let mut neighborhood = core.views().get_neighborhood();

        let target_home_id = resolve_target_home_id(&neighborhood, HomeSelector::parse(home_id)?)?;
        let target_home_name = resolve_home_name(&homes, &neighborhood, target_home_id);
        let target_member_count = homes
            .home_state(&target_home_id)
            .map(|home| home.member_count);

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

        let newly_joined = neighborhood.add_member_home(target_home_id);
        if newly_joined {
            if let Some(home) = homes.home_mut(&target_home_id) {
                home.storage
                    .join_neighborhood()
                    .map_err(|error| AuraError::budget_exceeded(error.to_string()))?;
            }
        }

        (homes, neighborhood)
    };

    publish_homes_and_neighborhood_projection(app_core, homes_state, neighborhood_state).await
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

    publish_homes_and_neighborhood_projection(app_core, homes, neighborhood).await?;

    let _ = description;
    Ok(home_id)
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
