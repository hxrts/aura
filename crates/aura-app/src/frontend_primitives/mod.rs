//! Shared frontend primitives for all Layer 7 shells.
//!
//! This module provides platform-agnostic types that every frontend
//! (Dioxus-based or otherwise) consumes. Shell crates may re-export
//! these types under their own aliases but should not fork them.

mod cancellation_waiters;
mod clipboard;
mod debug_probe;
mod operations;
mod submitted_operation;
mod task_owner;

pub use clipboard::{ClipboardPort, MemoryClipboard};
pub use debug_probe::{emit_frontend_debug_probe, set_frontend_debug_probe};
pub use operations::FrontendUiOperation;
pub use submitted_operation::{
    dropped_owner_error, CeremonyMonitorHandoffRelease, CeremonyMonitorHandoffSubmission,
    CeremonySubmissionTerminalOutcome, LocalTerminalSubmission, SubmittedOperation,
    SubmittedOperationPublisher, SubmittedOperationRelease, SubmittedOperationWorkflowError,
    WorkflowHandoffRelease, WorkflowHandoffSubmission,
};
pub use task_owner::{FrontendTaskManager, FrontendTaskOwner, FrontendTaskRuntime};
