use super::scope::ModerationScope;
use crate::workflows::home_scope::{identify_materialized_channel_hint, resolve_target_authority};
use crate::workflows::observed_projection::{
    homes_signal_snapshot, replace_homes_projection_observed,
};
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, execute_with_runtime_retry_budget, timeout_runtime_call,
    workflow_retry_policy,
};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    types::identifiers::{AuthorityId, ChannelId},
    AuraError, RetryRunError,
};
use aura_journal::fact::RelationalFact;
use std::collections::BTreeSet;
use std::sync::Arc;

pub(super) enum ModerationCapability {
    Kick,
    Ban,
    Mute,
    Pin,
}

impl ModerationCapability {
    fn permission_message(&self) -> &'static str {
        match self {
            Self::Kick => "Only moderators with kick capability can kick members",
            Self::Ban => "Only moderators with ban capability can ban members",
            Self::Mute => "Only moderators with mute capability can mute members",
            Self::Pin => "Only moderators with pin capability can pin messages",
        }
    }
}

pub(super) fn require_capability(
    scope: &ModerationScope,
    capability: ModerationCapability,
) -> Result<(), AuraError> {
    if scope.can_moderate {
        Ok(())
    } else {
        Err(AuraError::permission_denied(
            capability.permission_message(),
        ))
    }
}

pub(super) async fn resolve_target_id(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<AuthorityId, AuraError> {
    resolve_target_authority(app_core, target).await
}

pub(super) async fn resolve_channel_hint(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
) -> Result<ChannelId, AuraError> {
    Ok(identify_materialized_channel_hint(
        app_core,
        channel,
        "identify_materialized_channel_hint",
        "resolve moderation channel",
        super::MODERATION_RUNTIME_TIMEOUT,
    )
    .await?
    .channel_id)
}

pub(super) async fn moderation_timestamp(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    operation: &'static str,
    stage: &'static str,
) -> Result<u64, AuraError> {
    Ok(timeout_runtime_call(
        runtime,
        operation,
        "current_time_ms",
        super::MODERATION_RUNTIME_TIMEOUT,
        || runtime.current_time_ms(),
    )
    .await
    .map_err(|e| super::super::error::runtime_call(stage, e))?
    .map_err(|e| super::super::error::runtime_call(stage, e))?)
}

pub(super) async fn send_moderation_fact_with_retry(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer: AuthorityId,
    context_id: aura_core::types::identifiers::ContextId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let retry_policy = workflow_retry_policy(
        super::MODERATION_FACT_SEND_MAX_ATTEMPTS as u32,
        std::time::Duration::from_millis(1),
        std::time::Duration::from_millis(1),
    )?;
    execute_with_runtime_retry_budget(runtime, &retry_policy, |attempt| {
        let runtime = Arc::clone(runtime);
        async move {
            if attempt > 0 {
                converge_runtime(&runtime).await;
                for _ in 0..super::MODERATION_FACT_SEND_YIELDS_PER_RETRY {
                    cooperative_yield().await;
                }
            }
            timeout_runtime_call(
                &runtime,
                "send_moderation_fact_with_retry",
                "send_chat_fact",
                super::MODERATION_RUNTIME_TIMEOUT,
                || runtime.send_chat_fact(peer, context_id, fact),
            )
            .await?
            .map_err(|error| {
                AuraError::from(super::super::error::WorkflowError::DeliveryFailed {
                    peer: peer.to_string(),
                    attempts: super::MODERATION_FACT_SEND_MAX_ATTEMPTS,
                    source: AuraError::agent(error.to_string()),
                })
            })
        }
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
    })
}

pub(super) async fn commit_and_fanout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    scope: &ModerationScope,
    fact: RelationalFact,
    extra_peers: &[AuthorityId],
) -> Result<(), AuraError> {
    timeout_runtime_call(
        runtime,
        "commit_and_fanout",
        "commit_relational_facts",
        super::MODERATION_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
    )
    .await
    .map_err(|e| super::super::error::runtime_call("commit moderation fact", e))?
    .map_err(|e| super::super::error::runtime_call("commit moderation fact", e))?;

    let actor = runtime.authority_id();
    let mut fanout = BTreeSet::new();
    for peer in &scope.peers {
        if *peer != actor {
            fanout.insert(*peer);
        }
    }
    for peer in extra_peers {
        if *peer != actor {
            fanout.insert(*peer);
        }
    }

    for peer in fanout {
        send_moderation_fact_with_retry(runtime, peer, scope.context_id, &fact).await?;
    }

    Ok(())
}

pub(super) async fn apply_local_home_projection<F>(
    app_core: &Arc<RwLock<AppCore>>,
    scope: &ModerationScope,
    actor: AuthorityId,
    timestamp_ms: u64,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut crate::views::home::HomeState),
{
    let mut homes = homes_signal_snapshot(app_core).await?;
    if homes.home_state(&scope.home_id).is_none() {
        homes.add_home(crate::views::home::HomeState::new(
            scope.home_id,
            None,
            actor,
            timestamp_ms,
            scope.context_id,
        ));
    }

    let Some(home) = homes.home_mut(&scope.home_id) else {
        return Err(AuraError::not_found(scope.home_id.to_string()));
    };
    update(home);
    replace_homes_projection_observed(app_core, homes).await
}
