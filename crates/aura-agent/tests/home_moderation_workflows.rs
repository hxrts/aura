//! Home moderation workflow tests against a real testing runtime.
//!
//! The home creator is designated moderator at creation, so each moderation
//! command succeeds for it; once its access level no longer grants the
//! command's capability the same command is refused with a permission error.

#![allow(missing_docs)]

use crate::support;

use anyhow::Result;
use aura_app::ui::workflows::{access, context, moderation};
use aura_core::types::identifiers::AuthorityId;
use aura_core::AuraError;
use support::{home_view, quiesce, wait_until, SimNet};

fn is_permission_denied(error: &AuraError) -> bool {
    matches!(error, AuraError::PermissionDenied { .. })
}

#[tokio::test(start_paused = true)]
async fn creator_moderation_commands_are_allowed_then_refused_when_limited() -> Result<()> {
    let net = SimNet::new();
    let creator = net.testing_peer(61).await?;
    let (app_core, me) = (&creator.app, creator.id);
    let home_id = context::create_home(app_core, Some("ModHome".to_string()), None).await?;
    let target = AuthorityId::new_from_entropy([62u8; 32]);

    moderation::mute_user_resolved(app_core, target, Some(60), 1_000).await?;
    moderation::unmute_user_resolved(app_core, target).await?;
    moderation::ban_user_resolved(app_core, target, Some("spam"), 2_000).await?;
    moderation::unban_user_resolved(app_core, target).await?;
    moderation::kick_user_resolved(app_core, home_id, target, None, 3_000).await?;

    // Limit the creator's own access: Limited grants no moderation capability.
    access::set_access_override(app_core, None, me, aura_social::AccessLevel::Limited).await?;

    wait_until("ban is refused once access is Limited", || async {
        moderation::ban_user_resolved(app_core, target, None, 4_000)
            .await
            .is_err_and(|error| is_permission_denied(&error))
    })
    .await?;
    for result in [
        moderation::mute_user_resolved(app_core, target, None, 5_000).await,
        moderation::unmute_user_resolved(app_core, target).await,
        moderation::unban_user_resolved(app_core, target).await,
        moderation::kick_user_resolved(app_core, home_id, target, None, 6_000).await,
    ] {
        let error = result.expect_err("moderation must be refused at Limited access");
        assert!(is_permission_denied(&error), "unexpected error: {error}");
    }
    net.finish().await
}

#[tokio::test(start_paused = true)]
async fn created_home_channel_appears_in_creator_chat() -> Result<()> {
    let net = SimNet::new();
    let creator = net.testing_peer(63).await?;
    let app_core = &creator.app;
    let home_id = context::create_home(app_core, Some("ChatHome".to_string()), None).await?;
    let chat = aura_app::ui::workflows::messaging::observed_chat(app_core).await;
    let channel = chat
        .channel(&home_id)
        .expect("the creator's chat must list the created home channel");
    assert_eq!(channel.name, "ChatHome");
    net.finish().await
}

#[tokio::test(start_paused = true)]
async fn creator_ban_stays_in_home_ban_list() -> Result<()> {
    let net = SimNet::new();
    let creator = net.testing_peer(64).await?;
    let app_core = &creator.app;
    let home_id = context::create_home(app_core, Some("BanHome".to_string()), None).await?;
    let target = AuthorityId::new_from_entropy([65u8; 32]);
    moderation::ban_user_resolved(app_core, target, Some("spam"), 2_000).await?;

    for _ in 0..20 {
        quiesce().await;
        let home = home_view(app_core, home_id)
            .await
            .expect("the created home must stay in the homes signal");
        assert!(
            home.ban_list.contains_key(&target),
            "the creator's ban must stay in the home ban list"
        );
    }
    net.finish().await
}
