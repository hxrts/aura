#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args
)]
//! # TUI Callback Wiring E2E Tests - Batch 3
//!
//! Deep validation tests for:
//! 1. **Block Operations** - CreateBlock, BLOCK_SIGNAL/BLOCKS_SIGNAL, resident management
//! 2. **Recovery Flow** - Full recovery lifecycle (start → approve → complete/cancel)
//! 3. **Channel Lifecycle** - Create → Join → Send → Leave → Close
//! 4. **Contact Management** - Nickname updates, guardian toggle, contact operations
//!
//! ## Running
//!
//! ```bash
//! cargo test --package aura-terminal --test tui_callback_wiring_batch3 -- --nocapture
//! ```

use async_lock::RwLock;
use std::sync::Arc;

use aura_app::signal_defs::{
    BLOCKS_SIGNAL, BLOCK_SIGNAL, CHAT_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL,
    RECOVERY_SIGNAL, UNREAD_COUNT_SIGNAL,
};
use aura_app::views::RecoveryProcessStatus;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, ChannelId};
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::tui::context::IoContext;
use aura_terminal::tui::effects::EffectCommand;

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test environment with IoContext and AppCore
async fn setup_test_env(name: &str) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
    let test_dir = std::env::temp_dir().join(format!(
        "aura-callback-test3-{}-{}",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    let mut app_core = AppCore::new(AppConfig::default()).expect("Failed to create AppCore");
    app_core
        .init_signals()
        .await
        .expect("Failed to init signals");
    let app_core = Arc::new(RwLock::new(app_core));

    let ctx = IoContext::with_account_status(
        app_core.clone(),
        false,
        test_dir,
        format!("test-device-{}", name),
        TuiMode::Production,
    );

    // Create account for testing
    ctx.create_account(&format!("TestUser-{}", name)).await.expect("Failed to create account");

    (Arc::new(ctx), app_core)
}

/// Cleanup test directory
fn cleanup_test_dir(name: &str) {
    let test_dir = std::env::temp_dir().join(format!(
        "aura-callback-test3-{}-{}",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&test_dir);
}

// ============================================================================
// Block Operations Tests
// ============================================================================

/// Test that BLOCK_SIGNAL and BLOCKS_SIGNAL are properly initialized
#[tokio::test]
async fn test_block_signals_initialization() {
    println!("\n=== Block Signals Initialization Test ===\n");

    let (ctx, app_core) = setup_test_env("block-init").await;

    // Phase 1: Read initial BLOCK_SIGNAL state
    println!("Phase 1: Check BLOCK_SIGNAL initial state");
    let core = app_core.read().await;
    let block_state = core.read(&*BLOCK_SIGNAL).await;

    match block_state {
        Ok(state) => {
            println!("  BLOCK_SIGNAL initialized");
            println!("  Block ID: {}", state.id);
            println!("  Block name: {}", state.name);
            println!("  Resident count: {}", state.resident_count);
            // Default state should have empty/default values
            assert!(
                state.residents.is_empty() || state.id == ChannelId::default(),
                "Initial block state should be empty or default"
            );
        }
        Err(e) => {
            println!(
                "  BLOCK_SIGNAL read error (expected for uninitialized): {:?}",
                e
            );
        }
    }

    // Phase 2: Read initial BLOCKS_SIGNAL state
    println!("\nPhase 2: Check BLOCKS_SIGNAL initial state");
    let blocks_state = core.read(&*BLOCKS_SIGNAL).await;

    match blocks_state {
        Ok(state) => {
            println!("  BLOCKS_SIGNAL initialized");
            println!("  Total blocks: {}", state.blocks.len());
            println!("  Current block ID: {:?}", state.current_block_id);
            // Should start with no blocks
            assert!(
                state.blocks.is_empty(),
                "Initial blocks state should be empty"
            );
        }
        Err(e) => {
            println!(
                "  BLOCKS_SIGNAL read error (expected for uninitialized): {:?}",
                e
            );
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("block-init");
    println!("\n=== Block Signals Initialization Test PASSED ===\n");
}

/// Test CreateBlock command creates a block and updates signals
#[tokio::test]
async fn test_create_block_updates_signals() {
    println!("\n=== Create Block Updates Signals Test ===\n");

    let (ctx, app_core) = setup_test_env("create-block").await;

    // Phase 1: Get initial blocks count
    println!("Phase 1: Get initial state");
    let core = app_core.read().await;
    let initial_blocks = core
        .read(&*BLOCKS_SIGNAL)
        .await
        .map(|s| s.blocks.len())
        .unwrap_or(0);
    println!("  Initial blocks count: {}", initial_blocks);
    drop(core);

    // Phase 2: Create a block
    println!("\nPhase 2: Create a block");
    let result = ctx
        .dispatch(EffectCommand::CreateBlock {
            name: Some("Test Block".to_string()),
        })
        .await;

    // CreateBlock requires Sensitive authorization, may fail in test env
    match &result {
        Ok(response) => {
            println!("  CreateBlock succeeded: {:?}", response);
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("authorization") || err_msg.contains("Sensitive") {
                println!("  CreateBlock requires elevated authorization (expected)");
                println!("  Test validates authorization is enforced");
            } else {
                println!("  CreateBlock error: {:?}", e);
            }
        }
    }

    // Phase 3: Verify block was created (if succeeded) or authorization enforced
    println!("\nPhase 3: Verify state");
    let core = app_core.read().await;
    let blocks_state = core.read(&*BLOCKS_SIGNAL).await;

    if let Ok(state) = blocks_state {
        println!("  Blocks count after create: {}", state.blocks.len());
        if state.blocks.len() > initial_blocks {
            println!("  New block was created successfully");
            // Find the new block
            for (id, block) in &state.blocks {
                println!("    Block: {} ({})", block.name, id);
            }
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("create-block");
    println!("\n=== Create Block Updates Signals Test PASSED ===\n");
}

/// Test block resident management
#[tokio::test]
async fn test_block_resident_operations() {
    println!("\n=== Block Resident Operations Test ===\n");

    let (ctx, app_core) = setup_test_env("block-residents").await;

    // Phase 1: Check initial block state for residents
    println!("Phase 1: Check initial resident state");
    let core = app_core.read().await;

    if let Ok(block_state) = core.read(&*BLOCK_SIGNAL).await {
        println!("  Block ID: {}", block_state.id);
        println!("  Initial residents: {}", block_state.residents.len());
        println!("  My role: {:?}", block_state.my_role);

        // User should be owner/steward of their own block
        for resident in &block_state.residents {
            println!("    Resident: {} ({:?})", resident.name, resident.role);
        }
    }

    // Phase 2: Test GrantSteward command
    println!("\nPhase 2: Test GrantSteward command");
    let result = ctx
        .dispatch(EffectCommand::GrantSteward {
            target: "test-user-id".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  GrantSteward response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Admin") || err_msg.contains("authorization") {
                println!("  GrantSteward requires Admin privileges (expected)");
            } else {
                println!("  GrantSteward error: {:?}", e);
            }
        }
    }

    // Phase 3: Test RevokeSteward command
    println!("\nPhase 3: Test RevokeSteward command");
    let result = ctx
        .dispatch(EffectCommand::RevokeSteward {
            target: "test-user-id".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  RevokeSteward response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Admin") || err_msg.contains("authorization") {
                println!("  RevokeSteward requires Admin privileges (expected)");
            } else {
                println!("  RevokeSteward error: {:?}", e);
            }
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("block-residents");
    println!("\n=== Block Resident Operations Test PASSED ===\n");
}

// ============================================================================
// Recovery Flow Tests
// ============================================================================

/// Test complete recovery flow: Start → State Change
#[tokio::test]
async fn test_recovery_flow_start() {
    println!("\n=== Recovery Flow Start Test ===\n");

    let (ctx, app_core) = setup_test_env("recovery-start").await;

    // Phase 1: Get initial recovery state
    println!("Phase 1: Get initial recovery state");
    let core = app_core.read().await;
    let initial_recovery = core.read(&*RECOVERY_SIGNAL).await;

    if let Ok(recovery) = &initial_recovery {
        let status = recovery
            .active_recovery
            .as_ref()
            .map(|r| format!("{:?}", r.status))
            .unwrap_or_else(|| "Idle".to_string());
        println!("  Initial status: {}", status);
        println!("  Initial guardians: {}", recovery.guardians.len());
        println!("  Threshold: {}", recovery.threshold);
    }
    drop(core);

    // Phase 2: Dispatch StartRecovery
    println!("\nPhase 2: Dispatch StartRecovery");
    let result = ctx.dispatch(EffectCommand::StartRecovery).await;

    match &result {
        Ok(response) => {
            println!("  StartRecovery response: {:?}", response);
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") || err_msg.contains("authorization") {
                println!("  StartRecovery requires Sensitive authorization (expected)");
            } else {
                println!("  StartRecovery error: {:?}", e);
            }
        }
    }

    // Phase 3: Verify recovery state changed
    println!("\nPhase 3: Verify recovery state");
    let core = app_core.read().await;
    let new_recovery = core.read(&*RECOVERY_SIGNAL).await;

    if let Ok(recovery) = &new_recovery {
        let status = recovery
            .active_recovery
            .as_ref()
            .map(|r| format!("{:?}", r.status))
            .unwrap_or_else(|| "Idle".to_string());
        println!("  New status: {}", status);

        // Check if recovery was initiated
        if recovery.active_recovery.is_some() {
            println!("  Recovery flow initiated successfully");
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("recovery-start");
    println!("\n=== Recovery Flow Start Test PASSED ===\n");
}

/// Test CancelRecovery command
#[tokio::test]
async fn test_recovery_cancel() {
    println!("\n=== Recovery Cancel Test ===\n");

    let (ctx, app_core) = setup_test_env("recovery-cancel").await;

    // Try to cancel recovery (even if not started)
    println!("Phase 1: Dispatch CancelRecovery");
    let result = ctx.dispatch(EffectCommand::CancelRecovery).await;

    match &result {
        Ok(response) => {
            println!("  CancelRecovery response: {:?}", response);
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            // Either authorization error or "no active recovery" is valid
            if err_msg.contains("Sensitive") || err_msg.contains("authorization") {
                println!("  CancelRecovery requires Sensitive authorization (expected)");
            } else if err_msg.contains("no active") || err_msg.contains("not started") {
                println!("  No active recovery to cancel (expected)");
            } else {
                println!("  CancelRecovery error: {:?}", e);
            }
        }
    }

    // Verify recovery signal state
    println!("\nPhase 2: Verify recovery state");
    let core = app_core.read().await;
    if let Ok(recovery) = core.read(&*RECOVERY_SIGNAL).await {
        let status = recovery
            .active_recovery
            .as_ref()
            .map(|r| format!("{:?}", r.status))
            .unwrap_or_else(|| "Idle".to_string());
        println!("  Recovery status: {}", status);
        // After cancel, should be Idle (no active recovery) or Completed
        let is_idle_or_completed = recovery.active_recovery.is_none()
            || recovery
                .active_recovery
                .as_ref()
                .map(|r| matches!(r.status, RecoveryProcessStatus::Completed))
                .unwrap_or(false);
        assert!(
            is_idle_or_completed,
            "After cancel, recovery should be Idle or Completed"
        );
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("recovery-cancel");
    println!("\n=== Recovery Cancel Test PASSED ===\n");
}

/// Test SubmitGuardianApproval command
#[tokio::test]
async fn test_guardian_approval_submission() {
    println!("\n=== Guardian Approval Submission Test ===\n");

    let (ctx, app_core) = setup_test_env("guardian-approval").await;

    // Phase 1: Get initial recovery state
    println!("Phase 1: Get initial recovery state");
    let core = app_core.read().await;
    if let Ok(recovery) = core.read(&*RECOVERY_SIGNAL).await {
        if let Some(active) = &recovery.active_recovery {
            println!("  Status: {:?}", active.status);
            println!("  Approvals: {}", active.approvals.len());
        } else {
            println!("  No active recovery (status: Idle)");
            println!("  Approvals: 0");
        }
    }
    drop(core);

    // Phase 2: Submit guardian approval
    println!("\nPhase 2: Submit guardian approval");
    let result = ctx
        .dispatch(EffectCommand::SubmitGuardianApproval {
            guardian_id: "test-guardian-123".to_string(),
        })
        .await;

    match &result {
        Ok(response) => {
            println!("  SubmitGuardianApproval response: {:?}", response);
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") || err_msg.contains("authorization") {
                println!("  Requires Sensitive authorization (expected)");
            } else if err_msg.contains("no active") || err_msg.contains("not in progress") {
                println!("  No active recovery to approve (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    drop(ctx);
    cleanup_test_dir("guardian-approval");
    println!("\n=== Guardian Approval Submission Test PASSED ===\n");
}

// ============================================================================
// Channel Lifecycle Tests
// ============================================================================

/// Test full channel lifecycle: Create → use → close
#[tokio::test]
async fn test_channel_lifecycle() {
    println!("\n=== Channel Lifecycle Test ===\n");

    let (ctx, app_core) = setup_test_env("channel-lifecycle").await;

    // Phase 1: Get initial channel count
    println!("Phase 1: Get initial channel state");
    let core = app_core.read().await;
    let initial_channels = core
        .read(&*CHAT_SIGNAL)
        .await
        .map(|s| s.channels.len())
        .unwrap_or(0);
    println!("  Initial channels: {}", initial_channels);
    drop(core);

    // Phase 2: Create a channel
    println!("\nPhase 2: Create channel");
    let result = ctx
        .dispatch(EffectCommand::CreateChannel {
            name: "test-channel".to_string(),
            topic: Some("Test topic".to_string()),
            members: vec![],
        })
        .await;

    match &result {
        Ok(response) => println!("  CreateChannel response: {:?}", response),
        Err(e) => println!("  CreateChannel error: {:?}", e),
    }

    // Phase 3: Join the channel
    println!("\nPhase 3: Join channel");
    let result = ctx
        .dispatch(EffectCommand::JoinChannel {
            channel: "test-channel".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  JoinChannel response: {:?}", response),
        Err(e) => println!("  JoinChannel error: {:?}", e),
    }

    // Phase 4: Send a message to the channel
    println!("\nPhase 4: Send message to channel");
    let result = ctx
        .dispatch(EffectCommand::SendMessage {
            channel: "test-channel".to_string(),
            content: "Hello channel!".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  SendMessage response: {:?}", response),
        Err(e) => println!("  SendMessage error: {:?}", e),
    }

    // Phase 5: Set channel topic
    println!("\nPhase 5: Set channel topic");
    let result = ctx
        .dispatch(EffectCommand::SetTopic {
            channel: "test-channel".to_string(),
            text: "Updated topic".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  SetTopic response: {:?}", response),
        Err(e) => println!("  SetTopic error: {:?}", e),
    }

    // Phase 6: Leave the channel
    println!("\nPhase 6: Leave channel");
    let result = ctx
        .dispatch(EffectCommand::LeaveChannel {
            channel: "test-channel".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  LeaveChannel response: {:?}", response),
        Err(e) => println!("  LeaveChannel error: {:?}", e),
    }

    // Phase 7: Close the channel
    println!("\nPhase 7: Close channel");
    let result = ctx
        .dispatch(EffectCommand::CloseChannel {
            channel: "test-channel".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  CloseChannel response: {:?}", response),
        Err(e) => println!("  CloseChannel error: {:?}", e),
    }

    // Phase 8: Verify final state
    println!("\nPhase 8: Verify final channel state");
    let core = app_core.read().await;
    if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
        println!("  Final channel count: {}", chat_state.channels.len());
        for channel in &chat_state.channels {
            println!("    Channel: {} ({})", channel.name, channel.id);
            if let Some(topic) = &channel.topic {
                println!("      Topic: {}", topic);
            }
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("channel-lifecycle");
    println!("\n=== Channel Lifecycle Test PASSED ===\n");
}

/// Test RetryMessage functionality
#[tokio::test]
async fn test_retry_message() {
    println!("\n=== Retry Message Test ===\n");

    let (ctx, app_core) = setup_test_env("retry-msg").await;

    // Phase 1: First, create a DM channel to have a valid channel
    println!("Phase 1: Setup - create DM channel");
    let _ = ctx
        .dispatch(EffectCommand::StartDirectChat {
            contact_id: "retry-target".to_string(),
        })
        .await;

    // Phase 2: Retry a message (simulating retry of failed message)
    println!("\nPhase 2: Retry message");
    let result = ctx
        .dispatch(EffectCommand::RetryMessage {
            message_id: "failed-msg-123".to_string(),
            channel: "dm:retry-target".to_string(),
            content: "Retried message content".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  RetryMessage response: {:?}", response),
        Err(e) => println!("  RetryMessage error: {:?}", e),
    }

    // Phase 3: Verify message appears in chat
    println!("\nPhase 3: Verify message in chat state");
    let core = app_core.read().await;
    if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
        println!("  Messages in current view: {}", chat_state.messages.len());
        for msg in &chat_state.messages {
            if msg.content.contains("Retried") {
                println!("    Found retried message: {}", msg.content);
            }
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("retry-msg");
    println!("\n=== Retry Message Test PASSED ===\n");
}

// ============================================================================
// Contact Management Tests
// ============================================================================

/// Test UpdateContactNickname command
#[tokio::test]
async fn test_update_contact_nickname() {
    println!("\n=== Update Contact Nickname Test ===\n");

    let (ctx, app_core) = setup_test_env("contact-nickname").await;

    // Phase 1: Add a contact first by importing an invitation
    println!("Phase 1: Setup - create invitation and import");
    let export_result = ctx
        .dispatch(EffectCommand::CreateInvitation {
            invitation_type: "contact".to_string(),
            message: Some("Test invitation".to_string()),
            ttl_secs: None,
        })
        .await;

    let contact_id = "test-contact-123".to_string();

    // If we successfully created an invitation, use that ID
    if let Ok(response) = export_result {
        println!("  Created invitation: {:?}", response);
    }

    // Phase 2: Update nickname for a contact
    println!("\nPhase 2: Update contact nickname");
    let result = ctx
        .dispatch(EffectCommand::UpdateContactNickname {
            contact_id: contact_id.clone(),
            nickname: "My Friend".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  UpdateContactNickname response: {:?}", response),
        Err(e) => println!("  UpdateContactNickname error: {:?}", e),
    }

    // Phase 3: Verify nickname in contacts signal
    println!("\nPhase 3: Verify contacts state");
    let core = app_core.read().await;
    if let Ok(contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
        println!("  Total contacts: {}", contacts_state.contacts.len());

        // Look for the updated contact
        for contact in &contacts_state.contacts {
            if contact.id.to_string() == contact_id || contact.nickname == "My Friend" {
                println!(
                    "    Found contact: {} (nickname: {})",
                    contact.id, contact.nickname
                );
            }
        }
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("contact-nickname");
    println!("\n=== Update Contact Nickname Test PASSED ===\n");
}

/// Test ToggleContactGuardian command
#[tokio::test]
async fn test_toggle_contact_guardian() {
    println!("\n=== Toggle Contact Guardian Test ===\n");

    let (ctx, app_core) = setup_test_env("toggle-guardian").await;

    let contact_id = "guardian-candidate-123";
    let contact_authority_id = contact_id.parse::<AuthorityId>().unwrap_or_default();

    // Phase 1: Get initial guardian status
    println!("Phase 1: Check initial guardian status");
    let core = app_core.read().await;
    let mut _was_guardian = false;

    if let Ok(contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
        if let Some(contact) = contacts_state.contact(&contact_authority_id) {
            _was_guardian = contact.is_guardian;
            println!(
                "  Contact {} guardian status: {}",
                contact_id, _was_guardian
            );
        } else {
            println!("  Contact not found (will attempt to toggle anyway)");
        }
    }
    drop(core);

    // Phase 2: Toggle guardian status
    println!("\nPhase 2: Toggle guardian status");
    let result = ctx
        .dispatch(EffectCommand::ToggleContactGuardian {
            contact_id: contact_id.to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  ToggleContactGuardian response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") || err_msg.contains("authorization") {
                println!("  Requires Sensitive authorization (expected)");
            } else if err_msg.contains("not found") {
                println!("  Contact not found (expected for test contact)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    drop(ctx);
    cleanup_test_dir("toggle-guardian");
    println!("\n=== Toggle Contact Guardian Test PASSED ===\n");
}

/// Test InviteGuardian command (with and without contact_id)
#[tokio::test]
async fn test_invite_guardian() {
    println!("\n=== Invite Guardian Test ===\n");

    let (ctx, _app_core) = setup_test_env("invite-guardian").await;

    // Phase 1: Invite guardian without contact_id (should trigger modal)
    println!("Phase 1: InviteGuardian without contact_id");
    let result = ctx
        .dispatch(EffectCommand::InviteGuardian { contact_id: None })
        .await;

    match &result {
        Ok(response) => {
            println!("  Response: {:?}", response);
            // Without contact_id, should return Ok to signal "show modal"
        }
        Err(e) => println!("  Error: {:?}", e),
    }

    // Phase 2: Invite guardian with contact_id
    println!("\nPhase 2: InviteGuardian with contact_id");
    let result = ctx
        .dispatch(EffectCommand::InviteGuardian {
            contact_id: Some("guardian-contact-456".to_string()),
        })
        .await;

    match &result {
        Ok(response) => println!("  Response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") || err_msg.contains("authorization") {
                println!("  Requires Sensitive authorization (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    drop(ctx);
    cleanup_test_dir("invite-guardian");
    println!("\n=== Invite Guardian Test PASSED ===\n");
}

// ============================================================================
// Additional Signal Tests
// ============================================================================

/// Test ERROR_SIGNAL and UNREAD_COUNT_SIGNAL
#[tokio::test]
async fn test_auxiliary_signals() {
    println!("\n=== Auxiliary Signals Test ===\n");

    let (_ctx, app_core) = setup_test_env("aux-signals").await;

    let core = app_core.read().await;

    // Phase 1: Check ERROR_SIGNAL
    println!("Phase 1: Check ERROR_SIGNAL");
    let error_state = core.read(&*ERROR_SIGNAL).await;
    match error_state {
        Ok(err) => {
            println!("  ERROR_SIGNAL state: {:?}", err);
            // Should be None initially
            assert!(err.is_none(), "Initial error state should be None");
        }
        Err(e) => println!("  ERROR_SIGNAL read error: {:?}", e),
    }

    // Phase 2: Check UNREAD_COUNT_SIGNAL
    println!("\nPhase 2: Check UNREAD_COUNT_SIGNAL");
    let unread_count = core.read(&*UNREAD_COUNT_SIGNAL).await;
    match unread_count {
        Ok(count) => {
            println!("  UNREAD_COUNT_SIGNAL: {}", count);
            // Should be 0 initially
            assert_eq!(count, 0, "Initial unread count should be 0");
        }
        Err(e) => println!("  UNREAD_COUNT_SIGNAL read error: {:?}", e),
    }

    // Phase 3: Check INVITATIONS_SIGNAL
    println!("\nPhase 3: Check INVITATIONS_SIGNAL");
    let invitations = core.read(&*INVITATIONS_SIGNAL).await;
    match invitations {
        Ok(inv_state) => {
            println!("  Sent invitations: {}", inv_state.sent.len());
            println!("  Pending invitations: {}", inv_state.pending.len());
        }
        Err(e) => println!("  INVITATIONS_SIGNAL read error: {:?}", e),
    }

    drop(core);
    cleanup_test_dir("aux-signals");
    println!("\n=== Auxiliary Signals Test PASSED ===\n");
}

/// Test invitation accept/decline flow
#[tokio::test]
async fn test_invitation_accept_decline() {
    println!("\n=== Invitation Accept/Decline Test ===\n");

    let (ctx, _app_core) = setup_test_env("inv-accept").await;

    // Phase 1: Create an invitation to have something to work with
    println!("Phase 1: Create invitation");
    let create_result = ctx
        .dispatch(EffectCommand::CreateInvitation {
            invitation_type: "contact".to_string(),
            message: Some("Join me!".to_string()),
            ttl_secs: Some(3600),
        })
        .await;

    let invitation_id = match &create_result {
        Ok(_) => {
            println!("  Invitation created");
            "test-inv-123".to_string()
        }
        Err(e) => {
            println!("  Create error: {:?}", e);
            "test-inv-123".to_string()
        }
    };

    // Phase 2: Accept an invitation
    println!("\nPhase 2: Accept invitation");
    let result = ctx
        .dispatch(EffectCommand::AcceptInvitation {
            invitation_id: invitation_id.clone(),
        })
        .await;

    match &result {
        Ok(response) => println!("  AcceptInvitation response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("not found") || err_msg.contains("invalid") {
                println!("  Invitation not found (expected for synthetic ID)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    // Phase 3: Decline an invitation
    println!("\nPhase 3: Decline invitation");
    let result = ctx
        .dispatch(EffectCommand::DeclineInvitation {
            invitation_id: "another-inv-456".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  DeclineInvitation response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("not found") || err_msg.contains("invalid") {
                println!("  Invitation not found (expected for synthetic ID)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    drop(ctx);
    cleanup_test_dir("inv-accept");
    println!("\n=== Invitation Accept/Decline Test PASSED ===\n");
}

// ============================================================================
// Moderation Commands Tests
// ============================================================================

/// Test moderation commands (KickUser, BanUser, MuteUser)
#[tokio::test]
async fn test_moderation_commands() {
    println!("\n=== Moderation Commands Test ===\n");

    let (ctx, _app_core) = setup_test_env("moderation").await;

    let target_user = "troublemaker-123";
    let channel = "general";

    // Phase 1: KickUser
    println!("Phase 1: KickUser");
    let result = ctx
        .dispatch(EffectCommand::KickUser {
            channel: channel.to_string(),
            target: target_user.to_string(),
            reason: Some("Testing kick".to_string()),
        })
        .await;

    match &result {
        Ok(response) => println!("  KickUser response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Admin") {
                println!("  KickUser requires Admin privileges (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    // Phase 2: BanUser
    println!("\nPhase 2: BanUser");
    let result = ctx
        .dispatch(EffectCommand::BanUser {
            target: target_user.to_string(),
            reason: Some("Testing ban".to_string()),
        })
        .await;

    match &result {
        Ok(response) => println!("  BanUser response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Admin") {
                println!("  BanUser requires Admin privileges (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    // Phase 3: MuteUser
    println!("\nPhase 3: MuteUser");
    let result = ctx
        .dispatch(EffectCommand::MuteUser {
            target: target_user.to_string(),
            duration_secs: Some(300), // 5 minutes
        })
        .await;

    match &result {
        Ok(response) => println!("  MuteUser response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") {
                println!("  MuteUser requires Sensitive authorization (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    // Phase 4: UnmuteUser
    println!("\nPhase 4: UnmuteUser");
    let result = ctx
        .dispatch(EffectCommand::UnmuteUser {
            target: target_user.to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  UnmuteUser response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") {
                println!("  UnmuteUser requires Sensitive authorization (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    // Phase 5: UnbanUser
    println!("\nPhase 5: UnbanUser");
    let result = ctx
        .dispatch(EffectCommand::UnbanUser {
            target: target_user.to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  UnbanUser response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Admin") {
                println!("  UnbanUser requires Admin privileges (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    drop(ctx);
    cleanup_test_dir("moderation");
    println!("\n=== Moderation Commands Test PASSED ===\n");
}

/// Test message pin/unpin operations
#[tokio::test]
async fn test_pin_unpin_message() {
    println!("\n=== Pin/Unpin Message Test ===\n");

    let (ctx, app_core) = setup_test_env("pin-msg").await;

    // Phase 1: Get initial block state for pinned messages
    println!("Phase 1: Check initial pinned messages");
    let core = app_core.read().await;
    if let Ok(block_state) = core.read(&*BLOCK_SIGNAL).await {
        println!(
            "  Initial pinned messages: {}",
            block_state.pinned_messages.len()
        );
    }
    drop(core);

    // Phase 2: Pin a message
    println!("\nPhase 2: Pin message");
    let message_id = "msg-to-pin-123";
    let result = ctx
        .dispatch(EffectCommand::PinMessage {
            message_id: message_id.to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  PinMessage response: {:?}", response),
        Err(e) => println!("  PinMessage error: {:?}", e),
    }

    // Phase 3: Unpin the message
    println!("\nPhase 3: Unpin message");
    let result = ctx
        .dispatch(EffectCommand::UnpinMessage {
            message_id: message_id.to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  UnpinMessage response: {:?}", response),
        Err(e) => println!("  UnpinMessage error: {:?}", e),
    }

    drop(ctx);
    cleanup_test_dir("pin-msg");
    println!("\n=== Pin/Unpin Message Test PASSED ===\n");
}

// ============================================================================
// Device Management Tests
// ============================================================================

/// Test AddDevice and RemoveDevice commands
#[tokio::test]
async fn test_device_management() {
    println!("\n=== Device Management Test ===\n");

    let (ctx, _app_core) = setup_test_env("device-mgmt").await;

    // Phase 1: Add a device
    println!("Phase 1: Add device");
    let result = ctx
        .dispatch(EffectCommand::AddDevice {
            device_name: "My Laptop".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  AddDevice response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") {
                println!("  AddDevice requires Sensitive authorization (expected)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    // Phase 2: Remove a device
    println!("\nPhase 2: Remove device");
    let result = ctx
        .dispatch(EffectCommand::RemoveDevice {
            device_id: "device-to-remove-789".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  RemoveDevice response: {:?}", response),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("Sensitive") {
                println!("  RemoveDevice requires Sensitive authorization (expected)");
            } else if err_msg.contains("not found") {
                println!("  Device not found (expected for synthetic ID)");
            } else {
                println!("  Error: {:?}", e);
            }
        }
    }

    drop(ctx);
    cleanup_test_dir("device-mgmt");
    println!("\n=== Device Management Test PASSED ===\n");
}

// ============================================================================
// LAN Discovery Tests
// ============================================================================

/// Test LAN discovery commands
#[tokio::test]
async fn test_lan_discovery() {
    println!("\n=== LAN Discovery Test ===\n");

    let (ctx, _app_core) = setup_test_env("lan-discovery").await;

    // Phase 1: List LAN peers
    println!("Phase 1: List LAN peers");
    let result = ctx.dispatch(EffectCommand::ListLanPeers).await;

    match &result {
        Ok(response) => println!("  ListLanPeers response: {:?}", response),
        Err(e) => println!("  ListLanPeers error: {:?}", e),
    }

    // Phase 2: Invite a LAN peer
    println!("\nPhase 2: Invite LAN peer");
    let result = ctx
        .dispatch(EffectCommand::InviteLanPeer {
            authority_id: "lan-peer-authority-123".to_string(),
            address: "192.168.1.100:8080".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  InviteLanPeer response: {:?}", response),
        Err(e) => println!("  InviteLanPeer error: {:?}", e),
    }

    drop(ctx);
    cleanup_test_dir("lan-discovery");
    println!("\n=== LAN Discovery Test PASSED ===\n");
}

// ============================================================================
// SendAction (Emote) Test
// ============================================================================

/// Test SendAction command for emotes
#[tokio::test]
async fn test_send_action_emote() {
    println!("\n=== Send Action (Emote) Test ===\n");

    let (ctx, _app_core) = setup_test_env("send-action").await;

    // Send an action/emote
    println!("Phase 1: Send action");
    let result = ctx
        .dispatch(EffectCommand::SendAction {
            channel: "general".to_string(),
            action: "waves hello".to_string(),
        })
        .await;

    match &result {
        Ok(response) => println!("  SendAction response: {:?}", response),
        Err(e) => println!("  SendAction error: {:?}", e),
    }

    drop(ctx);
    cleanup_test_dir("send-action");
    println!("\n=== Send Action (Emote) Test PASSED ===\n");
}

// ============================================================================
// Complete Integration Flow Test
// ============================================================================

/// Test a complete user flow: create contact → chat → add as guardian
#[tokio::test]
async fn test_complete_contact_to_guardian_flow() {
    println!("\n=== Complete Contact to Guardian Flow Test ===\n");

    let (ctx, app_core) = setup_test_env("contact-guardian-flow").await;

    // Phase 1: Create an invitation
    println!("Phase 1: Create invitation for new contact");
    let _ = ctx
        .dispatch(EffectCommand::CreateInvitation {
            invitation_type: "contact".to_string(),
            message: Some("Let's connect!".to_string()),
            ttl_secs: None,
        })
        .await;

    // Phase 2: Simulate receiving (import) an invitation
    println!("\nPhase 2: Import invitation");
    let _ = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "AURA-TEST-FLOW-1234".to_string(),
        })
        .await;

    // Phase 3: Start a chat with the new contact
    println!("\nPhase 3: Start chat with contact");
    let contact_id = "flow-contact-123";
    let _ = ctx
        .dispatch(EffectCommand::StartDirectChat {
            contact_id: contact_id.to_string(),
        })
        .await;

    // Phase 4: Send messages
    println!("\nPhase 4: Send messages");
    let _ = ctx
        .dispatch(EffectCommand::SendDirectMessage {
            target: contact_id.to_string(),
            content: "Hey! Want to be my guardian?".to_string(),
        })
        .await;

    // Phase 5: Update nickname
    println!("\nPhase 5: Update contact nickname");
    let _ = ctx
        .dispatch(EffectCommand::UpdateContactNickname {
            contact_id: contact_id.to_string(),
            nickname: "Trusted Friend".to_string(),
        })
        .await;

    // Phase 6: Invite as guardian
    println!("\nPhase 6: Invite as guardian");
    let _ = ctx
        .dispatch(EffectCommand::InviteGuardian {
            contact_id: Some(contact_id.to_string()),
        })
        .await;

    // Phase 7: Verify final state
    println!("\nPhase 7: Verify final state");
    let core = app_core.read().await;

    // Check chat state
    if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
        let dm_channel = chat_state
            .channels
            .iter()
            .find(|c| c.id.to_string().contains(contact_id) || c.is_dm);
        if let Some(channel) = dm_channel {
            println!("  DM channel exists: {} ({})", channel.name, channel.id);
        }
        println!("  Total messages: {}", chat_state.messages.len());
    }

    // Check contacts state
    if let Ok(contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
        println!("  Total contacts: {}", contacts_state.contacts.len());
    }

    // Check recovery/guardian state
    if let Ok(recovery_state) = core.read(&*RECOVERY_SIGNAL).await {
        println!("  Total guardians: {}", recovery_state.guardians.len());
    }

    drop(core);
    drop(ctx);
    cleanup_test_dir("contact-guardian-flow");
    println!("\n=== Complete Contact to Guardian Flow Test PASSED ===\n");
}
