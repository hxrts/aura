// Clippy allows for new crate integration
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
pub mod command;
pub mod demo_invitation;
pub mod env;
pub mod error;
pub mod handlers;
pub mod ids;
pub mod local_store;
pub mod rpc;
pub mod rpc_socket;
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
pub use cli::SyncDaemonArgs;
#[cfg(feature = "terminal")]
pub use cli::TuiArgs;
pub use handlers::CliHandler;

// Action types defined in this module (no re-export needed)

// Action types are defined in this module and automatically available
// Import app types from aura-app (pure layer)
use aura_app::ui::prelude::*;
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::{AgentBuilder, EffectContext};
use aura_core::{effects::ExecutionMode, types::identifiers::DeviceId, AuraError};
use std::sync::Arc;

// Re-export unified terminal error types
pub use error::{TerminalError, TerminalResult};

/// Create a CLI handler for the given device ID
///
/// Uses AppCore as the unified backend for all operations.
pub fn create_cli_handler(device_id: DeviceId) -> Result<CliHandler, AuraError> {
    let authority_id = ids::authority_id(&format!("cli:authority:{device_id}"));
    let context_id = ids::context_id(&format!("cli:context:{device_id}"));

    // Build agent
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing()
        .map_err(|e| AuraError::agent(format!("Agent build failed: {e}")))?;
    let agent = Arc::new(agent);

    // Create AppCore with the runtime bridge (dependency inversion)
    let config = AppConfig::default();
    let app_core = AppCore::with_runtime(config, agent.clone().as_runtime_bridge())
        .map_err(|e| AuraError::agent(format!("AppCore creation failed: {e}")))?;
    let app_core = Arc::new(RwLock::new(app_core));

    let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
    Ok(CliHandler::with_agent(
        app_core,
        agent,
        device_id,
        effect_context,
    ))
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
