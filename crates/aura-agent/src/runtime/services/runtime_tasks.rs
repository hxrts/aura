//! Runtime task registry for agent background work.
//!
//! This module re-exports the shared task registry used across the runtime
//! and reactive pipeline to avoid duplicated logic.

pub use crate::task_registry::TaskRegistry as RuntimeTaskRegistry;
