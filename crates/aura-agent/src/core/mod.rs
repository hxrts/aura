//! Layer 6: Agent Public API - Builder, Config, Context, Errors
//!
//! Minimal public-facing API for runtime composition. Provides **AgentBuilder**,
//! **AgentConfig**, **AuthorityContext**, and **AgentError** for application integration.
//!
//! **Design Principle** (per docs/001_system_architecture.md):
//! Layer 6 provides clean API surface; internal orchestration (registry, services, lifecycle)
//! lives in aura-agent/runtime and aura-protocol/handlers (Layer 4). Enables applications
//! to drive the agent without exposing implementation details.

pub mod agent;
pub mod builder;
pub mod ceremony;
pub mod ceremony_processor;
pub mod config;
pub mod context;
pub mod error;
pub mod guardian;

pub use agent::AuraAgent;
pub use builder::AgentBuilder;
pub use config::{default_storage_path, AgentConfig};
pub use context::{default_context_id_for_authority, AuthorityContext};
pub use error::{AgentError, AgentResult};
