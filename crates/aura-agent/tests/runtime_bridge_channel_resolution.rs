//! Runtime bridge channel resolution tests.
//!
//! Verifies that channels created through the runtime bridge retain
//! authoritative context bindings that later workflows can reuse.

#![allow(missing_docs)]

use anyhow::Result;
use async_lock::RwLock;
use aura_agent::AgentBuilder;
use aura_app::core::{AppConfig, AppCore};
use aura_app::ui::workflows::messaging::invite_user_to_channel_with_context;
use aura_core::context::EffectContext;
use aura_core::effects::amp::{ChannelCreateParams, ChannelJoinParams};
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use std::sync::Arc;

async fn register_runtime_context(
    agent: &Arc<aura_agent::AuraAgent>,
    authority: AuthorityId,
    timestamp_ms: u64,
) -> Result<ContextId> {
    Ok(agent
        .runtime()
        .contexts()
        .create_context(authority, timestamp_ms)
        .await?)
}

#[tokio::test]
async fn create_channel_produces_runtime_resolvable_channel_context() -> Result<()> {
    let authority = AuthorityId::new_from_entropy([11u8; 32]);
    let ctx = EffectContext::new(
        authority,
        ContextId::new_from_entropy([12u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&ctx)
            .await?,
    );
    let runtime = agent.clone().as_runtime_bridge();
    let context_id = register_runtime_context(&agent, authority, 42).await?;

    let channel_id = runtime
        .amp_create_channel(ChannelCreateParams {
            context: context_id,
            channel: None,
            skip_window: None,
            topic: None,
        })
        .await?;
    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: authority,
        })
        .await?;

    let resolved = runtime.resolve_amp_channel_context(channel_id).await?;
    assert_eq!(resolved, Some(context_id));

    Ok(())
}

#[tokio::test]
async fn create_channel_in_active_home_context_produces_runtime_resolvable_channel_context(
) -> Result<()> {
    let authority = AuthorityId::new_from_entropy([21u8; 32]);
    let ctx = EffectContext::new(
        authority,
        ContextId::new_from_entropy([22u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&ctx)
            .await?,
    );
    let runtime = agent.clone().as_runtime_bridge();
    let context_id = register_runtime_context(&agent, authority, 42).await?;

    let channel_id = runtime
        .amp_create_channel(ChannelCreateParams {
            context: context_id,
            channel: None,
            skip_window: None,
            topic: None,
        })
        .await?;
    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: authority,
        })
        .await?;

    let resolved = runtime.resolve_amp_channel_context(channel_id).await?;
    assert_eq!(resolved, Some(context_id));

    Ok(())
}

#[tokio::test]
async fn create_channel_then_invite_user_requires_canonical_channel_metadata() -> Result<()> {
    let authority = AuthorityId::new_from_entropy([31u8; 32]);
    let ctx = EffectContext::new(
        authority,
        ContextId::new_from_entropy([32u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&ctx)
            .await?,
    );
    let runtime = agent.clone().as_runtime_bridge();
    let mut app = AppCore::with_runtime(AppConfig::default(), runtime.clone())?;
    app.init_signals().await?;
    let app_core = Arc::new(RwLock::new(app));
    let context_id = register_runtime_context(&agent, authority, 42).await?;

    let receiver = AuthorityId::new_from_entropy([33u8; 32]);
    let channel_id = runtime
        .amp_create_channel(ChannelCreateParams {
            context: context_id,
            channel: None,
            skip_window: None,
            topic: None,
        })
        .await?;
    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: authority,
        })
        .await?;
    let error = invite_user_to_channel_with_context(
        &app_core,
        &receiver.to_string(),
        &channel_id.to_string(),
        Some(context_id),
        None,
        None,
        None,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("canonical channel metadata"));
    let resolved = runtime.resolve_amp_channel_context(channel_id).await?;
    assert_eq!(resolved, Some(context_id));

    Ok(())
}

#[tokio::test]
async fn create_channel_then_invite_user_with_active_home_context_requires_canonical_channel_metadata(
) -> Result<()> {
    let authority = AuthorityId::new_from_entropy([41u8; 32]);
    let ctx = EffectContext::new(
        authority,
        ContextId::new_from_entropy([42u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&ctx)
            .await?,
    );
    let runtime = agent.clone().as_runtime_bridge();
    let mut app = AppCore::with_runtime(AppConfig::default(), runtime.clone())?;
    app.init_signals().await?;
    let app_core = Arc::new(RwLock::new(app));
    let context_id = register_runtime_context(&agent, authority, 42).await?;

    let receiver = AuthorityId::new_from_entropy([43u8; 32]);
    let channel_id = runtime
        .amp_create_channel(ChannelCreateParams {
            context: context_id,
            channel: None,
            skip_window: None,
            topic: None,
        })
        .await?;
    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: authority,
        })
        .await?;
    let error = invite_user_to_channel_with_context(
        &app_core,
        &receiver.to_string(),
        &channel_id.to_string(),
        Some(context_id),
        None,
        None,
        None,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("canonical channel metadata"));
    let resolved = runtime.resolve_amp_channel_context(channel_id).await?;
    assert_eq!(resolved, Some(context_id));

    Ok(())
}
