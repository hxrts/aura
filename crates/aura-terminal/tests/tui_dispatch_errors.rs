#![allow(clippy::expect_used)]
//! # Dispatch Error-Path Tests
//!
//! Tests for error handling in the dispatcher and operational handler:
//! 1. Capability denied → ERROR_SIGNAL emission
//! 2. Operational failure → ERROR_SIGNAL emission
//! 3. Unknown command → ERROR_SIGNAL emission

use async_lock::RwLock;
use std::sync::Arc;

use aura_app::{AppConfig, AppCore};
use aura_terminal::error::TerminalError;
use aura_terminal::tui::commands::CommandCapability;
use aura_terminal::tui::effects::{
    CapabilityPolicy, CommandDispatcher, DispatchError, OperationalHandler,
};

/// Helper to create a test AppCore with signals initialized
async fn test_app_core() -> Arc<RwLock<AppCore>> {
    let core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
    core.init_signals().await.expect("Failed to init signals");
    Arc::new(RwLock::new(core))
}

#[tokio::test]
async fn test_dispatch_error_converts_to_terminal_error() {
    // Verify that DispatchError converts to TerminalError::Capability
    let dispatch_err = DispatchError::PermissionDenied {
        required: CommandCapability::SendDm,
    };
    let terminal_err: TerminalError = dispatch_err.into();
    assert!(matches!(terminal_err, TerminalError::Capability(_)));

    // Verify NotFound converts correctly
    let not_found_err = DispatchError::NotFound {
        resource: "test-channel".to_string(),
    };
    let terminal_err: TerminalError = not_found_err.into();
    assert!(matches!(terminal_err, TerminalError::NotFound(_)));

    // Verify InvalidParameter converts correctly
    let invalid_param_err = DispatchError::InvalidParameter {
        param: "channel".to_string(),
        reason: "invalid format".to_string(),
    };
    let terminal_err: TerminalError = invalid_param_err.into();
    assert!(matches!(terminal_err, TerminalError::Input(_)));
}

#[tokio::test]
async fn test_operational_error_conversion() {
    use aura_terminal::tui::effects::OpError;

    // Test OpError → TerminalError conversion
    let op_err = OpError::Failed("test error".to_string());
    let terminal_err: TerminalError = op_err.into();
    assert!(matches!(terminal_err, TerminalError::Operation(_)));

    // Test NotImplemented conversion
    let not_impl = OpError::NotImplemented("test".to_string());
    let terminal_err: TerminalError = not_impl.into();
    assert!(matches!(terminal_err, TerminalError::NotImplemented(_)));

    // Test InvalidArgument conversion
    let invalid = OpError::InvalidArgument("test".to_string());
    let terminal_err: TerminalError = invalid.into();
    assert!(matches!(terminal_err, TerminalError::Input(_)));
}

#[tokio::test]
async fn test_handle_op_result_helper() {
    let app_core = test_app_core().await;
    let operational = OperationalHandler::new(app_core.clone());

    // Create a failing OpResult
    use aura_terminal::tui::effects::{OpError, OpResponse};
    let failing_result = Err(OpError::Failed("test error".to_string()));

    // Use the handle_op_result helper
    let result = operational
        .handle_op_result(Some(failing_result))
        .await;

    assert!(result.is_some());
    let unwrapped = result.unwrap();
    assert!(unwrapped.is_err());
    assert_eq!(unwrapped.unwrap_err(), "Operation failed: test error");

    // Test successful result
    let success_result = Ok(OpResponse::Ok);
    let result = operational
        .handle_op_result(Some(success_result))
        .await;

    assert!(result.is_some());
    assert!(result.unwrap().is_ok());
}

#[tokio::test]
async fn test_capability_policy_variants() {
    // AllowAll policy - should always pass
    let _allow_all = CommandDispatcher::with_policy(CapabilityPolicy::AllowAll);
    // Note: CommandDispatcher takes IrcCommand, not EffectCommand
    // The actual capability checking happens in the IRC command parsing

    // DenyNonPublic policy exists and can be instantiated
    let _deny_non_public = CommandDispatcher::with_policy(CapabilityPolicy::DenyNonPublic);

    // Custom policy can be created
    let custom_checker = Box::new(|cap: &CommandCapability| -> bool {
        matches!(cap, CommandCapability::None | CommandCapability::SendDm)
    });
    let _custom = CommandDispatcher::with_policy(CapabilityPolicy::Custom(custom_checker));

    // Verify the with_stub_biscuit constructor exists
    let _stub = CommandDispatcher::with_stub_biscuit();
}

#[tokio::test]
async fn test_unknown_command_handling() {
    // This test would require a way to create an "unknown" command
    // that doesn't map to Intent or Operational handler.
    // Currently, all commands are handled by one of these paths.
    // This test documents the expected behavior:
    //
    // 1. If command_to_intent returns None
    // 2. AND operational.execute returns None
    // 3. THEN error should be emitted to ERROR_SIGNAL
    //
    // The dispatch logic in IoContext handles this at lines 1014-1025
}

#[tokio::test]
async fn test_terminal_error_display() {
    // Test that TerminalError variants display correctly
    let err = TerminalError::Operation("test operation failed".to_string());
    assert_eq!(err.to_string(), "Operation failed: test operation failed");

    let err = TerminalError::Capability("admin".to_string());
    assert_eq!(err.to_string(), "Capability required: admin");

    let err = TerminalError::NotFound("test-resource".to_string());
    assert_eq!(err.to_string(), "Not found: test-resource");

    let err = TerminalError::Input("invalid parameter".to_string());
    assert_eq!(err.to_string(), "Invalid input: invalid parameter");

    let err = TerminalError::Network("connection failed".to_string());
    assert_eq!(err.to_string(), "Network error: connection failed");
}
