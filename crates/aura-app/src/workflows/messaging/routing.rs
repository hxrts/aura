use super::*;
use crate::views::chat::{
    is_note_to_self_channel_name, note_to_self_channel_id, note_to_self_context_id,
};
use crate::workflows::channel_ref::ChannelSelector;
use crate::workflows::error::{self, WorkflowError};

pub(super) fn parse_channel_ref(channel: &str) -> Result<ChannelSelector, AuraError> {
    ChannelSelector::parse(channel)
}

pub(super) fn channel_id_from_input(channel: &str) -> Result<ChannelId, AuraError> {
    Ok(parse_channel_ref(channel)?.to_channel_id())
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(super) async fn resolve_chat_channel_id_from_state_or_input(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<ChannelId, AuraError> {
    let selector = parse_channel_ref(channel_input)?;
    if let ChannelSelector::Id(channel_id) = &selector {
        return Ok(*channel_id);
    }
    let raw = channel_input.trim();

    let normalized_name = raw.trim_start_matches('#').trim();

    let local_authority = {
        let core = app_core.read().await;
        core.authority().cloned()
    };
    if is_note_to_self_channel_name(normalized_name) {
        if let Some(authority_id) = local_authority {
            return Ok(note_to_self_channel_id(authority_id));
        }
    }

    if let Ok(runtime) = require_runtime(app_core).await {
        return runtime
            .resolve_authoritative_channel_ids_by_name(normalized_name)
            .await
            .map_err(|error| {
                error::runtime_call("resolve authoritative channel ids by name", error)
            })?
            .into_iter()
            .next()
            .ok_or_else(|| AuraError::not_found(normalized_name.to_string()));
    }

    // OWNERSHIP: observed
    // Local-only fallback when no authoritative runtime exists.
    let chat = observed_chat_snapshot(app_core).await;
    if let Some(channel_id) = chat
        .all_channels()
        .find(|channel| channel.name.eq_ignore_ascii_case(normalized_name))
        .map(|channel| channel.id)
    {
        return Ok(channel_id);
    }

    Ok(selector.to_channel_id())
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(super) async fn matching_chat_channel_ids(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Vec<ChannelId> {
    let raw = channel_input.trim();
    if raw.is_empty() {
        return Vec::new();
    }

    let normalized_name = raw.trim_start_matches('#').trim();

    let local_authority = {
        let core = app_core.read().await;
        core.authority().cloned()
    };

    if is_note_to_self_channel_name(normalized_name) {
        if let Some(authority_id) = local_authority {
            let channel_id = note_to_self_channel_id(authority_id);
            return vec![channel_id];
        }
    }

    if let Ok(runtime) = require_runtime(app_core).await {
        return runtime
            .resolve_authoritative_channel_ids_by_name(normalized_name)
            .await
            .unwrap_or_default();
    }

    // OWNERSHIP: observed
    // Local-only fallback when no authoritative runtime exists.
    let chat = observed_chat_snapshot(app_core).await;
    chat.all_channels()
        .filter(|channel| channel.name.eq_ignore_ascii_case(normalized_name))
        .map(|channel| channel.id)
        .collect()
}

pub(super) async fn resolve_target_authority_for_invite(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
) -> Result<AuthorityId, AuraError> {
    if let Ok(contact) = crate::workflows::query::resolve_contact(app_core, target_user_id).await {
        return Ok(contact.id);
    }
    parse_authority_id(target_user_id)
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(super) async fn context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    local_authority: Option<AuthorityId>,
) -> Result<ContextId, AuraError> {
    if let Some(authority_id) = local_authority {
        if channel_id == note_to_self_channel_id(authority_id) {
            return Ok(note_to_self_context_id(authority_id));
        }
    }
    let runtime = require_runtime(app_core).await?;
    runtime
        .resolve_amp_channel_context(channel_id)
        .await
        .map_err(|error| error::runtime_call("resolve authoritative channel context", error))?
        .ok_or_else(|| {
            WorkflowError::MissingAuthoritativeContext {
                channel: channel_id.to_string(),
            }
            .into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{runtime_bridge::OfflineRuntimeBridge, AppConfig, AppCore};
    use aura_core::crypto::hash::hash;

    #[tokio::test]
    async fn resolve_chat_channel_id_uses_authoritative_runtime_lookup() {
        let authority = AuthorityId::new_from_entropy([9u8; 32]);
        let bridge = Arc::new(OfflineRuntimeBridge::new(authority));
        let canonical_id = ChannelId::from_bytes(hash(b"routing-canonical"));
        bridge.set_authoritative_channel_name_matches("shared-parity-lab", vec![canonical_id]);
        let core = AppCore::with_runtime(AppConfig::default(), bridge).expect("runtime-backed app");
        let app_core = Arc::new(RwLock::new(core));

        let resolved = resolve_chat_channel_id_from_state_or_input(&app_core, "#shared-parity-lab")
            .await
            .expect("matching channel");

        assert_eq!(resolved, canonical_id);
    }

    #[tokio::test]
    async fn context_id_for_channel_uses_authoritative_runtime_context() {
        let authority = AuthorityId::new_from_entropy([10u8; 32]);
        let bridge = Arc::new(OfflineRuntimeBridge::new(authority));
        let channel_id = ChannelId::from_bytes(hash(b"routing-channel-context"));
        let context_id = ContextId::new_from_entropy([11u8; 32]);
        bridge.set_amp_channel_context(channel_id, context_id);
        let core = AppCore::with_runtime(AppConfig::default(), bridge).expect("runtime-backed app");
        let app_core = Arc::new(RwLock::new(core));

        let resolved = context_id_for_channel(&app_core, channel_id, Some(authority))
            .await
            .expect("authoritative context");

        assert_eq!(resolved, context_id);
    }
}
