use std::sync::Arc;

use async_lock::RwLock;
use aura_core::{
    crypto::hash::hash,
    types::{AuthorityId, ChannelId, ContextId},
    AuraError,
};

use crate::{
    views::home::HomeState, workflows::observed_projection::homes_signal_snapshot, AppCore,
};

const MISSING_ACTIVE_HOME_MESSAGE: &str =
    "No home exists yet. Create a home from the Neighborhood screen to start group channels.";

/// Source of active-home resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveHomeSource {
    /// Home resolved from the currently selected home.
    Selected,
}

/// Active-home resolution result shared by context-dependent workflows.
#[aura_macros::strong_reference(domain = "home")]
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
        .ok_or_else(|| AuraError::not_found(home_id.to_string()))?;
    Ok(ActiveHomeResolution {
        home_id,
        context_id,
        source,
    })
}

#[aura_macros::authoritative_source(kind = "app_core")]
async fn authoritative_active_home_selection(app_core: &Arc<RwLock<AppCore>>) -> Option<ChannelId> {
    let core = app_core.read().await;
    core.active_home_selection()
}

/// Return the canonical missing-active-home guidance.
pub const fn missing_active_home_message() -> &'static str {
    MISSING_ACTIVE_HOME_MESSAGE
}

#[aura_macros::authoritative_source(kind = "app_core")]
async fn bound_authority_id(app_core: &Arc<RwLock<AppCore>>) -> Option<AuthorityId> {
    let core = app_core.read().await;
    core.authority()
        .copied()
        .or_else(|| core.runtime().map(|runtime| runtime.authority_id()))
}

/// Resolve an active home/context without implicit fallback behavior.
pub async fn resolve_active_home(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ActiveHomeResolution, AuraError> {
    let homes = homes_signal_snapshot(app_core).await?;

    if let Some(home_id) = authoritative_active_home_selection(app_core).await {
        if let Some(home_state) = homes.home_state(&home_id) {
            return resolution_from_home(home_id, home_state, ActiveHomeSource::Selected);
        }
    }

    if let Some(home_state) = homes.current_home() {
        return resolution_from_home(home_state.id, home_state, ActiveHomeSource::Selected);
    }

    Err(AuraError::not_found(MISSING_ACTIVE_HOME_MESSAGE))
}

/// Resolve the active home id.
pub async fn current_home_id(app_core: &Arc<RwLock<AppCore>>) -> Result<ChannelId, AuraError> {
    Ok(resolve_active_home(app_core).await?.home_id)
}

/// Get current home context id.
pub async fn current_home_context(app_core: &Arc<RwLock<AppCore>>) -> Result<ContextId, AuraError> {
    Ok(resolve_active_home(app_core).await?.context_id)
}

/// Resolve the context used for group channels.
///
/// Group creation always uses the local authority's stable default relational
/// context so standalone groups do not inherit home scoping implicitly.
pub async fn current_group_context(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ContextId, AuraError> {
    let authority_id = bound_authority_id(app_core)
        .await
        .ok_or_else(|| AuraError::not_found(MISSING_ACTIVE_HOME_MESSAGE))?;
    Ok(authority_default_relational_context(authority_id))
}

/// Stable default relational context for an authority when no home-scoped context applies.
#[must_use]
pub fn authority_default_relational_context(authority_id: AuthorityId) -> ContextId {
    ContextId::new_from_entropy(hash(&authority_id.to_bytes()))
}

/// Stable fallback context for relational facts that should not depend on UI selection.
pub fn default_relational_context() -> ContextId {
    ContextId::new_from_entropy(hash(b"relational-context:default"))
}
