//! Test configuration & environment settings
//!
//! This module provides standardized configuration patterns for test execution,
//! including test configuration options, network settings, and privacy-specific
//! configuration.

pub mod config;
pub mod privacy;

pub use config::*;
pub use privacy::*;
