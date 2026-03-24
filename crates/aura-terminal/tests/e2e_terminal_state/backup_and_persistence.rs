use async_lock::RwLock;
use std::sync::Arc;

use aura_app::AppCore;
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::tui::effects::EffectCommand;

use crate::support::IoContextTestEnvBuilder;

#[tokio::test]
async fn test_account_backup_restore_flow() {
    use aura_terminal::handlers::tui::{export_account_backup, import_account_backup};

    let test_dir_a =
        std::env::temp_dir().join(format!("aura-backup-test-a-{}", std::process::id()));
    let test_dir_b =
        std::env::temp_dir().join(format!("aura-backup-test-b-{}", std::process::id()));
    let test_dir_c =
        std::env::temp_dir().join(format!("aura-backup-test-c-{}", std::process::id()));

    let env_a = IoContextTestEnvBuilder::new("backup-a")
        .with_base_path(test_dir_a.clone())
        .with_device_id("test-device-backup-a")
        .with_mode(TuiMode::Production)
        .create_account_as("BackupTester")
        .build()
        .await;
    assert!(env_a.ctx.has_account());

    let backup_code = env_a
        .ctx
        .export_account_backup()
        .await
        .expect("Failed to export backup");
    assert!(backup_code.starts_with("aura:backup:v1:"));
    use base64::Engine;
    let encoded_part = &backup_code["aura:backup:v1:".len()..];
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded_part)
        .expect("Backup code should be valid base64");
    assert!(!decoded.is_empty());

    let (restored_authority, restored_context) =
        import_account_backup(&test_dir_b, &backup_code, false)
            .await
            .expect("Failed to import backup");
    assert!(test_dir_b.join("account.json.dat").exists());
    assert!(!restored_authority.to_string().is_empty());
    assert!(!restored_context.to_string().is_empty());

    let env_b = IoContextTestEnvBuilder::new("backup-b")
        .with_base_path(test_dir_b.clone())
        .with_existing_account(true)
        .with_device_id("test-device-backup-b")
        .with_mode(TuiMode::Production)
        .build()
        .await;
    assert!(env_b.ctx.has_account());

    assert!(env_a.ctx.dispatch(EffectCommand::ExportAccountBackup).await.is_ok());
    assert!(
        env_b
            .ctx
            .dispatch(EffectCommand::ImportAccountBackup {
                backup_code: backup_code.clone(),
            })
            .await
            .is_ok()
    );

    assert!(export_account_backup(&test_dir_c, None).await.is_err());
    assert!(import_account_backup(&test_dir_c, "invalid-code", false).await.is_err());
    let no_overwrite_result = import_account_backup(&test_dir_b, &backup_code, false).await;
    assert!(no_overwrite_result.is_err());
    assert!(no_overwrite_result
        .expect_err("existing account should fail without overwrite")
        .to_string()
        .contains("already exists"));
}

#[tokio::test]
async fn test_device_management() {
    use aura_core::types::identifiers::AuthorityId;
    let test_dir =
        std::env::temp_dir().join(format!("aura-device-mgmt-test-{}", std::process::id()));
    let env = IoContextTestEnvBuilder::new("device-management")
        .with_base_path(test_dir)
        .with_mock_runtime()
        .with_device_id("test-device-mgmt-123")
        .with_mode(TuiMode::Production)
        .create_account_as("DeviceTestUser")
        .build()
        .await;
    env.app_core
        .write()
        .await
        .set_authority(AuthorityId::new_from_entropy([42u8; 32]));

    let devices = env.ctx.snapshot_devices();
    assert!(!devices.devices.is_empty());
    assert_eq!(
        devices.current_device_id,
        Some("test-device-mgmt-123".to_string())
    );
    assert!(devices.devices.iter().any(|device| device.is_current));

    let add_result = env
        .ctx
        .dispatch(EffectCommand::AddDevice {
            nickname_suggestion: "TestPhone".to_string(),
            invitee_authority_id: aura_core::AuthorityId::new_from_entropy([19u8; 32]),
        })
        .await;
    assert!(add_result.is_ok());

    let remove_result = env
        .ctx
        .dispatch(EffectCommand::RemoveDevice {
            device_id: "test-device-to-remove".to_string(),
        })
        .await;
    assert!(remove_result.is_ok());
}

#[tokio::test]
async fn test_snapshot_data_accuracy() {
    use aura_app::signal_defs::HOMES_SIGNAL;
    use aura_app::views::contacts::{Contact, ContactsState, ReadReceiptPolicy};
    use aura_app::views::home::HomeState;
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

    let test_dir = std::env::temp_dir().join(format!(
        "aura-snapshot-accuracy-test-{}",
        std::process::id()
    ));

    let mut app_core =
        AppCore::new(aura_app::AppConfig::default()).expect("Failed to create AppCore");
    app_core
        .init_signals()
        .await
        .expect("Failed to init signals");
    let app_core = Arc::new(RwLock::new(app_core));
    let initialized_app_core = aura_terminal::tui::context::InitializedAppCore::new(app_core.clone())
        .await
        .expect("Failed to init signals");
    let authority_id = AuthorityId::new_from_entropy([42u8; 32]);
    app_core.write().await.set_authority(authority_id);

    let ctx = aura_terminal::tui::context::IoContext::builder()
        .with_app_core(initialized_app_core)
        .with_existing_account(true)
        .with_base_path(test_dir)
        .with_device_id("test-device-snapshot".to_string())
        .with_mode(TuiMode::Production)
        .build()
        .expect("IoContext builder should have all required fields");

    let test_created_at = 1702000000000u64;
    let home_id = "test-home-1".parse::<ChannelId>().unwrap_or_default();
    let home_context_id = ContextId::new_from_entropy([9u8; 32]);
    let home_state = HomeState::new(
        home_id,
        Some("Test Home".to_string()),
        authority_id,
        test_created_at,
        home_context_id,
    );

    {
        let core = app_core.read().await;
        let mut homes_state = aura_app::views::home::HomesState::default();
        homes_state.add_home(home_state.clone());
        core.emit(&*HOMES_SIGNAL, homes_state)
            .await
            .expect("Failed to emit home state");
    }

    let home_snapshot = ctx.snapshot_home();
    if let Some(home_info) = &home_snapshot.home_state {
        assert_eq!(home_info.created_at, test_created_at);
    }

    let members = home_snapshot.members();
    if !members.is_empty() {
        assert!(members.iter().any(|member| member.id == authority_id));
    }

    let contact1_id = AuthorityId::new_from_entropy([11u8; 32]);
    let contact2_id = AuthorityId::new_from_entropy([12u8; 32]);
    let contact3_id = AuthorityId::new_from_entropy([13u8; 32]);
    let contacts_state = ContactsState::from_contacts(vec![
        Contact {
            id: contact1_id,
            nickname: "Alice".to_string(),
            nickname_suggestion: Some("Alice Smith".to_string()),
            is_guardian: false,
            is_member: false,
            last_interaction: Some(1702000000000),
            is_online: true,
            read_receipt_policy: ReadReceiptPolicy::default(),
        },
        Contact {
            id: contact2_id,
            nickname: "Bob".to_string(),
            nickname_suggestion: Some("Bob".to_string()),
            is_guardian: false,
            is_member: false,
            last_interaction: Some(1702000000000),
            is_online: false,
            read_receipt_policy: ReadReceiptPolicy::default(),
        },
        Contact {
            id: contact3_id,
            nickname: "Carol".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: false,
            read_receipt_policy: ReadReceiptPolicy::default(),
        },
    ]);
    {
        let core = app_core.read().await;
        core.views().set_contacts(contacts_state);
    }

    let contacts_snapshot = ctx.snapshot_contacts();
    for contact in &contacts_snapshot.contacts {
        let has_pending_suggestion = contact
            .nickname_suggestion
            .as_ref()
            .is_some_and(|suggested| !suggested.is_empty() && *suggested != contact.nickname);

        let expected = if contact.id == AuthorityId::new_from_entropy([11u8; 32]) {
            true
        } else {
            false
        };
        assert_eq!(has_pending_suggestion, expected);
    }
}

#[tokio::test]
async fn test_journal_compaction_primitives() {
    use aura_core::tree::{AttestedOp, NodeIndex, TreeHash32, TreeOp, TreeOpKind};
    use aura_core::Epoch;
    use aura_journal::algebra::OpLog;

    let mut op_log = OpLog::default();
    for epoch in 0..10u64 {
        let tree_op = TreeOp {
            parent_epoch: Epoch::new(epoch),
            parent_commitment: TreeHash32::default(),
            op: TreeOpKind::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            version: 1,
        };
        op_log.add_operation(AttestedOp {
            op: tree_op,
            agg_sig: vec![0u8; 64],
            signer_count: 2,
        });
    }

    assert_eq!(op_log.len(), 10);
    let epoch = Epoch::new(5);
    let removed = op_log.compact_before_epoch(epoch);
    assert_eq!(removed, 5);
    assert_eq!(op_log.len(), 5);
    for (_cid, op) in op_log.iter() {
        assert!(op.op.parent_epoch >= epoch);
    }
    assert_eq!(op_log.compact_before_epoch(epoch), 0);
    assert_eq!(op_log.compact_before_epoch(Epoch::new(10)), 5);
    assert!(op_log.is_empty());
}
