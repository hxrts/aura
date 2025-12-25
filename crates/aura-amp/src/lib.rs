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

#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods,
    deprecated
)]
//! # Aura AMP - Layer 4: Authenticated Messaging Protocol

pub mod amp;

pub use amp::*;
