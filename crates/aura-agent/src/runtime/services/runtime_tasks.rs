//! Runtime task supervision for agent background work.
//!
//! This module re-exports the shared supervisor used across the runtime and
//! reactive pipeline to avoid duplicated logic.

#[allow(unused_imports)]
pub use crate::task_registry::{TaskGroup, TaskSupervisionError, TaskSupervisor};
pub use TaskSupervisor as RuntimeTaskRegistry;
