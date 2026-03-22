//! Home access configuration workflows.
//!
//! This module owns runtime-backed writes for per-home capability configuration.

use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, execute_with_runtime_retry_budget, require_runtime,
    timeout_runtime_call, workflow_retry_policy,
};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, HomeId};
use aura_core::{AuraError, RetryRunError};
use aura_journal::{fact::RelationalFact, DomainFact};
use aura_social::{AccessLevel, AccessLevelCapabilityConfig, SocialFact};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

const ACCESS_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const ACCESS_FACT_SEND_YIELDS_PER_RETRY: usize = 4;
const ACCESS_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

#[aura_macros::strong_reference(domain = "home_scope")]
#[derive(Debug, Clone)]
struct AccessScope {
    home_id: ChannelId,
    context_id: ContextId,
    home_state: crate::views::home::HomeState,
    peers: Vec<AuthorityId>,
}

fn map_runtime_error(operation: &'static str, error: impl std::fmt::Display) -> AuraError {
    super::error::runtime_call(operation, error).into()
}

async fn send_relational_fact_with_retry(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer: AuthorityId,
    context_id: ContextId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let retry_policy = workflow_retry_policy(
        ACCESS_FACT_SEND_MAX_ATTEMPTS as u32,
        std::time::Duration::from_millis(1),
        std::time::Duration::from_millis(1),
    )?;
    execute_with_runtime_retry_budget(runtime, &retry_policy, |attempt| {
        let runtime = Arc::clone(runtime);
        async move {
            if attempt > 0 {
                converge_runtime(&runtime).await;
                for _ in 0..ACCESS_FACT_SEND_YIELDS_PER_RETRY {
                    cooperative_yield().await;
                }
            }
            timeout_runtime_call(
                &runtime,
                "send_relational_fact_with_retry",
                "send_chat_fact",
                ACCESS_RUNTIME_TIMEOUT,
                || runtime.send_chat_fact(peer, context_id, fact),
            )
            .await?
            .map_err(|error| {
                AuraError::from(super::error::WorkflowError::DeliveryFailed {
                    peer: peer.to_string(),
                    attempts: ACCESS_FACT_SEND_MAX_ATTEMPTS,
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

async fn current_access_scope(app_core: &Arc<RwLock<AppCore>>) -> Result<AccessScope, AuraError> {
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };

    let home_id = crate::workflows::context::current_home_id(app_core).await?;
    let home_state = homes
        .home_state(&home_id)
        .cloned()
        .ok_or_else(|| AuraError::not_found(home_id.to_string()))?;

    let context_id = home_state
        .context_id
        .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
    let peers = home_state.members.iter().map(|member| member.id).collect();

    Ok(AccessScope {
        home_id,
        context_id,
        home_state,
        peers,
    })
}

/// Parse a comma-separated capability list into a normalized set.
pub fn parse_capability_list(raw: &str) -> BTreeSet<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

/// Commit per-home capability configuration using a channel/home scope hint.
pub async fn configure_home_capabilities(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    full_caps: &str,
    partial_caps: &str,
    limited_caps: &str,
) -> Result<(), AuraError> {
    let channel_id = channel_hint.and_then(|hint| hint.parse::<ChannelId>().ok());
    configure_home_capabilities_resolved(
        app_core,
        channel_id,
        parse_capability_list(full_caps),
        parse_capability_list(partial_caps),
        parse_capability_list(limited_caps),
    )
    .await
}

/// Commit per-home capability configuration using a resolved home scope.
pub async fn configure_home_capabilities_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    full_caps: BTreeSet<String>,
    partial_caps: BTreeSet<String>,
    limited_caps: BTreeSet<String>,
) -> Result<(), AuraError> {
    if full_caps.is_empty() || partial_caps.is_empty() || limited_caps.is_empty() {
        return Err(AuraError::invalid(
            "All capability sets must contain at least one capability",
        ));
    }

    if channel_hint.is_some() {
        return Err(AuraError::invalid(
            "explicit home scope hints are no longer supported; select the target home first",
        ));
    }

    let scope = current_access_scope(app_core).await?;
    if !scope.home_state.is_admin() {
        return Err(AuraError::permission_denied(
            "Only moderators can configure home capabilities",
        ));
    }

    let runtime = require_runtime(app_core).await?;
    let now_ms = timeout_runtime_call(
        &runtime,
        "configure_home_capabilities_resolved",
        "current_time_ms",
        ACCESS_RUNTIME_TIMEOUT,
        || runtime.current_time_ms(),
    )
    .await
    .map_err(|e| map_runtime_error("Capability config timestamp", e))?
    .map_err(|e| map_runtime_error("Capability config timestamp", e))?;
    let actor = runtime.authority_id();
    let fact = SocialFact::access_level_capabilities_configured_ms(
        HomeId::from_bytes(*scope.home_id.as_bytes()),
        scope.context_id,
        full_caps.into_iter().collect(),
        partial_caps.into_iter().collect(),
        limited_caps.into_iter().collect(),
        now_ms,
    )
    .to_generic();

    timeout_runtime_call(
        &runtime,
        "configure_home_capabilities_resolved",
        "commit_relational_facts",
        ACCESS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
    )
    .await
    .map_err(|e| map_runtime_error("Commit capability config fact", e))?
    .map_err(|e| map_runtime_error("Commit capability config fact", e))?;

    for peer in scope.peers {
        if peer == actor {
            continue;
        }
        send_relational_fact_with_retry(&runtime, peer, scope.context_id, &fact)
            .await
            .map_err(|e| map_runtime_error("Send capability config fact", e))?;
    }

    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();
    if let Some(home) = homes.home_mut(&scope.home_id) {
        home.set_access_level_capabilities(AccessLevelCapabilityConfig {
            full: fact_full_caps(&fact),
            partial: fact_partial_caps(&fact),
            limited: fact_limited_caps(&fact),
        });
        // OWNERSHIP: observed-display-update
        core.views_mut().set_homes(homes);
    }
    drop(core);

    let _ = crate::workflows::system::refresh_account(app_core).await;
    Ok(())
}

fn fact_full_caps(fact: &RelationalFact) -> BTreeSet<String> {
    let aura_journal::fact::RelationalFact::Generic { envelope, .. } = fact else {
        return BTreeSet::new();
    };
    let Some(SocialFact::AccessLevelCapabilitiesConfigured { full_caps, .. }) =
        SocialFact::from_envelope(envelope)
    else {
        return BTreeSet::new();
    };
    full_caps.into_iter().collect()
}

fn fact_partial_caps(fact: &RelationalFact) -> BTreeSet<String> {
    let aura_journal::fact::RelationalFact::Generic { envelope, .. } = fact else {
        return BTreeSet::new();
    };
    let Some(SocialFact::AccessLevelCapabilitiesConfigured { partial_caps, .. }) =
        SocialFact::from_envelope(envelope)
    else {
        return BTreeSet::new();
    };
    partial_caps.into_iter().collect()
}

fn fact_limited_caps(fact: &RelationalFact) -> BTreeSet<String> {
    let aura_journal::fact::RelationalFact::Generic { envelope, .. } = fact else {
        return BTreeSet::new();
    };
    let Some(SocialFact::AccessLevelCapabilitiesConfigured { limited_caps, .. }) =
        SocialFact::from_envelope(envelope)
    else {
        return BTreeSet::new();
    };
    limited_caps.into_iter().collect()
}

/// Commit a per-authority access override for a home using a channel/home scope hint.
pub async fn set_access_override(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    authority_id: AuthorityId,
    access_level: AccessLevel,
) -> Result<(), AuraError> {
    let channel_id = channel_hint.and_then(|hint| hint.parse::<ChannelId>().ok());
    set_access_override_resolved(app_core, channel_id, authority_id, access_level).await
}

/// Commit a per-authority access override for a resolved home scope.
pub async fn set_access_override_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    authority_id: AuthorityId,
    access_level: AccessLevel,
) -> Result<(), AuraError> {
    if matches!(access_level, AccessLevel::Full) {
        return Err(AuraError::invalid(
            "Access override only supports Limited or Partial",
        ));
    }

    if channel_hint.is_some() {
        return Err(AuraError::invalid(
            "explicit home scope hints are no longer supported; select the target home first",
        ));
    }

    let scope = current_access_scope(app_core).await?;
    if !scope.home_state.is_admin() {
        return Err(AuraError::permission_denied(
            "Only moderators can set access overrides",
        ));
    }

    let runtime = require_runtime(app_core).await?;
    let now_ms = timeout_runtime_call(
        &runtime,
        "set_access_override_resolved",
        "current_time_ms",
        ACCESS_RUNTIME_TIMEOUT,
        || runtime.current_time_ms(),
    )
    .await
    .map_err(|e| map_runtime_error("Access override timestamp", e))?
    .map_err(|e| map_runtime_error("Access override timestamp", e))?;
    let actor = runtime.authority_id();
    let fact = SocialFact::access_override_set_ms(
        authority_id,
        HomeId::from_bytes(*scope.home_id.as_bytes()),
        scope.context_id,
        access_level,
        now_ms,
    )
    .to_generic();

    timeout_runtime_call(
        &runtime,
        "set_access_override_resolved",
        "commit_relational_facts",
        ACCESS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
    )
    .await
    .map_err(|e| map_runtime_error("Commit access override fact", e))?
    .map_err(|e| map_runtime_error("Commit access override fact", e))?;

    for peer in scope.peers.iter().copied() {
        if peer == actor {
            continue;
        }
        send_relational_fact_with_retry(&runtime, peer, scope.context_id, &fact)
            .await
            .map_err(|e| map_runtime_error("Send access override fact", e))?;
    }

    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();
    if let Some(home) = homes.home_mut(&scope.home_id) {
        home.set_access_override(authority_id, access_level);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_homes(homes);
    }
    drop(core);

    let _ = crate::workflows::system::refresh_account(app_core).await;
    Ok(())
}
