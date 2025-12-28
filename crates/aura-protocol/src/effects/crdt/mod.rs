Aura Development Environment
============================

Rust version: rustc 1.90.0 (1159e78c4 2025-09-14)
Cargo version: cargo 1.90.0 (840b83a10 2025-07-30)
Quint version: 0.25.1
Apalache version: 0.45.4
TLA+ tools: available
Node.js version: v20.19.5
Lean version: Lean (version 4.23.0, arm64-apple-darwin, commit v4.23.0, Release)
Aeneas version: available

Available commands:
  just --list          Show all available tasks
  just build           Build all crates
  just test            Run all tests
  just check           Run clippy and format check
  just quint-parse     Parse Quint files to JSON
  trunk serve          Serve console with hot reload (in console/)
  quint --help         Formal verification with Quint
  apalache-mc --help   Model checking with Apalache
  lean --help          Kernel verification with Lean 4
  aeneas --help        Rust-to-Lean translation
  crate2nix --help     Generate hermetic Nix builds

Hermetic builds:
  nix build            Build with crate2nix (hermetic)
  nix build .#aura-terminal Build specific package
  nix run              Run aura CLI hermetically
  nix flake check      Run hermetic tests

//! Layer 4: CRDT Coordination (Choreographies + Delivery)
//!
//! This module provides choreography-facing coordination for CRDT synchronization.
//! It intentionally does **not** define the core CRDT handlers; those live in
//! `aura-journal::crdt` as pure, local enforcement of semilattice laws.
//!
//! Use this module for multi-party coordination:
//! - `CrdtCoordinator` (protocol bridge)
//! - Delivery guarantees and gossip configuration
//! - Execution helpers for choreography integration

mod composition;
mod crdt_coordinator;
mod delivery;
mod execution;

pub use composition::ComposedHandler;
pub use crdt_coordinator::{CrdtCoordinator, CrdtCoordinatorError};
pub use delivery::{DeliveryConfig, DeliveryEffect, DeliveryGuarantee, GossipStrategy, TopicId};
pub use execution::{execute_cv_sync, execute_delta_gossip, execute_op_broadcast};
