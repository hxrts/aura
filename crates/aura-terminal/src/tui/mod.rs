//! # Aura TUI - Terminal User Interface
//!
//! Layer 7 (User Interface) - IRC-style terminal interface for Aura.
//!
//! Built with iocraft for declarative, React-like UI components.
//!
//! ## Module Organization
//!
//! - **screens**: Full-screen views (Neighborhood, Chat, Notifications, etc.)
//! - **components**: Reusable UI widgets (Modal, Toast, CommandPalette)
//! - **context**: IoContext for effect dispatch and reactive data
//! - **theme**: Centralized color and style constants
//! - **types**: Shared domain types (Channel, Message, etc.)
//! - **hooks**: futures-signals integration for reactive state
//! - **reactive**: Reactive view layer (queries, views, signals)
//! - **effects**: Bridge to Aura effect system
//! - **commands**: IRC command parser
//! - **state_machine**: Pure state machine model for deterministic testing
//! - **iocraft_adapter**: Bridge between iocraft events and TerminalEffects trait
//!
//! ## Testing Architecture
//!
//! The TUI uses a pure state machine model for deterministic testing:
//!
//! ```text
//! TuiState × TerminalEvent → (TuiState, Vec<TuiCommand>)
//! ```
//!
//! This enables:
//! - **Fast tests**: No PTY setup, no sleeps, pure computation (<1ms per test)
//! - **Determinism**: Same inputs = same outputs, every time
//! - **Debuggability**: Full state visibility at every step
//! - **Formal verification**: semantic traces and invariants flow through the
//!   shared scenario contract rather than a direct Quint-to-TUI replay path
//!
//! See `tests/tui_deterministic.rs` for examples.

#![deny(clippy::print_stdout, clippy::print_stderr)]

// Core iocraft modules
pub mod components;
pub mod context;
pub(crate) mod fullscreen_stdio;
pub(crate) mod harness_state;
pub mod hooks;
pub mod iocraft_adapter;
pub mod layout;
pub mod props;
pub mod runtime;
pub mod screens;
pub mod state;
pub mod tasks;
pub mod theme;
pub(crate) mod timeout_support;
pub mod types;
pub mod updates;

// Shared infrastructure
pub mod callbacks;
pub(crate) mod channel_selection;
pub(crate) mod chat_scope;
pub mod commands;
pub mod effects;
pub mod flow_budget;
pub(crate) mod key_rotation;
pub mod keymap;
pub mod local_store;
pub mod navigation;
pub(crate) mod semantic_lifecycle;

// Public surface area
//
// Prefer explicit module paths (e.g. `tui::screens::Screen`) over wide re-exports.
// This keeps boundaries clear and avoids accidental coupling between layers.

pub use context::IoContext;
pub use screens::{run_app_with_context, Screen};
pub use state::{transition, TuiCommand, TuiState};
