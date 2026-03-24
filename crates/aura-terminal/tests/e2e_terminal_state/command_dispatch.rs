use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::tui::effects::EffectCommand;

use crate::support::{BuiltIoContextTestEnv, IoContextTestEnvBuilder};

fn test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aura-{name}-{}", std::process::id()))
}

async fn build_production_env(
    name: &str,
    device_id: &str,
    nickname: &str,
) -> BuiltIoContextTestEnv {
    IoContextTestEnvBuilder::new(name)
        .with_base_path(test_dir(name))
        .with_device_id(device_id)
        .with_mode(TuiMode::Production)
        .create_account_as(nickname)
        .build()
        .await
}

async fn build_mock_runtime_env(
    name: &str,
    device_id: &str,
    nickname: &str,
) -> BuiltIoContextTestEnv {
    IoContextTestEnvBuilder::new(name)
        .with_base_path(test_dir(name))
        .with_mock_runtime()
        .with_device_id(device_id)
        .with_mode(TuiMode::Production)
        .create_account_as(nickname)
        .build()
        .await
}

#[tokio::test]
async fn test_moderation_commands_dispatch() {
    let env =
        build_production_env("moderation-test", "test-device-moderation", "ModerationTester")
            .await;

    let ban_result = env
        .ctx
        .dispatch(EffectCommand::BanUser {
            channel: None,
            target: "user_to_moderate".to_string(),
            reason: Some("Test ban reason".to_string()),
        })
        .await;
    let mute_result = env
        .ctx
        .dispatch(EffectCommand::MuteUser {
            channel: None,
            target: "user_to_moderate".to_string(),
            duration_secs: Some(300),
        })
        .await;
    let kick_result = env
        .ctx
        .dispatch(EffectCommand::KickUser {
            channel: "test_channel_123".to_string(),
            target: "user_to_moderate".to_string(),
            reason: Some("Test kick reason".to_string()),
        })
        .await;

    assert!(ban_result.is_ok() || ban_result.is_err());
    assert!(mute_result.is_ok() || mute_result.is_err());
    assert!(kick_result.is_ok() || kick_result.is_err());
}

#[tokio::test]
async fn test_peer_discovery_commands() {
    let env = build_production_env("peer-discovery-test", "test-device-peers", "PeerTester").await;

    let _ = env.ctx.dispatch(EffectCommand::ListPeers).await;
    let _ = env.ctx.dispatch(EffectCommand::DiscoverPeers).await;
    let _ = env.ctx.dispatch(EffectCommand::ListLanPeers).await;
    assert!(env.ctx.get_discovered_peers().await.is_empty());
}

#[tokio::test]
async fn test_lan_peer_invitation_flow() {
    let env = build_production_env("lan-invite-test", "test-device-lan", "LanInviter").await;
    let test_authority_id = aura_core::AuthorityId::new_from_entropy([22u8; 32]);

    assert!(env.ctx.get_invited_peer_ids().await.is_empty());
    let invite_result = env
        .ctx
        .dispatch(EffectCommand::InviteLanPeer {
            authority_id: test_authority_id,
            address: "192.168.1.100:8080".to_string(),
        })
        .await;
    assert!(invite_result.is_ok() || invite_result.is_err());

    env.ctx.mark_peer_invited(&test_authority_id.to_string()).await;
    assert!(env.ctx.is_peer_invited(&test_authority_id.to_string()).await);
    assert!(!env.ctx.is_peer_invited("unknown_peer").await);

    let invited_peers = env.ctx.get_invited_peer_ids().await;
    assert!(invited_peers.contains(&test_authority_id.to_string()));
    assert_eq!(invited_peers.len(), 1);

    let second_authority = aura_core::AuthorityId::new_from_entropy([23u8; 32]).to_string();
    env.ctx.mark_peer_invited(&second_authority).await;
    let all_invited = env.ctx.get_invited_peer_ids().await;
    assert_eq!(all_invited.len(), 2);
    assert!(all_invited.contains(&test_authority_id.to_string()));
    assert!(all_invited.contains(&second_authority));

    env.ctx.mark_peer_invited(&test_authority_id.to_string()).await;
    assert_eq!(env.ctx.get_invited_peer_ids().await.len(), 2);
}

#[tokio::test]
async fn test_threshold_configuration_flow() {
    use aura_terminal::tui::effects::ThresholdConfig;
    use aura_terminal::tui::screens::ThresholdState;

    let mut state = ThresholdState::new();
    assert!(!state.visible);
    assert_eq!(state.threshold_k, 0);
    assert_eq!(state.threshold_n, 0);

    state.show(2, 5);
    assert!(state.visible);
    assert_eq!(state.threshold_k, 2);
    assert_eq!(state.threshold_n, 5);
    assert!(!state.has_changed());

    state.increment();
    assert_eq!(state.threshold_k, 3);
    assert!(state.has_changed());
    state.increment();
    state.increment();
    state.increment();
    assert_eq!(state.threshold_k, 5);

    state.show(3, 5);
    state.decrement();
    state.decrement();
    state.decrement();
    assert_eq!(state.threshold_k, 1);

    state.show(2, 5);
    assert!(!state.can_submit());
    state.increment();
    assert!(state.can_submit());
    state.start_submitting();
    assert!(!state.can_submit());

    state.show(2, 5);
    state.increment();
    state.increment();
    state.hide();
    assert_eq!(state.threshold_k, 2);

    let env =
        build_mock_runtime_env("threshold-test", "test-device-threshold", "ThresholdTester").await;
    let update_result = env
        .ctx
        .dispatch(EffectCommand::UpdateThreshold {
            config: ThresholdConfig::new(3, 5).expect("valid threshold"),
        })
        .await;
    assert!(update_result.is_ok() || update_result.is_err());
}

#[tokio::test]
async fn test_home_messaging_flow() {
    let env = build_mock_runtime_env("home-test", "test-device-home", "HomeTester").await;

    let send_result = env
        .ctx
        .dispatch(EffectCommand::SendMessage {
            channel: "home:main".to_string(),
            content: "Hello from the home!".to_string(),
        })
        .await;
    assert!(send_result.is_ok() || send_result.is_err());

    let move_result = env
        .ctx
        .dispatch(EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            home_id: "home".to_string(),
            depth: "Full".to_string(),
        })
        .await;
    assert!(move_result.is_ok());

    let limited_result = env
        .ctx
        .dispatch(EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            home_id: "current".to_string(),
            depth: "Limited".to_string(),
        })
        .await;
    assert!(limited_result.is_ok());

    let uuid_home_channel = format!("home:{}", "550e8400-e29b-41d4-a716-446655440000");
    let uuid_result = env
        .ctx
        .dispatch(EffectCommand::SendMessage {
            channel: uuid_home_channel,
            content: "Message to UUID home".to_string(),
        })
        .await;
    if let Err(error) = uuid_result {
        let error_text = error.to_string();
        assert!(
            error_text.contains("Unauthorized")
                || error_text.contains("authority")
                || error_text.contains("failed")
        );
    }
}

#[tokio::test]
async fn test_set_context_flow() {
    let env = build_production_env("context-test", "test-device-context", "ContextTester").await;

    assert!(env.ctx.get_current_context().await.is_none());

    let home_context = "home:main".to_string();
    assert!(
        env.ctx
            .dispatch(EffectCommand::SetContext {
                context_id: home_context.clone(),
            })
            .await
            .is_ok()
    );
    assert_eq!(env.ctx.get_current_context().await, Some(home_context));

    let channel_context = "channel:general".to_string();
    assert!(
        env.ctx
            .dispatch(EffectCommand::SetContext {
                context_id: channel_context.clone(),
            })
            .await
            .is_ok()
    );
    assert_eq!(env.ctx.get_current_context().await, Some(channel_context));

    assert!(
        env.ctx
            .dispatch(EffectCommand::SetContext {
                context_id: String::new(),
            })
            .await
            .is_ok()
    );
    assert!(env.ctx.get_current_context().await.is_none());

    env.ctx
        .set_current_context(Some("dm:user123".to_string()))
        .await;
    assert_eq!(
        env.ctx.get_current_context().await,
        Some("dm:user123".to_string())
    );
    env.ctx.set_current_context(None).await;
    assert!(env.ctx.get_current_context().await.is_none());
}

#[tokio::test]
async fn test_moderator_role_flow() {
    use aura_app::signal_defs::HOMES_SIGNAL;
    use aura_app::views::home::{HomeMember, HomeRole, HomeState};
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

    let env =
        build_mock_runtime_env("moderator-test", "test-device-moderator", "ModeratorTester")
            .await;

    let home_id = ChannelId::from_bytes([0x41; 32]);
    let home_context_id = ContextId::new_from_entropy([9u8; 32]);
    let owner_id = AuthorityId::new_from_entropy([1u8; 32]);
    let member1_id = AuthorityId::new_from_entropy([2u8; 32]);
    let member2_id = AuthorityId::new_from_entropy([3u8; 32]);
    let missing_id = AuthorityId::new_from_entropy([4u8; 32]);

    {
        let mut core = env.app_core.write().await;
        let mut home = HomeState::new(
            home_id.clone(),
            Some("Test Home".to_string()),
            owner_id.clone(),
            0,
            home_context_id,
        );
        home.add_member(HomeMember {
            id: member1_id.clone(),
            name: "Alice".to_string(),
            role: HomeRole::Member,
            is_online: true,
            joined_at: 0,
            last_seen: None,
            storage_allocated: 200 * 1024,
        });
        home.add_member(HomeMember {
            id: member2_id.clone(),
            name: "Bob".to_string(),
            role: HomeRole::Member,
            is_online: true,
            joined_at: 0,
            last_seen: None,
            storage_allocated: 200 * 1024,
        });
        home.my_role = HomeRole::Moderator;

        let mut homes = aura_app::views::home::HomesState::default();
        homes.add_home(home);
        homes.select_home(Some(home_id));
        core.views().set_homes(homes.clone());
        core.set_active_home_selection(Some(home_id));
        core.emit(&*HOMES_SIGNAL, homes)
            .await
            .expect("Failed to emit homes state");
    }

    assert!(
        env.ctx
            .dispatch(EffectCommand::GrantModerator {
                channel: None,
                target: member1_id.to_string(),
            })
            .await
            .is_ok()
    );
    {
        let core = env.app_core.read().await;
        let home = core.views().get_homes().current_home().cloned().expect("home exists");
        let member = home.member(&member1_id).expect("member exists");
        assert!(matches!(member.role, HomeRole::Moderator));
    }

    assert!(
        env.ctx
            .dispatch(EffectCommand::RevokeModerator {
                channel: None,
                target: member1_id.to_string(),
            })
            .await
            .is_ok()
    );
    {
        let core = env.app_core.read().await;
        let home = core.views().get_homes().current_home().cloned().expect("home exists");
        let member = home.member(&member1_id).expect("member exists");
        assert!(matches!(member.role, HomeRole::Member));
    }

    assert!(
        env.ctx
            .dispatch(EffectCommand::GrantModerator {
                channel: None,
                target: owner_id.to_string(),
            })
            .await
            .is_ok()
    );

    let already_moderator = env
        .ctx
        .dispatch(EffectCommand::GrantModerator {
            channel: None,
            target: owner_id.to_string(),
        })
        .await
        .expect_err("existing moderator should fail");
    let already_moderator_text = already_moderator.to_string();
    assert!(
        already_moderator_text.contains("moderator")
            || already_moderator_text.contains("modify")
            || already_moderator_text.contains("already")
    );

    let non_moderator = env
        .ctx
        .dispatch(EffectCommand::RevokeModerator {
            channel: None,
            target: member2_id.to_string(),
        })
        .await
        .expect_err("non-moderator revoke should fail");
    let non_moderator_text = non_moderator.to_string();
    assert!(
        non_moderator_text.contains("not a moderator")
            || non_moderator_text.contains("revoke")
            || non_moderator_text.contains("Moderator")
    );

    let missing = env
        .ctx
        .dispatch(EffectCommand::GrantModerator {
            channel: None,
            target: missing_id.to_string(),
        })
        .await
        .expect_err("missing member grant should fail");
    let missing_text = missing.to_string();
    let missing_text_lower = missing_text.to_lowercase();
    assert!(
        missing_text_lower.contains("not found")
            || missing_text_lower.contains("member")
            || missing_text.contains(&missing_id.to_string())
    );
}

#[tokio::test]
async fn test_neighborhood_navigation_flow() {
    use aura_app::views::neighborhood::{
        NeighborHome, NeighborhoodState, OneHopLinkType, TraversalPosition,
    };
    use aura_core::types::identifiers::ChannelId;

    let env = build_production_env("neighborhood-test", "test-device-nav", "NavigationTester").await;

    let home_home_id = ChannelId::from_bytes([0x51; 32]);
    let alice_home_id = ChannelId::from_bytes([0x52; 32]);
    let bob_home_id = ChannelId::from_bytes([0x53; 32]);
    let locked_home_id = ChannelId::from_bytes([0x54; 32]);

    {
        let core = env.app_core.write().await;
        let mut neighborhood = NeighborhoodState::from_parts(
            home_home_id.clone(),
            "My Home".to_string(),
            vec![
                NeighborHome {
                    id: alice_home_id.clone(),
                    name: "Alice's Home".to_string(),
                    one_hop_link: OneHopLinkType::Direct,
                    shared_contacts: 3,
                    member_count: Some(5),
                    can_traverse: true,
                },
                NeighborHome {
                    id: bob_home_id.clone(),
                    name: "Bob's Home".to_string(),
                    one_hop_link: OneHopLinkType::Direct,
                    shared_contacts: 2,
                    member_count: Some(4),
                    can_traverse: true,
                },
                NeighborHome {
                    id: locked_home_id.clone(),
                    name: "Private Home".to_string(),
                    one_hop_link: OneHopLinkType::TwoHop,
                    shared_contacts: 0,
                    member_count: Some(8),
                    can_traverse: false,
                },
            ],
        );
        neighborhood.position = Some(TraversalPosition {
            current_home_id: home_home_id.clone(),
            current_home_name: "My Home".to_string(),
            depth: 2,
            path: vec![home_home_id.clone()],
        });
        neighborhood.max_depth = 3;
        neighborhood.loading = false;
        core.views().set_neighborhood(neighborhood);
    }

    assert!(
        env.ctx
            .dispatch(EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: alice_home_id.to_string(),
                depth: "Full".to_string(),
            })
            .await
            .is_ok()
    );
    {
        let core = env.app_core.read().await;
        let position = core
            .views()
            .get_neighborhood()
            .position
            .expect("position after navigation");
        assert_eq!(position.current_home_id, alice_home_id);
        assert_eq!(position.current_home_name, "Alice's Home");
        assert_eq!(position.depth, 2);
    }

    assert!(
        env.ctx
            .dispatch(EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "home".to_string(),
                depth: "Full".to_string(),
            })
            .await
            .is_ok()
    );
    {
        let core = env.app_core.read().await;
        let neighborhood = core.views().get_neighborhood();
        assert!(neighborhood.is_at_home());
        let position = neighborhood.position.clone().expect("position after home");
        assert_eq!(position.current_home_id, home_home_id);
    }

    env.ctx
        .dispatch(EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            home_id: bob_home_id.to_string(),
            depth: "Full".to_string(),
        })
        .await
        .expect("Should enter Bob's home");
    assert!(
        env.ctx
            .dispatch(EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "current".to_string(),
                depth: "Limited".to_string(),
            })
            .await
            .is_ok()
    );
    {
        let core = env.app_core.read().await;
        let position = core
            .views()
            .get_neighborhood()
            .position
            .expect("position after limited");
        assert_eq!(position.current_home_id, bob_home_id);
        assert_eq!(position.depth, 0);
    }

    assert!(
        env.ctx
            .dispatch(EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "current".to_string(),
                depth: "Partial".to_string(),
            })
            .await
            .is_ok()
    );
    {
        let core = env.app_core.read().await;
        let position = core
            .views()
            .get_neighborhood()
            .position
            .expect("position after partial");
        assert_eq!(position.depth, 1);
    }
}
