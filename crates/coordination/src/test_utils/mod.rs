//! Test utilities for coordination layer
//!
//! This module provides testing utilities and frameworks for the coordination layer,
//! including error scenario testing, mock implementations, and test helpers.

pub mod error_scenarios;

pub use error_scenarios::{
    ErrorScenarioTester, NetworkConditions, DeviceConfig, TestError
};