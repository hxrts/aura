// Command modules for CLI
//
// NOTE: Temporarily simplified - agent/coordination dependent commands disabled

// Temporarily disabled - requires agent crate
// pub mod authz; // Authorization commands (what you can do)

/// Shared utilities for all commands
pub mod common;

// Temporarily disabled - requires agent crate
// pub mod debug; // Interactive debugging tools and analysis commands

/// Account initialization command
pub mod init;

// Temporarily disabled - requires agent crate
// pub mod network;
// pub mod node; // Node management and dev console commands

/// Scenario management and execution commands
pub mod scenarios;

/// Account status command
pub mod status;

// Temporarily disabled - requires agent crate
// pub mod storage;
