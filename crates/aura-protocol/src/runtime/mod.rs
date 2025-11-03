//! Protocol Runtime Module
//!
//! This module contains execution context, session management, and the effect runtime
//! that coordinates between effect handlers and protocol execution.
//!
//! ## Architecture Principles
//!
//! 1. **Execution Context**: Manages the environment in which protocols execute
//! 2. **Session Management**: Handles protocol session lifecycle and coordination
//! 3. **Effect Runtime**: Coordinates effect handler execution and middleware application
//! 4. **Resource Management**: Handles cleanup and resource lifecycle
//!
//! ## Components
//!
//! - **Context**: Execution environment with device identity, session info, and handlers
//! - **Session**: Protocol session state and participant coordination
//! - **Executor**: Effect execution engine that applies middleware and manages handlers
//! - **Registry**: Service registry for effect handler discovery and injection

pub mod context;
pub mod executor;
pub mod session;

pub use context::{ExecutionContext, ContextBuilder};
pub use executor::{EffectExecutor, ExecutorConfig, ExecutionMode};
pub use session::{SessionManager, SessionState, SessionConfig, SessionStatus};