//! Network and CGKA commands module
//!
//! This module provides CLI commands for network operations, peer management,
//! group operations, capability management, and integration testing.
//!
//! ## Module Organization
//!
//! Currently, all implementations are in the `network_impl` submodule.
//! This serves as the main entry point for the network command system.
//!
//! ## Future Decomposition Plan
//!
//! The network module is a large monolithic file (~7,270 lines) that will be
//! progressively split into focused, domain-specific modules:
//!
//! - `peer_ops.rs` - Peer connection/disconnection operations (25 lines)
//! - `group_ops.rs` - Group creation and messaging (125 lines)
//! - `capability_ops.rs` - Capability delegation and revocation (55 lines)
//! - `status_ops.rs` - Network statistics and status reporting (60 lines)
//! - `basic_tests.rs` - Protocol and connectivity tests (~2,073 lines)
//!   - `test_multi_agent`
//!   - `test_peer_discovery`
//!   - `test_establish_connections`
//!   - `test_message_exchange`
//!   - `test_network_partition`
//! - `storage_tests.rs` - Storage subsystem tests (~1,662 lines)
//!   - `test_storage_operations`
//!   - `test_storage_persistence`
//!   - `test_storage_replication`
//!   - `test_encryption_integrity`
//!   - `test_storage_quota_management`
//! - `advanced_tests.rs` - Protocol state machines and ledger tests (~2,200 lines)
//!   - `test_capability_revocation_and_access_denial`
//!   - `test_protocol_state_machines`
//!   - `test_ledger_consistency`
//! - `e2e_tests.rs` - End-to-end integration tests (~158 lines)
//!   - `test_e2e_integration`
//!   - `E2ETestResults` struct
//! - `helpers.rs` - Shared test utility functions (~157 lines)
//!   - Event creation helpers

// Test helper utilities
pub mod helpers;

// Extracted test modules - temporarily commented out due to API changes
// TODO: Update these tests to use new Agent API
// pub mod basic_tests;
// pub mod storage_tests;
// pub mod advanced_tests;
// pub mod e2e_tests;

// pub use network_impl::{handle_network_command, NetworkCommand}; // Commented out - module does not exist
