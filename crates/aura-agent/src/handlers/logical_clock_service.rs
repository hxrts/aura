//! Logical clock service shim.
//!
//! The runtime-owned implementation now lives in `runtime::services`.

pub use crate::runtime::services::LogicalClockManager as LogicalClockService;
