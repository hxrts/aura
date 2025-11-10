//! Aura CLI Library
//!
//! This library provides the command-line interface for the Aura threshold identity platform.
//! It uses the unified effect system architecture for all operations following the guidance
//! from docs/002_system_architecture.md.
//!
//! The aura-cli crate adheres to the unified effect system by:
//! - Using AuraEffectSystem for all system interactions
//! - Implementing CLI-specific effects as composition of core effects
//! - Following proper dependency injection patterns
//! - Avoiding direct system access except in effect handlers

#![allow(missing_docs)]

pub mod effects;
pub mod handlers;

// Re-export key types and traits
pub use effects::{CliConfig, CliEffects, ConfigEffects, OutputEffects};
pub use handlers::CliHandler;

use aura_core::identifiers::DeviceId;
use aura_protocol::AuraEffectSystem;

/// Create a CLI handler for the given device ID
pub fn create_cli_handler(device_id: DeviceId) -> CliHandler {
    let effect_system = AuraEffectSystem::for_production(device_id);
    CliHandler::new(effect_system)
}

/// Create a test CLI handler for the given device ID
pub fn create_test_cli_handler(device_id: DeviceId) -> CliHandler {
    let effect_system = AuraEffectSystem::for_testing(device_id);
    CliHandler::new(effect_system)
}

/// Create a CLI handler with a generated device ID
pub fn create_default_cli_handler() -> CliHandler {
    let device_id = DeviceId::new();
    create_cli_handler(device_id)
}

/// Create a test CLI handler with a generated device ID
pub fn create_default_test_cli_handler() -> CliHandler {
    let device_id = DeviceId::new();
    create_test_cli_handler(device_id)
}

/// Scenario action types
#[derive(Debug, Clone, clap::Subcommand)]
pub enum ScenarioAction {
    /// Discover scenarios in a directory tree
    Discover {
        /// Root directory to search
        #[arg(long)]
        root: std::path::PathBuf,
        /// Whether to validate discovered scenarios
        #[arg(long)]
        validate: bool,
    },
    /// List available scenarios
    List {
        /// Directory containing scenarios
        #[arg(long)]
        directory: std::path::PathBuf,
        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },
    /// Validate scenario configurations
    Validate {
        /// Directory containing scenarios
        #[arg(long)]
        directory: std::path::PathBuf,
        /// Validation strictness level
        #[arg(long)]
        strictness: Option<String>,
    },
    /// Run scenarios
    Run {
        /// Directory containing scenarios
        #[arg(long)]
        directory: Option<std::path::PathBuf>,
        /// Pattern to match scenario names
        #[arg(long)]
        pattern: Option<String>,
        /// Run scenarios in parallel
        #[arg(long)]
        parallel: bool,
        /// Maximum number of parallel scenarios
        #[arg(long)]
        max_parallel: Option<usize>,
        /// Output file for results
        #[arg(long)]
        output_file: Option<std::path::PathBuf>,
        /// Generate detailed report
        #[arg(long)]
        detailed_report: bool,
    },
    /// Generate reports from scenario results
    Report {
        /// Input results file
        #[arg(long)]
        input: std::path::PathBuf,
        /// Output report file
        #[arg(long)]
        output: std::path::PathBuf,
        /// Report format (text, json, html)
        #[arg(long)]
        format: Option<String>,
        /// Include detailed information
        #[arg(long)]
        detailed: bool,
    },
}

/// Snapshot maintenance subcommands.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum SnapshotAction {
    /// Run the full Snapshot_v1 ceremony locally (propose + commit + GC).
    Propose,
}

/// Admin maintenance subcommands.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum AdminAction {
    /// Replace the administrator for an account (stub, writes journal fact).
    Replace {
        /// Account identifier (UUID string).
        #[arg(long)]
        account: String,
        /// Device ID of the new admin (UUID string).
        #[arg(long)]
        new_admin: String,
        /// Epoch when the new admin becomes authoritative.
        #[arg(long)]
        activation_epoch: u64,
    },
}

/// Recovery subcommands exposed via CLI.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum RecoveryAction {
    /// Initiate guardian recovery from the local device.
    Start {
        /// Account identifier to recover.
        #[arg(long)]
        account: String,
        /// Comma separated guardian device IDs.
        #[arg(long)]
        guardians: String,
        /// Required guardian threshold (defaults to 2).
        #[arg(long, default_value = "2")]
        threshold: u32,
        /// Recovery priority (normal|urgent|emergency).
        #[arg(long, default_value = "normal")]
        priority: String,
        /// Dispute window in hours (guardians can object before finalize).
        #[arg(long, default_value = "48")]
        dispute_hours: u64,
        /// Optional human readable justification recorded in the request.
        #[arg(long)]
        justification: Option<String>,
    },
    /// Approve a guardian recovery request from this device.
    Approve {
        /// Path to a serialized recovery request (JSON).
        #[arg(long)]
        request_file: std::path::PathBuf,
    },
    /// Show local guardian recovery status and cooldown timers.
    Status,
    /// File a dispute against a recovery evidence record.
    Dispute {
        /// Evidence identifier returned by `aura recovery start`.
        #[arg(long)]
        evidence: String,
        /// Human readable reason included in the dispute log.
        #[arg(long)]
        reason: String,
    },
}

/// Invitation subcommands.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum InvitationAction {
    /// Create a device invitation envelope and broadcast it.
    Create {
        /// Account identifier.
        #[arg(long)]
        account: String,
        /// Device ID of the invitee.
        #[arg(long)]
        invitee: String,
        /// Role granted to the invitee.
        #[arg(long, default_value = "device")]
        role: String,
        /// Optional TTL in seconds.
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Accept an invitation envelope serialized to disk.
    Accept {
        /// Path to the invitation envelope JSON file.
        #[arg(long)]
        envelope: std::path::PathBuf,
    },
}

/// CLI error types
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}
