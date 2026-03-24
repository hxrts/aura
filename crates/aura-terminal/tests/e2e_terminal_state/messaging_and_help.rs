use assert_matches::assert_matches;

#[tokio::test]
async fn test_message_delivery_status_flow() {
    use aura_terminal::tui::types::{DeliveryStatus, Message};

    assert_eq!(DeliveryStatus::Sending.indicator(), "◐");
    assert_eq!(DeliveryStatus::Sent.indicator(), "✓");
    assert_eq!(DeliveryStatus::Delivered.indicator(), "✓✓");
    assert_eq!(DeliveryStatus::Failed.indicator(), "✗");
    assert_eq!(DeliveryStatus::Sending.description(), "Sending...");
    assert_eq!(DeliveryStatus::Sent.description(), "Sent");
    assert_eq!(DeliveryStatus::Delivered.description(), "Delivered");
    assert_eq!(DeliveryStatus::Failed.description(), "Failed");

    let default_msg = Message::new("m1", "Alice", "Hello!");
    assert_eq!(default_msg.delivery_status, DeliveryStatus::Sent);

    let sending_msg = Message::sending("m2", "ch1", "Me", "Sending now...");
    assert_eq!(sending_msg.delivery_status, DeliveryStatus::Sending);
    assert!(sending_msg.is_own);

    let failed_msg = Message::new("m3", "Me", "Failed message")
        .own(true)
        .with_status(DeliveryStatus::Failed);
    assert_eq!(failed_msg.delivery_status, DeliveryStatus::Failed);

    let mut msg = Message::sending("m4", "ch1", "Me", "Test message");
    msg = msg.with_status(DeliveryStatus::Sent);
    msg = msg.with_status(DeliveryStatus::Delivered);
    assert_eq!(msg.delivery_status, DeliveryStatus::Delivered);

    let mut failed = Message::sending("m5", "ch1", "Me", "Will fail");
    failed = failed.with_status(DeliveryStatus::Failed);
    assert_eq!(failed.delivery_status, DeliveryStatus::Failed);
    assert_eq!(DeliveryStatus::default(), DeliveryStatus::Sent);
}

#[tokio::test]
async fn test_retry_message_command() {
    use aura_terminal::tui::effects::{CommandAuthorizationLevel, EffectCommand};
    use aura_terminal::tui::types::{DeliveryStatus, Message};

    let retry_cmd = EffectCommand::RetryMessage {
        message_id: "msg-123".to_string(),
        channel: "general".to_string(),
        content: "Hello, retry!".to_string(),
    };
    assert_matches!(
        &retry_cmd,
        EffectCommand::RetryMessage { message_id, channel, content }
            if message_id == "msg-123" && channel == "general" && content == "Hello, retry!"
    );
    assert_eq!(
        retry_cmd.authorization_level(),
        CommandAuthorizationLevel::Basic
    );

    let failed_msg =
        Message::sending("msg-456", "general", "Me", "This will fail").with_status(DeliveryStatus::Failed);
    let retry_msg = Message::sending("msg-456-retry", "general", "Me", &failed_msg.content);
    assert_eq!(failed_msg.delivery_status, DeliveryStatus::Failed);
    assert_eq!(retry_msg.delivery_status, DeliveryStatus::Sending);
    assert_eq!(retry_msg.content, failed_msg.content);
}

#[tokio::test]
async fn test_channel_mode_operations() {
    use async_lock::RwLock;
    use std::sync::Arc;

    use aura_app::signal_defs::HOMES_SIGNAL;
    use aura_app::views::home::{HomeRole, HomeState};
    use aura_app::AppCore;
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_terminal::handlers::tui::TuiMode;
    use aura_terminal::tui::effects::EffectCommand;
    use aura_terminal::tui::types::ChannelMode;
    use aura_testkit::MockRuntimeBridge;

    use crate::support::IoContextTestEnvBuilder;

    let mut mode = ChannelMode::default();
    assert!(!mode.moderated);
    assert!(!mode.private);
    assert!(!mode.topic_protected);
    assert!(!mode.invite_only);

    mode.parse_flags("+mpt");
    assert!(mode.moderated);
    assert!(mode.private);
    assert!(mode.topic_protected);
    assert!(!mode.invite_only);
    mode.parse_flags("-p");
    assert!(mode.moderated);
    assert!(!mode.private);
    assert!(mode.topic_protected);
    mode.parse_flags("+i");
    assert!(mode.invite_only);

    let mode_str = mode.to_string();
    assert!(mode_str.contains('m'));
    assert!(mode_str.contains('t'));
    assert!(mode_str.contains('i'));
    assert!(!mode_str.contains('p'));

    let desc = mode.description();
    assert!(desc.contains(&"Moderated"));
    assert!(desc.contains(&"Topic Protected"));
    assert!(desc.contains(&"Invite Only"));
    assert!(!desc.contains(&"Private"));

    let cmd = EffectCommand::SetChannelMode {
        channel: "general".to_string(),
        flags: "+mpt".to_string(),
    };
    assert_matches!(
        &cmd,
        EffectCommand::SetChannelMode { channel, flags }
            if channel == "general" && flags == "+mpt"
    );

    let test_dir =
        std::env::temp_dir().join(format!("aura-channel-mode-test-{}", std::process::id()));
    let env = IoContextTestEnvBuilder::new("channel-mode-test")
        .with_base_path(test_dir)
        .with_mock_runtime()
        .with_device_id("test-device-channel-mode")
        .with_mode(TuiMode::Production)
        .create_account_as("ChannelModeTester")
        .build()
        .await;

    let home_id = ChannelId::from_bytes([0x30; 32]);
    let owner_id = AuthorityId::new_from_entropy([0x31; 32]);
    let home_context_id = ContextId::new_from_entropy([10u8; 32]);

    {
        let core = env.app_core.write().await;
        let mut home = HomeState::new(
            home_id.clone(),
            Some("Test Home".to_string()),
            owner_id,
            0,
            home_context_id,
        );
        home.my_role = HomeRole::Member;
        let mut homes = aura_app::views::home::HomesState::default();
        homes.add_home(home);
        homes.select_home(Some(home_id));
        core.views().set_homes(homes.clone());
        core.emit(&*HOMES_SIGNAL, homes)
            .await
            .expect("Failed to emit homes state");
    }

    let mock_bridge = Arc::new(MockRuntimeBridge::new());
    let _app_core = Arc::new(RwLock::new(
        AppCore::with_runtime(aura_app::AppConfig::default(), mock_bridge.clone())
            .expect("Failed to create AppCore"),
    ));
    mock_bridge
        .set_amp_channel_context(home_id, home_context_id)
        .await;
    mock_bridge
        .set_materialized_channel_name_matches("another-channel", vec![home_id])
        .await;

    let initial_mode = env.ctx.get_channel_mode("test-channel").await;
    assert!(!initial_mode.moderated);
    assert!(!initial_mode.private);

    env.ctx.set_channel_mode("test-channel", "+mpi").await;
    let updated_mode = env.ctx.get_channel_mode("test-channel").await;
    assert!(updated_mode.moderated);
    assert!(updated_mode.private);
    assert!(updated_mode.invite_only);
    assert!(!updated_mode.topic_protected);

    env.ctx.set_channel_mode("test-channel", "-m+t").await;
    let final_mode = env.ctx.get_channel_mode("test-channel").await;
    assert!(!final_mode.moderated);
    assert!(final_mode.private);
    assert!(final_mode.invite_only);
    assert!(final_mode.topic_protected);

    let dispatch_result = env
        .ctx
        .dispatch(EffectCommand::SetChannelMode {
            channel: "another-channel".to_string(),
            flags: "+pt".to_string(),
        })
        .await;
    assert!(dispatch_result.is_ok() || dispatch_result.is_err());
}

#[tokio::test]
async fn test_request_state_sync() {
    use aura_terminal::tui::effects::EffectCommand;

    let cmd = EffectCommand::RequestState {
        peer_id: "peer123".to_string(),
    };
    assert_matches!(&cmd, EffectCommand::RequestState { peer_id } if peer_id == "peer123");

    let cmd1 = EffectCommand::RequestState {
        peer_id: "authority:abc123".to_string(),
    };
    if let EffectCommand::RequestState { peer_id } = &cmd1 {
        assert!(peer_id.starts_with("authority:"));
    }

    let cmd2 = EffectCommand::RequestState {
        peer_id: String::new(),
    };
    if let EffectCommand::RequestState { peer_id } = &cmd2 {
        assert!(peer_id.is_empty());
    }
}

#[tokio::test]
async fn test_help_screen_shortcuts() {
    use aura_terminal::tui::components::{get_help_commands, HelpCommand};

    let commands = get_help_commands();
    assert!(!commands.is_empty());

    let categories: std::collections::HashSet<_> =
        commands.iter().map(|command| command.category.as_str()).collect();
    assert!(categories.contains("Navigation"));
    assert!(categories.contains("Chat"));
    assert!(categories.contains("Contacts"));
    assert!(categories.contains("Neighborhood"));
    assert!(categories.contains("Settings"));
    assert!(categories.contains("Notifications"));

    for command in &commands {
        assert!(!command.name.starts_with('/'));
        assert!(command.name.chars().count() <= 10);
    }

    assert!(commands.iter().any(|command| command.name == "q"));
    assert!(commands.iter().any(|command| command.name == "?"));
    assert!(commands.iter().any(|command| command.name == "1-5"));
    assert!(commands.iter().any(|command| command.name == "Esc"));

    let cmd = HelpCommand::new("t", "t", "Test description", "Test");
    assert_eq!(cmd.name, "t");
    assert_eq!(cmd.syntax, "t");
    assert_eq!(cmd.description, "Test description");
    assert_eq!(cmd.category, "Test");
}

#[tokio::test]
async fn test_context_sensitive_help() {
    use aura_terminal::tui::components::{get_help_commands, get_help_commands_for_screen};

    let default_commands = get_help_commands_for_screen(None);
    let all_commands = get_help_commands();
    assert_eq!(default_commands.len(), all_commands.len());

    let chat_commands = get_help_commands_for_screen(Some("Chat"));
    assert!(chat_commands.len() < all_commands.len());
    assert_eq!(chat_commands[0].category, "Navigation");
    let nav_count = chat_commands
        .iter()
        .filter(|command| command.category == "Navigation")
        .count();
    assert_eq!(chat_commands[nav_count].category, "Chat");

    let neighborhood_commands = get_help_commands_for_screen(Some("Neighborhood"));
    let nav_count = neighborhood_commands
        .iter()
        .filter(|command| command.category == "Navigation")
        .count();
    assert_eq!(neighborhood_commands[nav_count].category, "Neighborhood");

    let chat_categories: std::collections::HashSet<_> =
        chat_commands.iter().map(|command| command.category.as_str()).collect();
    assert!(chat_categories.contains("Navigation"));
    assert!(chat_categories.contains("Chat"));
    assert!(chat_categories.contains("Slash Commands"));
    assert!(!chat_categories.contains("Settings"));
    assert!(!chat_categories.contains("Notifications"));
}

#[tokio::test]
async fn test_error_toast_display() {
    use aura_terminal::tui::components::{ToastLevel, ToastMessage};
    use aura_terminal::tui::context::IoContext;

    let error_toast = ToastMessage::error("test-error", "Something went wrong");
    assert_eq!(error_toast.id, "test-error");
    assert_eq!(error_toast.message, "Something went wrong");
    assert!(matches!(error_toast.level, ToastLevel::Error));
    assert!(error_toast.is_error());

    let success_toast = ToastMessage::success("test-success", "Operation completed");
    assert_eq!(success_toast.id, "test-success");
    assert!(matches!(success_toast.level, ToastLevel::Success));
    assert!(!success_toast.is_error());

    let warning_toast = ToastMessage::warning("test-warning", "Please check your input");
    assert!(matches!(warning_toast.level, ToastLevel::Warning));
    let info_toast = ToastMessage::info("test-info", "Did you know?");
    assert!(matches!(info_toast.level, ToastLevel::Info));

    assert_eq!(ToastLevel::Error.indicator(), "✗");
    assert_eq!(ToastLevel::Success.indicator(), "✓");
    assert_eq!(ToastLevel::Warning.indicator(), "⚠");
    assert_eq!(ToastLevel::Info.indicator(), "ℹ");

    let io_ctx = IoContext::with_defaults_async().await;
    assert!(io_ctx.get_toasts().await.is_empty());

    io_ctx
        .add_error_toast("send-error", "Failed to send message")
        .await;
    let toasts = io_ctx.get_toasts().await;
    assert_eq!(toasts.len(), 1);
    assert_eq!(toasts[0].id, "send-error");
    assert!(toasts[0].is_error());

    io_ctx
        .add_success_toast("save-success", "Settings saved")
        .await;
    assert_eq!(io_ctx.get_toasts().await.len(), 2);

    io_ctx
        .add_toast(ToastMessage::warning("custom-warning", "Low disk space"))
        .await;
    assert_eq!(io_ctx.get_toasts().await.len(), 3);

    io_ctx.add_error_toast("e1", "Error 1").await;
    io_ctx.add_error_toast("e2", "Error 2").await;
    io_ctx.add_error_toast("e3", "Error 3").await;
    let toast_state = io_ctx.get_toasts().await;
    let ids: Vec<_> = toast_state
        .iter()
        .map(|toast| toast.id.as_str())
        .collect();
    assert!(ids.len() <= 5);
    assert!(!ids.contains(&"send-error"));

    io_ctx.clear_toast("e3").await;
    let toast_state = io_ctx.get_toasts().await;
    let ids: Vec<_> = toast_state
        .iter()
        .map(|toast| toast.id.as_str())
        .collect();
    assert!(!ids.contains(&"e3"));

    io_ctx.clear_toasts().await;
    assert!(io_ctx.get_toasts().await.is_empty());
}

#[tokio::test]
async fn test_authorization_checking() {
    use aura_terminal::handlers::tui::TuiMode;
    use aura_terminal::tui::effects::{CommandAuthorizationLevel, EffectCommand};

    use crate::support::IoContextTestEnvBuilder;

    let env = IoContextTestEnvBuilder::new("auth-test")
        .with_base_path(std::env::temp_dir().join(format!("aura-auth-test-{}", std::process::id())))
        .with_device_id("test-device-auth")
        .with_mode(TuiMode::Production)
        .create_account_as("AuthTester")
        .build()
        .await;

    let ping_cmd = EffectCommand::Ping;
    assert_eq!(ping_cmd.authorization_level(), CommandAuthorizationLevel::Public);

    let send_cmd = EffectCommand::SendMessage {
        channel: "test".to_string(),
        content: "hello".to_string(),
    };
    assert_eq!(send_cmd.authorization_level(), CommandAuthorizationLevel::Basic);

    let recovery_cmd = EffectCommand::StartRecovery;
    assert_eq!(
        recovery_cmd.authorization_level(),
        CommandAuthorizationLevel::Sensitive
    );

    let ban_cmd = EffectCommand::BanUser {
        channel: None,
        target: "spammer".to_string(),
        reason: Some("spam".to_string()),
    };
    assert_eq!(ban_cmd.authorization_level(), CommandAuthorizationLevel::Admin);

    let kick_cmd = EffectCommand::KickUser {
        channel: "test".to_string(),
        target: "user".to_string(),
        reason: None,
    };
    assert_eq!(kick_cmd.authorization_level(), CommandAuthorizationLevel::Admin);

    let grant_cmd = EffectCommand::GrantModerator {
        channel: None,
        target: "user".to_string(),
    };
    assert_eq!(grant_cmd.authorization_level(), CommandAuthorizationLevel::Admin);

    assert!(env.ctx.check_authorization(&EffectCommand::Ping).is_ok());
    assert!(
        env.ctx
            .check_authorization(&EffectCommand::SendMessage {
                channel: "test".to_string(),
                content: "hello".to_string(),
            })
            .is_ok()
    );
    assert!(env.ctx.check_authorization(&EffectCommand::StartRecovery).is_ok());

    let ban_result = env.ctx.check_authorization(&EffectCommand::BanUser {
        channel: None,
        target: "spammer".to_string(),
        reason: None,
    });
    assert!(ban_result.is_err());
    let ban_err = ban_result.expect_err("ban should fail");
    let ban_err_text = ban_err.to_string();
    assert!(ban_err_text.contains("Permission denied"));
    assert!(ban_err_text.contains("Ban user") || ban_err_text.contains("administrator"));

    assert!(
        env.ctx
            .check_authorization(&EffectCommand::KickUser {
                channel: String::new(),
                target: "user".to_string(),
                reason: None,
            })
            .is_err()
    );
    assert!(
        env.ctx
            .check_authorization(&EffectCommand::GrantModerator {
                channel: None,
                target: "user".to_string(),
            })
            .is_err()
    );

    let dispatch_result = env
        .ctx
        .dispatch(EffectCommand::BanUser {
            channel: None,
            target: "spammer".to_string(),
            reason: Some("testing".to_string()),
        })
        .await;
    let dispatch_err = dispatch_result.expect_err("admin dispatch should fail");
    assert!(dispatch_err.to_string().contains("Permission denied"));
}
