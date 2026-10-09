//! Opt-in native integration fixture setup, never a production frontend API.
//! Publication delegates the same graph-and-view owner used by workflows.

use crate::{views::home::HomesState, AppCore};
use async_lock::RwLock;
use aura_core::{AuraError, ChannelId};
use std::sync::Arc;

/// Publish an already materialized, detached home fixture through its owner.
pub async fn publish_homes_fixture(
    app: &Arc<RwLock<AppCore>>,
    homes: HomesState,
) -> Result<(), AuraError> {
    crate::workflows::observed_projection::update_homes_projection_observed(app, move |current| {
        *current = homes;
    })
    .await
}

/// Change only the mode of an exact existing fixture home; never insert one.
pub async fn set_home_mode_fixture(
    app: &Arc<RwLock<AppCore>>,
    home_id: ChannelId,
    mode_flags: Option<String>,
) -> Result<(), AuraError> {
    crate::workflows::observed_projection::try_update_homes_projection_observed(app, move |homes| {
        let home = homes
            .home_mut(&home_id)
            .ok_or_else(|| AuraError::not_found("fixture home is not materialized"))?;
        home.mode_flags = mode_flags;
        Ok(())
    })
    .await
}
