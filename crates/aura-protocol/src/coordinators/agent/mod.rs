//! Layer 4: Agent Effect Handlers - Authentication, Session, System
//!
//! Effect handlers for agent operations implementing agent effect traits (Layer 1).
//! These handlers manage device authentication, session lifecycle, and system status.
//!
//! **Handler Types**:
//! - **AuthenticationHandler**: Device authentication and credential verification
//! - **MemorySessionHandler**: In-memory session lifecycle management
//! - **AgentEffectSystemHandler**: Agent system status and configuration
//!
//! **Note**: Handlers here focus on effect implementations; orchestration logic
//! (state machines, workflows) lives in aura-agent (Layer 6)

pub mod auth;
pub mod session;
pub mod system;

pub use auth::AuthenticationHandler;
pub use session::MemorySessionHandler;
pub use system::AgentEffectSystemHandler;
