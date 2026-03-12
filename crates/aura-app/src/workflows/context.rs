//! Context Workflow - Portable Business Logic
//!
//! This module contains context/navigation operations that are portable across all frontends.
//! It follows the reactive signal pattern and manages neighborhood navigation state.

use crate::{
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL, NEIGHBORHOOD_SIGNAL_NAME},
    views::{
        home::{HomeState, HomesState},
        neighborhood::{NeighborHome, NeighborhoodState, OneHopLinkType, TraversalPosition},
    },
    workflows::channel_ref::HomeSelector,
    workflows::harness_determinism,
    workflows::signals::read_signal,
    AppCore,
};
use async_lock::RwLock;
use aura_core::{
    crypto::hash::hash,
    identifiers::{AuthorityId, ChannelId, ContextId},
    AuraError,
};
use std::sync::Arc;

use crate::workflows::signals::emit_signal;
pub use crate::workflows::time::current_time_ms;

const MISSING_ACTIVE_HOME_MESSAGE: &str =
    "No active home selected. Open Neighborhood and create or select a home.";

/// Source of active-home resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveHomeSource {
    /// Home resolved from an explicit caller hint.
    ExplicitHint,
    /// Home resolved from the currently selected home.
    Selected,
    /// Home resolved from deterministic fallback ordering.
    Fallback,
}

/// Active-home resolution result shared by context-dependent workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveHomeResolution {
    /// Resolved home identifier.
    pub home_id: ChannelId,
    /// Resolved relational context identifier.
    pub context_id: ContextId,
    /// Resolution source.
    pub source: ActiveHomeSource,
}

fn resolution_from_home(
    home_id: ChannelId,
    home_state: &HomeState,
    source: ActiveHomeSource,
) -> Result<ActiveHomeResolution, AuraError> {
    let context_id = home_state
        .context_id
        .ok_or_else(|| AuraError::not_found(format!("Home {home_id} has no context ID")))?;
    Ok(ActiveHomeResolution {
        home_id,
        context_id,
        source,
    })
}

fn fallback_home(homes: &HomesState) -> Option<(ChannelId, HomeState)> {
    homes
        .iter()
        .min_by(|(lhs, _), (rhs, _)| lhs.as_bytes().cmp(rhs.as_bytes()))
        .map(|(home_id, home)| (*home_id, home.clone()))
}

async fn homes_state_signal_fallback(app_core: &Arc<RwLock<AppCore>>) -> HomesState {
    let view_homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    if view_homes.iter().next().is_some() {
        return view_homes;
    }

    let signal_homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
        .await
        .unwrap_or_default();
    if signal_homes.iter().next().is_some() {
        let mut core = app_core.write().await;
        core.views_mut().set_homes(signal_homes.clone());
    }
    signal_homes
}

async fn local_first_timestamp_ms(app_core: &Arc<RwLock<AppCore>>) -> u64 {
    if harness_determinism::harness_mode_enabled() {
        return harness_determinism::parity_timestamp_ms(app_core, "context-local-first", &[])
            .await
            .unwrap_or(0);
    }

    current_time_ms(app_core).await.unwrap_or(0)
}

/// Set active context for navigation and command targeting
///
/// **What it does**: Sets the active context ID
/// **Returns**: Optional context ID
/// **Signal pattern**: Read-only operation (no signal emission)
///
/// The actual state update is handled by the UI layer when it receives
/// the context change notification.
pub async fn set_context(
    _app_core: &Arc<RwLock<AppCore>>,
    context_id: Option<String>,
) -> Result<Option<String>, AuraError> {
    // Context switching is handled by UI layer
    // This workflow just validates and returns the new context
    Ok(context_id)
}

/// Move position in neighborhood view
///
/// **What it does**: Updates neighborhood traversal position
/// **Returns**: Unit result
/// **Signal pattern**: Updates neighborhood view state directly
///
/// This operation:
/// 1. Determines target home (home, current, or specific ID)
/// 2. Resolves home name from neighbor list
/// 3. Creates/updates TraversalPosition
/// 4. Updates neighborhood view state
///
/// Depth values:
/// - 0: Limited access
/// - 1: Partial access (default)
/// - 2: Full access
pub async fn move_position(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
    depth: &str,
) -> Result<(), AuraError> {
    // Parse the access-level string to determine traversal depth
    let depth_value = match depth.to_lowercase().as_str() {
        "limited" => 0,
        "partial" => 1,
        "full" => 2,
        _ => 1, // Default to partial
    };

    let signal_homes = homes_state_signal_fallback(app_core).await;

    let mut core = app_core.write().await;

    // Get current neighborhood state
    let mut neighborhood = core.views().get_neighborhood();
    let mut homes = core.views().get_homes();
    if homes.iter().next().is_none() && signal_homes.iter().next().is_some() {
        homes = signal_homes;
    }

    // Determine target home ID
    let target_selector = HomeSelector::parse(home_id)?;
    let target_home_id = resolve_target_home_id(&neighborhood, target_selector)?;

    // Get home name from neighbors or use the ID
    let home_name = neighborhood
        .neighbor(&target_home_id)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| {
            // Check if it's home
            if target_home_id == neighborhood.home_home_id {
                neighborhood.home_name.clone()
            } else {
                target_home_id.to_string()
            }
        });

    // Create or update position
    let position = TraversalPosition {
        current_home_id: target_home_id,
        current_home_name: home_name,
        depth: depth_value,
        path: vec![target_home_id],
    };
    neighborhood.position = Some(position);

    // Keep homes selection aligned with neighborhood traversal when the target
    // home is known locally (for member/channel metadata lookups).
    if homes.has_home(&target_home_id) {
        homes.select_home(Some(target_home_id));
        core.views_mut().set_homes(homes);
    }

    // Set the updated state
    core.views_mut().set_neighborhood(neighborhood);

    Ok(())
}

fn resolve_target_home_id(
    neighborhood: &NeighborhoodState,
    home_id: HomeSelector,
) -> Result<ChannelId, AuraError> {
    match home_id {
        HomeSelector::Home => Ok(neighborhood.home_home_id),
        HomeSelector::Current => Ok(neighborhood
            .position
            .as_ref()
            .map(|p| p.current_home_id)
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
        .map(|n| n.name.clone())
        .unwrap_or_else(|| home_id.to_string())
}

/// Create or select the active neighborhood.
///
/// This is a local-first workflow that stamps a deterministic neighborhood ID
/// and updates `NEIGHBORHOOD_SIGNAL`.
pub async fn create_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    name: String,
) -> Result<String, AuraError> {
    let timestamp_ms = local_first_timestamp_ms(app_core).await;
    let neighborhood_name = if name.trim().is_empty() {
        "Neighborhood".to_string()
    } else {
        name.trim().to_string()
    };

    let authority = {
        let core = app_core.read().await;
        core.runtime()
            .map(|r| r.authority_id())
            .or_else(|| core.authority().copied())
    }
    .ok_or_else(|| AuraError::permission_denied("Authority not set"))?;

    let neighborhood_id = ChannelId::from_bytes(hash(
        format!("neighborhood:{authority}:{neighborhood_name}:{timestamp_ms}").as_bytes(),
    ))
    .to_string();

    let neighborhood_state = {
        let mut core = app_core.write().await;
        let mut neighborhood = core.views().get_neighborhood();
        neighborhood.neighborhood_id = Some(neighborhood_id.clone());
        neighborhood.neighborhood_name = Some(neighborhood_name);
        core.views_mut().set_neighborhood(neighborhood.clone());
        neighborhood
    };

    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    Ok(neighborhood_id)
}

/// Add a home as a member of the active neighborhood and apply allocation budget.
///
/// The workflow is idempotent per home ID for the currently active neighborhood.
pub async fn add_home_to_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
) -> Result<(), AuraError> {
    let (homes_state, neighborhood_state) = {
        let mut core = app_core.write().await;
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
                    .map_err(|e| AuraError::budget_exceeded(e.to_string()))?;
            }
        }

        core.views_mut().set_homes(homes.clone());
        core.views_mut().set_neighborhood(neighborhood.clone());
        (homes, neighborhood)
    };

    emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    Ok(())
}

/// Force direct one_hop_link between local home and the target home in the active neighborhood.
pub async fn link_home_one_hop_link(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
) -> Result<(), AuraError> {
    let neighborhood_state = {
        let mut core = app_core.write().await;
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

        let updated_neighbor = NeighborHome {
            id: target_home_id,
            name: target_home_name,
            one_hop_link: OneHopLinkType::Direct,
            shared_contacts: 0,
            member_count: target_member_count,
            can_traverse: true,
        };

        neighborhood.add_neighbor(updated_neighbor);
        core.views_mut().set_neighborhood(neighborhood.clone());
        neighborhood
    };

    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    Ok(())
}

/// Create a home and update homes/neighborhood view state.
///
/// This is currently a local-first workflow. It creates a deterministic home ID,
/// updates `HOMES_SIGNAL`, and makes the home visible in `NEIGHBORHOOD_SIGNAL`.
async fn create_home_with_creator(
    app_core: &Arc<RwLock<AppCore>>,
    creator: AuthorityId,
    name: Option<String>,
    description: Option<String>,
) -> Result<ChannelId, AuraError> {
    let timestamp_ms = local_first_timestamp_ms(app_core).await;
    let home_name = name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
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

    let (homes_state, neighborhood_state) = {
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

        let mut neighborhood = core.views().get_neighborhood();
        if neighborhood.home_name.is_empty() || neighborhood.home_home_id == ChannelId::default() {
            neighborhood.home_home_id = home_id;
            neighborhood.home_name = home_name.clone();
            neighborhood.position = Some(TraversalPosition {
                current_home_id: home_id,
                current_home_name: home_name.clone(),
                depth: 2, // Full
                path: vec![home_id],
            });
        } else if should_promote_to_primary {
            neighborhood.position = Some(TraversalPosition {
                current_home_id: home_id,
                current_home_name: home_name.clone(),
                depth: 2, // Full
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

        core.views_mut().set_homes(homes.clone());
        core.views_mut().set_neighborhood(neighborhood.clone());
        (homes, neighborhood)
    };

    emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    let _ = description;
    Ok(home_id)
}

/// Create a home for the active authority and return its channel id.
pub async fn create_home(
    app_core: &Arc<RwLock<AppCore>>,
    name: Option<String>,
    description: Option<String>,
) -> Result<ChannelId, AuraError> {
    let creator = {
        let core = app_core.read().await;
        core.runtime()
            .map(|r| r.authority_id())
            .or_else(|| core.authority().copied())
    }
    .ok_or_else(|| AuraError::permission_denied("Authority not set"))?;

    create_home_with_creator(app_core, creator, name, description).await
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

/// Ensure a local-first home projection remains present in views/signals until
/// runtime-backed facts supersede it.
pub async fn ensure_local_home_projection(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: ChannelId,
    home_name: String,
    creator: AuthorityId,
) -> Result<(), AuraError> {
    let context_id =
        ContextId::new_from_entropy(hash(format!("home-context:{creator}:{home_id}").as_bytes()));
    let timestamp_ms = local_first_timestamp_ms(app_core).await;

    let (homes_state, neighborhood_state) = {
        let mut core = app_core.write().await;
        let mut homes = core.views().get_homes();
        if !homes.has_home(&home_id) {
            let mut home = HomeState::new(
                home_id,
                Some(home_name.clone()),
                creator,
                timestamp_ms,
                context_id,
            );
            if homes.is_empty()
                || homes
                    .current_home()
                    .map(|current| current.id == ChannelId::default())
                    .unwrap_or(true)
            {
                home.is_primary = true;
            }
            let _ = homes.add_home(home);
        }
        homes.select_home(Some(home_id));

        let mut neighborhood = core.views().get_neighborhood();
        let should_set_local_home = neighborhood.home_home_id == ChannelId::default()
            || neighborhood.home_name.trim().is_empty()
            || neighborhood
                .position
                .as_ref()
                .map(|position| position.current_home_id == ChannelId::default())
                .unwrap_or(true);
        if should_set_local_home {
            neighborhood.home_home_id = home_id;
            neighborhood.home_name = home_name.clone();
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

        core.views_mut().set_homes(homes.clone());
        core.views_mut().set_neighborhood(neighborhood.clone());
        (homes, neighborhood)
    };

    emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;
    Ok(())
}

/// Get current neighborhood state
///
/// **What it does**: Reads neighborhood state from views
/// **Returns**: Current neighborhood state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_neighborhood_state(app_core: &Arc<RwLock<AppCore>>) -> NeighborhoodState {
    let core = app_core.read().await;
    core.views().get_neighborhood()
}

/// Return the canonical missing-active-home guidance.
pub const fn missing_active_home_message() -> &'static str {
    MISSING_ACTIVE_HOME_MESSAGE
}

/// Resolve an active home/context with deterministic fallback behavior.
pub async fn resolve_active_home(
    app_core: &Arc<RwLock<AppCore>>,
    home_hint: Option<ChannelId>,
) -> Result<ActiveHomeResolution, AuraError> {
    let homes = homes_state_signal_fallback(app_core).await;

    if let Some(home_id) = home_hint {
        if let Some(home_state) = homes.home_state(&home_id) {
            return resolution_from_home(home_id, home_state, ActiveHomeSource::ExplicitHint);
        }
    }

    if let Some(home_state) = homes.current_home() {
        return resolution_from_home(home_state.id, home_state, ActiveHomeSource::Selected);
    }

    if let Some((home_id, home_state)) = fallback_home(&homes) {
        return resolution_from_home(home_id, &home_state, ActiveHomeSource::Fallback);
    }

    Err(AuraError::not_found(MISSING_ACTIVE_HOME_MESSAGE))
}

/// Resolve the active home id with deterministic fallback behavior.
pub async fn current_home_id_or_fallback(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ChannelId, AuraError> {
    Ok(resolve_active_home(app_core, None).await?.home_id)
}

/// Get current home context id with a deterministic fallback.
pub async fn current_home_context_or_fallback(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ContextId, AuraError> {
    Ok(resolve_active_home(app_core, None).await?.context_id)
}

/// Stable default relational context for an authority when no home-scoped context applies.
#[must_use]
pub fn authority_default_relational_context(authority_id: AuthorityId) -> ContextId {
    ContextId::new_from_entropy(hash(&authority_id.to_bytes()))
}

/// Resolve a chat-capable context, preferring the active home and falling back to
/// the authority default context when no home has been selected yet.
pub async fn current_home_context_or_authority_default(
    app_core: &Arc<RwLock<AppCore>>,
    authority_id: AuthorityId,
) -> Result<(ContextId, bool), AuraError> {
    match resolve_active_home(app_core, None).await {
        Ok(resolution) => Ok((resolution.context_id, true)),
        Err(AuraError::NotFound { message }) if message == MISSING_ACTIVE_HOME_MESSAGE => {
            Ok((authority_default_relational_context(authority_id), false))
        }
        Err(error) => Err(error),
    }
}

/// Stable fallback context for relational facts that should not depend on UI selection.
pub fn default_relational_context() -> ContextId {
    ContextId::new_from_entropy(hash(b"relational-context:default"))
}

/// Get current traversal position
///
/// **What it does**: Reads current position from neighborhood state
/// **Returns**: Optional traversal position
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_current_position(app_core: &Arc<RwLock<AppCore>>) -> Option<TraversalPosition> {
    let core = app_core.read().await;
    let neighborhood = core.views().get_neighborhood();
    neighborhood.position
}

/// Initialize HOMES_SIGNAL with a default test home.
///
/// This is a test helper that creates a home and populates HOMES_SIGNAL
/// so that tests have a valid current home available.
///
/// **Signal pattern**: Emits HOMES_SIGNAL
pub async fn initialize_test_home(
    app_core: &Arc<RwLock<AppCore>>,
    name: &str,
    authority_id: aura_core::identifiers::AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    use crate::signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME};
    use crate::views::home::HomeState;
    use crate::workflows::signals::emit_signal;
    use aura_core::crypto::hash::hash;

    // Create a deterministic home ID from the name
    let home_id = ChannelId::from_bytes(hash(format!("test-home:{name}").as_bytes()));

    // Create a context ID for the home
    let context_id = ContextId::new_from_entropy(hash(format!("test-context:{name}").as_bytes()));

    // Create the home state
    let home_state = HomeState::new(
        home_id,
        Some(name.to_string()),
        authority_id,
        timestamp_ms,
        context_id,
    );

    // Add to HOMES_SIGNAL
    let homes = {
        let mut core = app_core.write().await;
        let mut homes = core.views().get_homes();
        homes.add_home(home_state);
        core.views_mut().set_homes(homes.clone());
        homes
    };

    // Emit to ReactiveEffects subscribers
    emit_signal(app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME).await?;

    Ok(home_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::home::HomeState;
    use crate::AppConfig;
    use aura_core::crypto::hash::hash;

    #[tokio::test]
    async fn test_set_context() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = set_context(&app_core, Some("context-123".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("context-123".to_string()));

        let result = set_context(&app_core, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_get_neighborhood_state() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let state = get_neighborhood_state(&app_core).await;
        assert!(state.neighbors_is_empty());
    }

    #[tokio::test]
    async fn test_resolve_active_home_uses_selected_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let authority = aura_core::identifiers::AuthorityId::new_from_entropy([21u8; 32]);

        let selected_home = ChannelId::from_bytes(hash(b"selected-home"));
        let selected_ctx = ContextId::new_from_entropy(hash(b"selected-ctx"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home_with_auto_select(HomeState::new(
                selected_home,
                Some("Selected".to_string()),
                authority,
                1,
                selected_ctx,
            ));
            core.views_mut().set_homes(homes);
        }

        let resolved = resolve_active_home(&app_core, None).await.unwrap();
        assert_eq!(resolved.home_id, selected_home);
        assert_eq!(resolved.context_id, selected_ctx);
        assert_eq!(resolved.source, ActiveHomeSource::Selected);
    }

    #[tokio::test]
    async fn test_resolve_active_home_uses_deterministic_fallback() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let authority = aura_core::identifiers::AuthorityId::new_from_entropy([22u8; 32]);

        let home_a = ChannelId::from_bytes(hash(b"home-z"));
        let home_b = ChannelId::from_bytes(hash(b"home-a"));
        let ctx_a = ContextId::new_from_entropy(hash(b"ctx-z"));
        let ctx_b = ContextId::new_from_entropy(hash(b"ctx-a"));
        let expected_home = if home_a.as_bytes() <= home_b.as_bytes() {
            home_a
        } else {
            home_b
        };
        let expected_ctx = if expected_home == home_a {
            ctx_a
        } else {
            ctx_b
        };

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                home_a,
                Some("Zeta".to_string()),
                authority,
                1,
                ctx_a,
            ));
            homes.add_home(HomeState::new(
                home_b,
                Some("Alpha".to_string()),
                authority,
                2,
                ctx_b,
            ));
            homes.select_home(None);
            core.views_mut().set_homes(homes);
        }

        let resolved = resolve_active_home(&app_core, None).await.unwrap();
        assert_eq!(resolved.home_id, expected_home);
        assert_eq!(resolved.context_id, expected_ctx);
        assert_eq!(resolved.source, ActiveHomeSource::Fallback);
    }

    #[tokio::test]
    async fn test_resolve_active_home_returns_guidance_when_missing() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let error = resolve_active_home(&app_core, None).await.unwrap_err();
        assert!(error.to_string().contains(MISSING_ACTIVE_HOME_MESSAGE));
    }

    #[tokio::test]
    async fn test_current_home_context_or_authority_default_uses_active_home_when_available() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let authority = aura_core::identifiers::AuthorityId::new_from_entropy([31u8; 32]);
        let home_id = ChannelId::from_bytes(hash(b"chat-home"));
        let home_ctx = ContextId::new_from_entropy(hash(b"chat-home-ctx"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home_with_auto_select(HomeState::new(
                home_id,
                Some("Chat Home".to_string()),
                authority,
                1,
                home_ctx,
            ));
            core.views_mut().set_homes(homes);
        }

        let (resolved_ctx, used_home_context) =
            current_home_context_or_authority_default(&app_core, authority)
                .await
                .expect("context should resolve");
        assert_eq!(resolved_ctx, home_ctx);
        assert!(used_home_context);
    }

    #[tokio::test]
    async fn test_current_home_context_or_authority_default_falls_back_without_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let authority = aura_core::identifiers::AuthorityId::new_from_entropy([32u8; 32]);

        let (resolved_ctx, used_home_context) =
            current_home_context_or_authority_default(&app_core, authority)
                .await
                .expect("fallback context should resolve");
        assert_eq!(
            resolved_ctx,
            authority_default_relational_context(authority)
        );
        assert!(!used_home_context);
    }

    #[tokio::test]
    async fn test_move_position_selects_known_target_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let authority = aura_core::identifiers::AuthorityId::new_from_entropy([11u8; 32]);

        let home_a = ChannelId::from_bytes(hash(b"home-a"));
        let home_b = ChannelId::from_bytes(hash(b"home-b"));
        let ctx_a = ContextId::new_from_entropy(hash(b"ctx-a"));
        let ctx_b = ContextId::new_from_entropy(hash(b"ctx-b"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home_with_auto_select(HomeState::new(
                home_a,
                Some("Alpha".to_string()),
                authority,
                1,
                ctx_a,
            ));
            homes.add_home(HomeState::new(
                home_b,
                Some("Beta".to_string()),
                authority,
                2,
                ctx_b,
            ));
            homes.select_home(Some(home_a));
            core.views_mut().set_homes(homes);

            let mut neighborhood = core.views().get_neighborhood();
            neighborhood.home_home_id = home_a;
            neighborhood.home_name = "Alpha".to_string();
            neighborhood.add_neighbor(NeighborHome {
                id: home_b,
                name: "Beta".to_string(),
                one_hop_link: OneHopLinkType::Direct,
                shared_contacts: 0,
                member_count: Some(1),
                can_traverse: true,
            });
            core.views_mut().set_neighborhood(neighborhood);
        }

        move_position(&app_core, &home_b.to_string(), "full")
            .await
            .unwrap();

        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let neighborhood = core.views().get_neighborhood();

        assert_eq!(homes.current_home_id().copied(), Some(home_b));
        assert_eq!(
            neighborhood.position.as_ref().map(|p| p.current_home_id),
            Some(home_b)
        );
        assert_eq!(neighborhood.position.as_ref().map(|p| p.depth), Some(2));
    }
}
