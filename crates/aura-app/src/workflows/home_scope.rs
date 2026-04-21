#[cfg_attr(not(test), allow(unused_imports))]
use crate::views::home::{HomeState, HomesState};
use crate::workflows::channel_ref::ChannelSelector;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{require_runtime, timeout_runtime_call};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::AuraError;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] /* Cleanup target (2026-07): remove after every channel-hint consumer reads the context hint or the field is deleted. */
pub(crate) struct MaterializedChannelHint {
    pub(crate) channel_id: ChannelId,
    pub(crate) context_id: Option<ContextId>,
}

pub(crate) async fn resolve_target_authority(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<AuthorityId, AuraError> {
    if let Ok(contact) = crate::workflows::query::resolve_contact(app_core, target).await {
        return Ok(contact.id);
    }
    parse_authority_id(target)
}

pub(crate) async fn identify_materialized_channel_hint(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    workflow_name: &'static str,
    error_action: &'static str,
    timeout: Duration,
) -> Result<MaterializedChannelHint, AuraError> {
    let runtime = require_runtime(app_core).await?;
    match ChannelSelector::parse(channel)? {
        ChannelSelector::Id(channel_id) => Ok(MaterializedChannelHint {
            channel_id,
            context_id: None,
        }),
        ChannelSelector::Name(channel_name) => {
            let resolved = timeout_runtime_call(
                &runtime,
                workflow_name,
                "identify_materialized_channel_bindings_by_name",
                timeout,
                || runtime.identify_materialized_channel_bindings_by_name(&channel_name),
            )
            .await
            .map_err(|error| super::error::runtime_call(error_action, error))?
            .map_err(|error| super::error::runtime_call(error_action, error))?;
            match resolved.as_slice() {
                [] => Err(AuraError::not_found(channel_name)),
                [binding] => Ok(MaterializedChannelHint {
                    channel_id: binding.channel_id,
                    context_id: Some(binding.context_id),
                }),
                _ => Err(AuraError::invalid(format!(
                    "Ambiguous channel hint for {error_action}: {channel_name}"
                ))),
            }
        }
    }
}

#[allow(dead_code)] /* Cleanup target (2026-07): remove if moderator/moderation stop sharing this ranking helper. */
pub(crate) fn best_home_for_context_by<F>(
    homes: &HomesState,
    context_id: ContextId,
    rank: F,
) -> Option<(ChannelId, HomeState)>
where
    F: Fn(&HomeState) -> (u8, u8, usize),
{
    homes
        .iter()
        .filter(|(_, home)| home.context_id == Some(context_id))
        .map(|(home_id, home)| (*home_id, home.clone()))
        .max_by_key(|(_, home)| rank(home))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::fs;

    #[test]
    fn moderator_and_moderation_use_shared_home_scope_helpers() {
        let moderator = fs::read_to_string("src/workflows/moderator.rs")
            .expect("read moderator workflow source");
        let moderation = fs::read_to_string("src/workflows/moderation.rs")
            .expect("read moderation workflow source");

        for source in [&moderator, &moderation] {
            assert!(
                !source.contains("async fn resolve_target_authority("),
                "workflow should reuse shared home_scope::resolve_target_authority"
            );
            assert!(
                !source.contains("async fn identify_materialized_channel_hint("),
                "workflow should reuse shared home_scope::identify_materialized_channel_hint"
            );
        }
    }
}
