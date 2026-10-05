//! Home moderation workflow tests against a real testing runtime.
//!
//! The home creator is designated moderator at creation, so each moderation
//! command succeeds for it; once its access level no longer grants the
//! command's capability the same command is refused with a permission error.

#![allow(missing_docs)]

use anyhow::Result;
use async_lock::RwLock;
use aura_agent::{AgentBuilder, AgentConfig};
use aura_app::core::{AppConfig, AppCore};
use aura_app::ui::workflows::{access, context, moderation};
use aura_core::context::EffectContext;
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use std::sync::Arc;
use std::time::Duration;

async fn creator_app_core(
    seed: u8,
) -> Result<(tempfile::TempDir, Arc<RwLock<AppCore>>, AuthorityId)> {
    let authority = AuthorityId::new_from_entropy([seed; 32]);
    let ctx = EffectContext::new(
        authority,
        ContextId::new_from_entropy([seed.wrapping_add(1); 32]),
        ExecutionMode::Testing,
    );
    let temp = tempfile::tempdir()?;
    let mut config = AgentConfig::default();
    config.storage.base_path = temp.path().join("aura");
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&ctx)
            .await?,
    );
    let app_core = Arc::new(RwLock::new(AppCore::with_runtime(
        AppConfig::default(),
        agent.as_runtime_bridge(),
    )?));
    AppCore::init_signals_with_hooks(&app_core).await?;
    Ok((temp, app_core, authority))
}

fn is_permission_denied(error: &AuraError) -> bool {
    matches!(error, AuraError::PermissionDenied { .. })
}

#[tokio::test]
async fn creator_moderation_commands_are_allowed_then_refused_when_limited() -> Result<()> {
    let (_temp, app_core, me) = creator_app_core(61).await?;
    let home_id = context::create_home(&app_core, Some("ModHome".to_string()), None).await?;
    let target = AuthorityId::new_from_entropy([62u8; 32]);

    moderation::mute_user_resolved(&app_core, target, Some(60), 1_000).await?;
    moderation::unmute_user_resolved(&app_core, target).await?;
    moderation::ban_user_resolved(&app_core, target, Some("spam"), 2_000).await?;
    moderation::unban_user_resolved(&app_core, target).await?;
    moderation::kick_user_resolved(&app_core, home_id, target, None, 3_000).await?;

    // Limit the creator's own access: Limited grants no moderation capability.
    access::set_access_override(&app_core, None, me, aura_social::AccessLevel::Limited).await?;

    let mut refused = None;
    for _ in 0..50 {
        match moderation::ban_user_resolved(&app_core, target, None, 4_000).await {
            Err(error) if is_permission_denied(&error) => {
                refused = Some(error);
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(
        refused.is_some(),
        "ban must be refused once access is Limited"
    );
    for result in [
        moderation::mute_user_resolved(&app_core, target, None, 5_000).await,
        moderation::unmute_user_resolved(&app_core, target).await,
        moderation::unban_user_resolved(&app_core, target).await,
        moderation::kick_user_resolved(&app_core, home_id, target, None, 6_000).await,
    ] {
        let error = result.expect_err("moderation must be refused at Limited access");
        assert!(is_permission_denied(&error), "unexpected error: {error}");
    }
    Ok(())
}
