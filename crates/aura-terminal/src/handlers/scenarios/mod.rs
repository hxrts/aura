//! Scenarios Command Handler
//!
//! Effect-based implementation of scenario management commands.
//!
//! This module is organized into submodules for maintainability:
//! - `handlers`: Individual command handlers (discover, list, validate, run, report)
//! - `execution`: Scenario file parsing and execution
//! - `simulation`: CLI recovery demo simulation
//! - `logging`: Scenario logging and persistence
//! - `types`: Shared data structures

mod execution;
mod handlers;
mod logging;
mod simulation;
mod types;

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::HandlerContext;
use crate::ScenarioAction;

// Re-export types for external use
pub use types::{ScenarioInfo, ScenarioResult};

/// Handle scenario operations through effects
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_scenarios(ctx: &HandlerContext<'_>, action: &ScenarioAction) -> TerminalResult<()> {
    match action {
        ScenarioAction::Discover { root, validate } => {
            handlers::handle_discover(ctx, root, *validate).await
        }
        ScenarioAction::List {
            directory,
            detailed,
        } => handlers::handle_list(ctx, directory, *detailed).await,
        ScenarioAction::Validate {
            directory,
            strictness,
        } => handlers::handle_validate(ctx, directory, strictness.as_deref()).await,
        ScenarioAction::Run {
            directory,
            pattern,
            parallel,
            max_parallel,
            output_file,
            detailed_report,
        } => {
            handlers::handle_run(
                ctx,
                directory.as_deref(),
                pattern.as_deref(),
                *parallel,
                *max_parallel,
                output_file.as_deref(),
                *detailed_report,
            )
            .await
        }
        ScenarioAction::Report {
            input,
            output,
            format,
            detailed,
        } => handlers::handle_report(ctx, input, output, format.as_deref(), *detailed).await,
    }
}
