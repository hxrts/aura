// Clippy allows for new crate integration - see docs/805_development_patterns.md
#![allow(clippy::clone_on_copy)]
#![allow(clippy::single_match)]
#![allow(clippy::let_and_return)]

//! # Aura Terminal - Layer 7: User Interface
//!
//! This crate provides the terminal interface (CLI + TUI) for the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 7 user interface crate providing:
//! - CLI command implementations for scenario management, authority operations, and recovery
//! - Interactive TUI for real-time interaction with Aura
//! - Integration with `AppCore` for all runtime operations
//! - User-facing commands for account management, authentication, and recovery
//! - Visualization and reporting tools for status and diagnostics
//!
//! ## Architecture
//!
//! Uses dependency inversion: `aura-app` is pure, `aura-agent` implements runtime:
//!
//! ```text
//! ┌─────────────────────────┐
//! │     aura-terminal       │  ← THIS CRATE
//! │                         │
//! │  CLI handlers           │
//! │  TUI screens/components │
//! └───────────┬─────────────┘
//!             │
//!             ↓ imports from both
//! ┌───────────────────────────┐     ┌───────────────────────────┐
//! │        aura-app           │     │       aura-agent          │
//! │    (pure app core)        │     │    (runtime layer)        │
//! │                           │     │                           │
//! │  AppCore, Intent          │     │  AuraAgent, EffectContext │
//! │  ViewState, RuntimeBridge │     │  Services (Auth, Recovery)│
//! └───────────────────────────┘     └───────────────────────────┘
//! ```
//!
//! ## Constraints
//!
//! This crate:
//! - **IMPORTS FROM**: `aura-app` (pure types), `aura-agent` (runtime), `aura-core` (types only)
//! - **MUST NOT**: Create effect implementations or handlers (use aura-effects)
//! - **MUST NOT**: Be imported by Layer 1-6 crates (no circular dependencies)
//!
//! ## What Belongs Here
//!
//! - CLI command definitions and argument parsing
//! - TUI screens, components, and layout
//! - CLI handler for command execution and coordination
//! - Terminal-specific rendering and input handling
//! - Human-friendly command implementations
//! - Visualization and output formatting
//! - Error handling and user-friendly error messages
//!
//! ## What Does NOT Belong Here
//!
//! - Effect implementations (belong in aura-effects)
//! - Protocol logic (belong in Layer 5 feature crates)
//! - Runtime composition (belong in aura-agent)
//! - Platform-agnostic views and intents (belong in aura-app)
//! - Test harnesses and fixtures (belong in aura-testkit)

#![allow(clippy::disallowed_methods)] // CLI handlers intentionally call system APIs for user interactions
#![allow(clippy::disallowed_types)]
// CLI uses blake3::Hasher directly in some places
#![allow(clippy::empty_line_after_doc_comments)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::identity_op)]
#![allow(clippy::or_fun_call)]
#![allow(clippy::unwrap_or_default)]
#![allow(missing_docs)]

pub mod cli;
pub mod error;
pub mod handlers;
pub mod ids;
pub mod local_store;
#[cfg(feature = "terminal")]
pub mod tui;

// Testing utilities for deterministic TUI testing
#[cfg(any(test, feature = "testing"))]
pub mod testing;

// Demo module requires simulator - only available with development feature
#[cfg(feature = "development")]
pub mod demo;

// Re-export CLI handler and command enums
#[cfg(feature = "development")]
pub use cli::DemoCommands;
#[cfg(feature = "terminal")]
pub use cli::TuiArgs;
pub use cli::{AmpAction, AuthorityCommands, ChatCommands, ContextAction, SyncAction};
pub use handlers::CliHandler;

// Action types defined in this module (no re-export needed)

// Action types are defined in this module and automatically available
// Import app types from aura-app (pure layer)
use aura_app::{AppConfig, AppCore};
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::{AgentBuilder, EffectContext};
use aura_core::{effects::ExecutionMode, identifiers::DeviceId, AuraError};
use std::sync::Arc;

// Re-export unified terminal error types
pub use error::{TerminalError, TerminalResult};

/// Create a CLI handler for the given device ID
///
/// Uses AppCore as the unified backend for all operations.
pub fn create_cli_handler(device_id: DeviceId) -> Result<CliHandler, AuraError> {
    let authority_id = ids::authority_id(&format!("cli:authority:{}", device_id));
    let context_id = ids::context_id(&format!("cli:context:{}", device_id));

    // Build agent
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing()
        .map_err(|e| AuraError::agent(format!("Agent build failed: {}", e)))?;
    let agent = Arc::new(agent);

    // Create AppCore with the runtime bridge (dependency inversion)
    let config = AppConfig::default();
    let app_core = AppCore::with_runtime(config, agent.clone().as_runtime_bridge())
        .map_err(|e| AuraError::agent(format!("AppCore creation failed: {}", e)))?;
    let app_core = Arc::new(RwLock::new(app_core));

    let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
    Ok(CliHandler::with_agent(
        app_core,
        agent,
        device_id,
        effect_context,
    ))
}

/// Create a test CLI handler for the given device ID
pub fn create_test_cli_handler(device_id: DeviceId) -> Result<CliHandler, AuraError> {
    let authority_id = ids::authority_id(&format!("cli:test-authority:{}", device_id));
    let context_id = ids::context_id(&format!("cli:test-context:{}", device_id));

    // Build agent
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing()
        .map_err(|e| AuraError::agent(format!("Agent build failed: {}", e)))?;
    let agent = Arc::new(agent);

    // Create AppCore with the runtime bridge (dependency inversion)
    let config = AppConfig::default();
    let app_core = AppCore::with_runtime(config, agent.clone().as_runtime_bridge())
        .map_err(|e| AuraError::agent(format!("AppCore creation failed: {}", e)))?;
    let app_core = Arc::new(RwLock::new(app_core));

    let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
    Ok(CliHandler::with_agent(
        app_core,
        agent,
        device_id,
        effect_context,
    ))
}

/// Create a CLI handler with a generated device ID
pub fn create_default_cli_handler() -> Result<CliHandler, AuraError> {
    let device_id = ids::device_id("cli:default-device");
    create_cli_handler(device_id)
}

/// Create a test CLI handler with a deterministic device ID
#[cfg(test)]
pub fn create_default_test_cli_handler() -> Result<CliHandler, AuraError> {
    use aura_testkit::DeviceTestFixture;
    let fixture = DeviceTestFixture::new(0);
    let device_id = fixture.device_id();
    create_test_cli_handler(device_id)
}

/// Create a test CLI handler with a deterministic device ID (fallback for non-test builds)
#[cfg(not(test))]
pub fn create_default_test_cli_handler() -> Result<CliHandler, AuraError> {
    let device_id = ids::device_id("cli:default-test-device");
    create_test_cli_handler(device_id)
}

/// Scenario action types
#[derive(Debug, Clone)]
pub enum ScenarioAction {
    /// Discover scenarios in a directory tree
    Discover {
        /// Root directory to search
        root: std::path::PathBuf,
        /// Whether to validate discovered scenarios
        validate: bool,
    },
    /// List available scenarios
    List {
        /// Directory containing scenarios
        directory: std::path::PathBuf,
        /// Show detailed information
        detailed: bool,
    },
    /// Validate scenario configurations
    Validate {
        /// Directory containing scenarios
        directory: std::path::PathBuf,
        /// Validation strictness level
        strictness: Option<String>,
    },
    /// Run scenarios
    Run {
        /// Directory containing scenarios
        directory: Option<std::path::PathBuf>,
        /// Pattern to match scenario names
        pattern: Option<String>,
        /// Run scenarios in parallel
        parallel: bool,
        /// Maximum number of parallel scenarios
        max_parallel: Option<usize>,
        /// Output file for results
        output_file: Option<std::path::PathBuf>,
        /// Generate detailed report
        detailed_report: bool,
    },
    /// Generate reports from scenario results
    Report {
        /// Input results file
        input: std::path::PathBuf,
        /// Output report file
        output: std::path::PathBuf,
        /// Report format (text, json, html)
        format: Option<String>,
        /// Include detailed information
        detailed: bool,
    },
}

/// Snapshot maintenance subcommands.
#[derive(Debug, Clone)]
pub enum SnapshotAction {
    /// Run the full Snapshot_v1 ceremony locally (propose + commit + GC).
    Propose,
}

/// Admin maintenance subcommands.
#[derive(Debug, Clone)]
pub enum AdminAction {
    /// Replace the administrator for an account (records journal fact).
    Replace {
        /// Account identifier (UUID string).
        account: String,
        /// Device ID of the new admin (UUID string).
        new_admin: String,
        /// Epoch when the new admin becomes authoritative.
        activation_epoch: u64,
    },
}

/// Recovery subcommands exposed via CLI.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Initiate guardian recovery from the local device.
    Start {
        /// Account identifier to recover.
        account: String,
        /// Comma separated guardian device IDs.
        guardians: String,
        /// Required guardian threshold (defaults to 2).
        threshold: u32,
        /// Recovery priority (normal|urgent|emergency).
        priority: String,
        /// Dispute window in hours (guardians can object before finalize).
        dispute_hours: u64,
        /// Optional human readable justification recorded in the request.
        justification: Option<String>,
    },
    /// Approve a guardian recovery request from this device.
    Approve {
        /// Path to a serialized recovery request (JSON).
        request_file: std::path::PathBuf,
    },
    /// Show local guardian recovery status and cooldown timers.
    Status,
    /// File a dispute against a recovery evidence record.
    Dispute {
        /// Evidence identifier returned by `aura recovery start`.
        evidence: String,
        /// Human readable reason included in the dispute log.
        reason: String,
    },
}

/// Invitation subcommands.
#[derive(Debug, Clone)]
pub enum InvitationAction {
    /// Create a device invitation envelope and broadcast it.
    Create {
        /// Account identifier.
        account: String,
        /// Device ID of the invitee.
        invitee: String,
        /// Role granted to the invitee.
        role: String,
        /// Optional TTL in seconds.
        ttl: Option<u64>,
    },
    /// Accept an invitation by ID.
    Accept {
        /// Invitation identifier string.
        invitation_id: String,
    },
    /// Decline an invitation by ID.
    Decline {
        /// Invitation identifier string.
        invitation_id: String,
    },
    /// Cancel an invitation you previously sent.
    Cancel {
        /// Invitation identifier string.
        invitation_id: String,
    },
    /// List pending invitations.
    List,
    /// Export an invitation as a shareable code for out-of-band transfer.
    Export {
        /// Invitation ID to export.
        invitation_id: String,
    },
    /// Import and display details of a shareable invitation code.
    Import {
        /// The shareable invitation code (format: aura:v1:<base64>).
        code: String,
    },
}

/// OTA upgrade subcommands
#[derive(Debug, Clone)]
pub enum OtaAction {
    /// Submit a new upgrade proposal
    Propose {
        /// Source version (from)
        from_version: String,
        /// Target version (to)
        to_version: String,
        /// Upgrade type: soft, hard, or security
        upgrade_type: String,
        /// Download URL for the upgrade package
        download_url: String,
        /// Upgrade description
        description: String,
    },
    /// Set user opt-in policy
    Policy {
        /// Policy type: auto, manual, security, soft-auto
        policy: String,
    },
    /// Check upgrade status
    Status,
    /// Opt into a specific upgrade
    OptIn {
        /// Proposal ID to opt into
        proposal_id: String,
    },
    /// List all upgrade proposals
    List,
    /// Show upgrade statistics
    Stats,
}

/// CLI error types
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Command was not found or recognized
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    /// Invalid input provided by the user
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Configuration-related error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// File system operation error
    #[error("File system error: {0}")]
    FileSystem(String),

    /// Data serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Feature not yet implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Authentication or authorization error
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Network communication error
    #[error("Network error: {0}")]
    Network(String),

    /// General operation failure
    #[error("Operation failed: {0}")]
    OperationFailed(String),
}
