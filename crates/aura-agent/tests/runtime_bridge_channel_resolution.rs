#![allow(missing_docs)]

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::AgentBuilder;
use aura_app::core::{AppConfig, AppCore};
use aura_app::ui::workflows::context::ensure_local_home_projection;
use aura_app::ui::workflows::messaging::{create_channel, invite_user_to_channel};
use aura_core::context::EffectContext;
use aura_core::effects::ExecutionMode;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

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

    let mut app = AppCore::with_runtime(AppConfig::default(), agent.as_runtime_bridge())?;
    app.init_signals().await?;
    let app_core = Arc::new(RwLock::new(app));

    let channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42).await?;
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| anyhow!("runtime bridge unavailable"))?
    };

    let resolved = runtime.resolve_amp_channel_context(channel_id).await?;
    assert!(
        resolved.is_some(),
        "runtime should resolve context for created channel {channel_id}"
    );
    let homes = {
        let core = app_core.read().await;
        core.snapshot().homes
    };
    let home = homes
        .home_state(&channel_id)
        .unwrap_or_else(|| panic!("expected created channel {channel_id} to materialize in homes"));
    assert!(
        home.context_id.is_some(),
        "expected created channel {channel_id} home projection to carry context"
    );

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

    let mut app = AppCore::with_runtime(AppConfig::default(), agent.as_runtime_bridge())?;
    app.init_signals().await?;
    let app_core = Arc::new(RwLock::new(app));

    let home_id = ChannelId::from_bytes(hash(b"home-context-resolution"));
    ensure_local_home_projection(&app_core, home_id, "Primary Home".to_string(), authority).await?;

    let channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42).await?;
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| anyhow!("runtime bridge unavailable"))?
    };

    let resolved = runtime.resolve_amp_channel_context(channel_id).await?;
    assert!(
        resolved.is_some(),
        "runtime should resolve context for created channel {channel_id} in active home context"
    );

    Ok(())
}

#[tokio::test]
async fn create_channel_then_invite_user_succeeds_via_runtime_bridge() -> Result<()> {
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

    let mut app = AppCore::with_runtime(AppConfig::default(), agent.as_runtime_bridge())?;
    app.init_signals().await?;
    let app_core = Arc::new(RwLock::new(app));

    let receiver = AuthorityId::new_from_entropy([33u8; 32]);
    let channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42).await?;
    let invitation_id = invite_user_to_channel(
        &app_core,
        &receiver.to_string(),
        &channel_id.to_string(),
        None,
        None,
    )
    .await?;

    assert!(
        invitation_id.as_str().starts_with("inv-"),
        "expected channel invite id, got {invitation_id}"
    );

    Ok(())
}

#[tokio::test]
async fn create_channel_then_invite_user_succeeds_with_active_home_context() -> Result<()> {
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

    let mut app = AppCore::with_runtime(AppConfig::default(), agent.as_runtime_bridge())?;
    app.init_signals().await?;
    let app_core = Arc::new(RwLock::new(app));

    let home_id = ChannelId::from_bytes(hash(b"home-context-invite-resolution"));
    ensure_local_home_projection(&app_core, home_id, "Primary Home".to_string(), authority).await?;

    let receiver = AuthorityId::new_from_entropy([43u8; 32]);
    let channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42).await?;
    let invitation_id = invite_user_to_channel(
        &app_core,
        &receiver.to_string(),
        &channel_id.to_string(),
        None,
        None,
    )
    .await?;

    assert!(
        invitation_id.as_str().starts_with("inv-"),
        "expected channel invite id, got {invitation_id}"
    );

    Ok(())
}
