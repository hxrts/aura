use super::*;
use crate::views::chat::{
    is_note_to_self_channel_name, note_to_self_channel_id, note_to_self_context_id, Channel,
};
use crate::workflows::channel_ref::ChannelSelector;
use crate::workflows::context::current_home_context_or_fallback;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ChannelMatchClass {
    Name,
    Id,
}

fn channel_matches_input(
    channel: &Channel,
    raw: &str,
    raw_lower: &str,
    normalized_name: &str,
    normalized_lower: &str,
) -> Option<ChannelMatchClass> {
    let id = channel.id.to_string();
    let id_lower = id.to_ascii_lowercase();

    if id == raw || id_lower == raw_lower {
        return Some(ChannelMatchClass::Id);
    }
    if channel.name.eq_ignore_ascii_case(normalized_name)
        || format!("# {}", channel.name).eq_ignore_ascii_case(raw)
        || format!("#{}", channel.name).eq_ignore_ascii_case(raw)
        || channel.name.to_ascii_lowercase() == normalized_lower
    {
        return Some(ChannelMatchClass::Name);
    }

    None
}

fn better_chat_match(
    candidate: (&Channel, ChannelMatchClass),
    current: (&Channel, ChannelMatchClass),
) -> bool {
    let (candidate_channel, candidate_class) = candidate;
    let (current_channel, current_class) = current;
    let candidate_key = (
        candidate_class,
        candidate_channel.context_id.is_some(),
        candidate_channel.member_count,
        candidate_channel.last_activity,
        std::cmp::Reverse(candidate_channel.id.to_string()),
    );
    let current_key = (
        current_class,
        current_channel.context_id.is_some(),
        current_channel.member_count,
        current_channel.last_activity,
        std::cmp::Reverse(current_channel.id.to_string()),
    );
    candidate_key > current_key
}

fn resolve_matching_chat_channel_candidate<'a>(
    chat: &'a crate::views::chat::ChatState,
    raw: &str,
    raw_lower: &str,
    normalized_name: &str,
    normalized_lower: &str,
) -> Option<(&'a Channel, ChannelMatchClass)> {
    let mut best: Option<(&Channel, ChannelMatchClass)> = None;
    for channel in chat.all_channels() {
        let Some(class) =
            channel_matches_input(channel, raw, raw_lower, normalized_name, normalized_lower)
        else {
            continue;
        };
        match best {
            None => best = Some((channel, class)),
            Some(current) if better_chat_match((channel, class), current) => {
                best = Some((channel, class));
            }
            Some(_) => {}
        }
    }
    best
}

pub(super) fn parse_channel_ref(channel: &str) -> Result<ChannelSelector, AuraError> {
    ChannelSelector::parse(channel)
}

pub(super) fn channel_id_from_input(channel: &str) -> Result<ChannelId, AuraError> {
    Ok(parse_channel_ref(channel)?.to_channel_id())
}

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
    let normalized_lower = normalized_name.to_ascii_lowercase();
    let raw_lower = raw.to_ascii_lowercase();

    let local_authority = {
        let core = app_core.read().await;
        core.authority().cloned()
    };
    if is_note_to_self_channel_name(normalized_name) {
        if let Some(authority_id) = local_authority {
            return Ok(note_to_self_channel_id(authority_id));
        }
    }

    let chat = chat_snapshot(app_core).await;
    if let Some((channel, _)) = resolve_matching_chat_channel_candidate(
        &chat,
        raw,
        &raw_lower,
        normalized_name,
        &normalized_lower,
    ) {
        return Ok(channel.id);
    }

    Ok(selector.to_channel_id())
}

pub(super) async fn matching_chat_channel_ids(
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

    let local_authority = {
        let core = app_core.read().await;
        core.authority().cloned()
    };

    let chat = chat_snapshot(app_core).await;
    let mut matches = Vec::new();
    for channel in chat.all_channels() {
        if channel_matches_input(channel, raw, &raw_lower, normalized_name, &normalized_lower)
            .is_some()
        {
            matches.push(channel.id);
        }
    }

    if is_note_to_self_channel_name(normalized_name) {
        if let Some(authority_id) = local_authority {
            let channel_id = note_to_self_channel_id(authority_id);
            if !matches.contains(&channel_id) {
                matches.push(channel_id);
            }
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
    if let Some(authority_id) = local_authority {
        if channel_id == note_to_self_channel_id(authority_id) {
            return Ok(note_to_self_context_id(authority_id));
        }
    }

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
        let mut homes = {
            let core = app_core.read().await;
            core.views().get_homes()
        };
        if homes.iter().next().is_none() {
            let signal_homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
                .await
                .unwrap_or_default();
            if signal_homes.iter().next().is_some() {
                homes = signal_homes;
            }
        }
        if let Some(home_state) = homes.home_state(&channel_id) {
            if let Some(ctx_id) = home_state.context_id {
                return Ok(ctx_id);
            }
        }
    }

    current_home_context_or_fallback(app_core).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::chat::{Channel, ChannelType, ChatState};
    use aura_core::crypto::hash::hash;

    fn test_channel(
        id: ChannelId,
        name: &str,
        context_id: Option<ContextId>,
        member_count: u32,
        last_activity: u64,
    ) -> Channel {
        Channel {
            id,
            context_id,
            name: name.to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count,
            last_message: None,
            last_message_time: None,
            last_activity,
            last_finalized_epoch: 0,
        }
    }

    #[test]
    fn resolve_matching_chat_channel_prefers_context_backed_candidate_for_name_match() {
        let stale_id = ChannelId::from_bytes(hash(b"routing-stale"));
        let canonical_id = ChannelId::from_bytes(hash(b"routing-canonical"));
        let chat = ChatState::from_channels([
            test_channel(stale_id, "shared-parity-lab", None, 1, 10),
            test_channel(
                canonical_id,
                "shared-parity-lab",
                Some(ContextId::new_from_entropy([9u8; 32])),
                2,
                20,
            ),
        ]);

        let resolved = resolve_matching_chat_channel(
            &chat,
            "#shared-parity-lab",
            "#shared-parity-lab",
            "shared-parity-lab",
            "shared-parity-lab",
        )
        .expect("matching channel");

        assert_eq!(resolved, canonical_id);
    }

    #[test]
    fn resolve_matching_chat_channel_prefers_exact_id_over_name_match() {
        let stale_id = ChannelId::from_bytes(hash(b"routing-exact-id"));
        let name_only_id = ChannelId::from_bytes(hash(b"routing-name-only"));
        let stale_id_str = stale_id.to_string();
        let stale_id_lower = stale_id_str.to_ascii_lowercase();
        let chat = ChatState::from_channels([
            test_channel(name_only_id, "shared-parity-lab", None, 2, 50),
            test_channel(stale_id, "other-name", None, 1, 1),
        ]);

        let resolved = resolve_matching_chat_channel(
            &chat,
            &stale_id_str,
            &stale_id_lower,
            "shared-parity-lab",
            "shared-parity-lab",
        )
        .expect("matching channel");

        assert_eq!(resolved, stale_id);
    }
}
