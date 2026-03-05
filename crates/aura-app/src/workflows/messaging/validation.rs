use super::*;

pub(super) fn is_invitation_capability_missing(error: &AuraError) -> bool {
    error.to_string().contains("invitation:capability-missing")
}

fn collect_home_candidates(
    homes: &crate::views::home::HomesState,
    context_id: ContextId,
    channel_id: ChannelId,
) -> Vec<crate::views::home::HomeState> {
    let mut candidates = Vec::new();

    for (_, home) in homes.iter() {
        if home.context_id == Some(context_id) {
            let known = candidates
                .iter()
                .any(|candidate: &crate::views::home::HomeState| candidate.id == home.id);
            if !known {
                candidates.push(home.clone());
            }
        }

        if home.id == channel_id {
            let known = candidates
                .iter()
                .any(|candidate: &crate::views::home::HomeState| candidate.id == home.id);
            if !known {
                candidates.push(home.clone());
            }
        }
    }

    if let Some(home) = homes.current_home() {
        let known = candidates.iter().any(|candidate| candidate.id == home.id);
        if !known {
            candidates.push(home.clone());
        }
    }

    candidates
}

pub(super) fn join_error_is_not_found(error: &AuraError) -> bool {
    if matches!(error, AuraError::NotFound { .. }) {
        return true;
    }

    let lowered = error.to_string().to_ascii_lowercase();
    lowered.contains("not found")
        || lowered.contains("unknown channel")
        || lowered.contains("no such channel")
}

pub(super) fn intent_error_is_not_found(error: &IntentError) -> bool {
    if matches!(error, IntentError::ContextNotFound { .. }) {
        return true;
    }

    let lowered = error.to_string().to_ascii_lowercase();
    lowered.contains("not found")
        || lowered.contains("unknown channel")
        || lowered.contains("no such channel")
}

pub(super) async fn enforce_home_moderation_for_sender(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    sender_id: AuthorityId,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let candidates = {
        let core = app_core.read().await;
        collect_home_candidates(&core.views().get_homes(), context_id, channel_id)
    };

    if candidates.is_empty() {
        return Ok(());
    }

    if candidates.iter().any(|home| home.is_banned(&sender_id)) {
        return Err(AuraError::permission_denied(
            "You are banned from this home and cannot send messages",
        ));
    }

    if candidates
        .iter()
        .any(|home| home.is_muted(&sender_id, timestamp_ms))
    {
        return Err(AuraError::permission_denied(
            "You are muted in this home and cannot send messages",
        ));
    }

    let has_member_roster = candidates.iter().any(|home| !home.members.is_empty());
    let sender_is_member = candidates
        .iter()
        .any(|home| home.member(&sender_id).is_some());
    if has_member_roster && !sender_is_member {
        return Err(AuraError::permission_denied(
            "You are not a member of this home",
        ));
    }

    Ok(())
}

pub(super) async fn enforce_home_join_allowed(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    authority_id: AuthorityId,
) -> Result<(), AuraError> {
    let candidates = {
        let core = app_core.read().await;
        collect_home_candidates(&core.views().get_homes(), context_id, channel_id)
    };

    if candidates.is_empty() {
        return Ok(());
    }

    if candidates.iter().any(|home| home.is_banned(&authority_id)) {
        return Err(AuraError::permission_denied(
            "You are banned from this home and cannot join channels",
        ));
    }

    Ok(())
}
