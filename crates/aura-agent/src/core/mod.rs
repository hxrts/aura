//! Layer 6: Agent Public API - Builder, Config, Context, Errors
//!
//! Minimal public-facing API for runtime composition. Provides **AgentBuilder**,
//! **AgentConfig**, **AuthorityContext**, and **AgentError** for application integration.
//!
//! **Design Principle** (per docs/001_system_architecture.md):
//! Layer 6 provides clean API surface; internal orchestration (registry, services, lifecycle)
//! lives in aura-agent/runtime and aura-protocol/handlers (Layer 4). Enables applications
//! to drive the agent without exposing implementation details.

pub mod api;
pub mod config;
pub mod context;
pub mod error;

pub use api::{AgentBuilder, AuraAgent};
pub use config::AgentConfig;
pub use context::AuthorityContext;
pub use error::{AgentError, AgentResult};
