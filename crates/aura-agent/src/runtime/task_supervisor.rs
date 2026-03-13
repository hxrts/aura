//! Runtime-local supervision surface for structured concurrency.
//!
//! This keeps the runtime-facing task ownership API explicit even while the
//! underlying implementation remains in the shared crate-level task registry.

pub use crate::task_registry::{TaskGroup, TaskSupervisionError, TaskSupervisor};
