use super::*;
use crate::workflows::channel_ref::ChannelSelector;

pub(super) fn parse_channel_ref(channel: &str) -> Result<ChannelSelector, AuraError> {
    ChannelSelector::parse(channel)
}

pub(super) fn channel_id_from_input(channel: &str) -> Result<ChannelId, AuraError> {
    Ok(parse_channel_ref(channel)?.to_channel_id())
}

pub(super) async fn resolve_channel_id_from_state_or_input(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<ChannelId, AuraError> {
    let selector = parse_channel_ref(channel_input)?;
    if let ChannelSelector::Id(channel_id) = &selector {
        return Ok(*channel_id);
    }
    let raw = channel_input.trim();

    let normalized_name = raw.trim_start_matches('#').trim();
    let normalized_lower = normalized_name.to_ascii_lowercase();
    let raw_lower = raw.to_ascii_lowercase();

    let chat = chat_snapshot(app_core).await;
    if let Some(existing) = chat.all_channels().find(|channel| {
        let id = channel.id.to_string();
        let id_lower = id.to_ascii_lowercase();
        id == raw
            || id_lower == raw_lower
            || channel.name.eq_ignore_ascii_case(normalized_name)
            || format!("# {}", channel.name).eq_ignore_ascii_case(raw)
            || format!("#{}", channel.name).eq_ignore_ascii_case(raw)
            || format!("home:{}", channel.id).eq_ignore_ascii_case(raw)
            || channel.name.to_ascii_lowercase() == normalized_lower
    }) {
        return Ok(existing.id);
    }

    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    if let Some(home_id) = homes
        .iter()
        .filter_map(|(home_id, home)| {
            let home_name = home.name.trim();
            let is_match = home_id.to_string().eq_ignore_ascii_case(raw)
                || format!("home:{home_id}").eq_ignore_ascii_case(raw)
                || (!home_name.is_empty() && home_name.eq_ignore_ascii_case(normalized_name))
                || (!home_name.is_empty() && format!("#{home_name}").eq_ignore_ascii_case(raw))
                || (!home_name.is_empty() && format!("# {home_name}").eq_ignore_ascii_case(raw));
            is_match.then_some((*home_id, home.is_admin(), home.member_count))
        })
        .max_by_key(|(_, is_admin, member_count)| (u8::from(*is_admin), *member_count))
        .map(|(home_id, _, _)| home_id)
    {
        return Ok(home_id);
    }

    Ok(selector.to_channel_id())
}

pub(super) async fn matching_channel_ids(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Vec<ChannelId> {
    let raw = channel_input.trim();
    if raw.is_empty() {
        return Vec::new();
    }

    let normalized_name = raw.trim_start_matches('#').trim();
    let normalized_lower = normalized_name.to_ascii_lowercase();
    let raw_lower = raw.to_ascii_lowercase();

    let chat = chat_snapshot(app_core).await;
    let mut matches = Vec::new();
    for channel in chat.all_channels() {
        let id = channel.id.to_string();
        let id_lower = id.to_ascii_lowercase();
        let is_match = id == raw
            || id_lower == raw_lower
            || channel.name.eq_ignore_ascii_case(normalized_name)
            || format!("# {}", channel.name).eq_ignore_ascii_case(raw)
            || format!("#{}", channel.name).eq_ignore_ascii_case(raw)
            || format!("home:{}", channel.id).eq_ignore_ascii_case(raw)
            || channel.name.to_ascii_lowercase() == normalized_lower;

        if is_match {
            matches.push(channel.id);
        }
    }

    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    for (home_id, home) in homes.iter() {
        let home_name = home.name.trim();
        let is_match = home_id.to_string().eq_ignore_ascii_case(raw)
            || format!("home:{home_id}").eq_ignore_ascii_case(raw)
            || (!home_name.is_empty() && home_name.eq_ignore_ascii_case(normalized_name))
            || (!home_name.is_empty() && format!("#{home_name}").eq_ignore_ascii_case(raw))
            || (!home_name.is_empty() && format!("# {home_name}").eq_ignore_ascii_case(raw));
        if is_match && !matches.contains(home_id) {
            matches.push(*home_id);
        }
    }

    matches
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

pub(super) async fn context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    local_authority: Option<AuthorityId>,
) -> Result<ContextId, AuraError> {
    {
        let chat = chat_snapshot(app_core).await;
        if let Some(channel) = chat.channel(&channel_id) {
            if let Some(ctx_id) = channel.context_id {
                return Ok(ctx_id);
            }
            if channel.is_dm {
                if let Some(self_authority) = local_authority {
                    if let Some(peer_authority) = channel
                        .member_ids
                        .iter()
                        .copied()
                        .find(|member| *member != self_authority)
                        .or_else(|| channel.member_ids.first().copied())
                    {
                        return Ok(pair_dm_context_id(self_authority, peer_authority));
                    }
                }
            }
        }
    }
    {
        let core = app_core.read().await;
        let homes = core.views().get_homes();
        if let Some(home_state) = homes.home_state(&channel_id) {
            if let Some(ctx_id) = home_state.context_id {
                return Ok(ctx_id);
            }
        }
    }

    current_home_context_or_fallback(app_core).await
}
