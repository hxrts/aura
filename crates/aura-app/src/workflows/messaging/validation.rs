use super::*;

pub(super) fn is_invitation_capability_missing(error: &AuraError) -> bool {
    error.to_string().contains("invitation:capability-missing")
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(super) async fn authoritative_home_moderation_status(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    authority_id: AuthorityId,
    timestamp_ms: u64,
) -> Result<crate::runtime_bridge::AuthoritativeModerationStatus, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    }
    .ok_or_else(|| {
        AuraError::permission_denied("authoritative moderation status requires runtime")
    })?;

    runtime
        .moderation_status(context_id, channel_id, authority_id, timestamp_ms)
        .await
        .map_err(|error| {
            crate::workflows::error::runtime_call("authoritative moderation status", error).into()
        })
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
    let status = authoritative_home_moderation_status(
        app_core,
        context_id,
        channel_id,
        sender_id,
        timestamp_ms,
    )
    .await?;

    if status.is_banned {
        return Err(AuraError::permission_denied(
            "You are banned from this home and cannot send messages",
        ));
    }

    if status.is_muted {
        return Err(AuraError::permission_denied(
            "You are muted in this home and cannot send messages",
        ));
    }

    if status.roster_known && !status.is_member {
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
    let status =
        authoritative_home_moderation_status(app_core, context_id, channel_id, authority_id, 0)
            .await?;
    if status.is_banned {
        return Err(AuraError::permission_denied(
            "You are banned from this home and cannot join channels",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppConfig, AppCore};

    #[tokio::test]
    async fn authoritative_home_moderation_status_reads_runtime_owned_status() {
        let authority = AuthorityId::new_from_entropy([91u8; 32]);
        let target = AuthorityId::new_from_entropy([92u8; 32]);
        let context_id = ContextId::new_from_entropy([93u8; 32]);
        let channel_id = ChannelId::from_bytes([94u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        runtime.set_moderation_status(
            context_id,
            channel_id,
            target,
            crate::runtime_bridge::AuthoritativeModerationStatus {
                is_banned: true,
                is_muted: true,
                roster_known: true,
                is_member: false,
            },
        );
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime).unwrap(),
        ));

        let status =
            authoritative_home_moderation_status(&app_core, context_id, channel_id, target, 1_000)
                .await
                .expect("runtime-backed moderation status should resolve");

        assert!(status.is_banned);
        assert!(status.is_muted);
        assert!(status.roster_known);
        assert!(!status.is_member);
    }
}
