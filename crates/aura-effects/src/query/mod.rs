//! Query Effect Handler
//!
//! Implements `QueryEffects` for executing typed Datalog queries.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect (Layer 3)
//! - **Purpose**: Execute typed queries against journal facts via Datalog
//! - **Dependencies**: JournalEffects, AuthorizationEffects, ReactiveEffects
//!
//! # Architecture
//!
//! QueryHandler bridges the gap between typed queries (from aura-app) and
//! the Datalog execution engine (via Biscuit). It:
//! 1. Compiles Query::to_datalog() to executable programs
//! 2. Checks authorization via Biscuit capabilities
//! 3. Executes against journal facts
//! 4. Parses results back to typed values
//! 5. Supports live subscriptions with automatic invalidation
//!
//! # Module Structure
//!
//! - `handler` - QueryHandler implementation and supporting types
//! - `datalog` - Datalog formatting and parsing utilities

mod datalog;
mod handler;

// Re-export the main handler
pub use handler::QueryHandler;

// Re-export datalog utilities for external use
pub use datalog::{format_rule, format_value, parse_arg_to_value, parse_fact_to_row};
