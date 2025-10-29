//! Aura Protocol Types
//!
//! Core types used by the coordination runtime and transport layer.
//! This crate breaks the circular dependency between runtime and transport.

pub mod messages;
pub mod protocol;
pub mod runtime;
pub mod session;

// Re-export commonly used types
pub use messages::{DkdResult, TransportEvent};
pub use protocol::{ParticipantId, SessionId, ThresholdConfig};
pub use runtime::{SessionCommand, SessionResponse, SessionResult, TransportSession};
pub use session::{SessionProtocolType, SessionStatus, SessionStatusInfo};
