//! Session Management Modules
//!
//! This module contains role-focused session management handlers split by concern:
//! - shared: Common types and utilities
//! - coordination: Session coordination handlers
//! - threshold: Threshold operation session handlers  
//! - metadata: Session metadata management

pub mod coordination;
pub mod metadata;
pub mod shared;
pub mod threshold;

// Re-export the main session operations handler
pub use coordination::SessionOperations;

// Re-export common types
pub use shared::{
    DeviceInfo, MetadataSync, MetadataUpdate, ParticipantChange, ParticipantUpdate,
    SessionCreateRequest, SessionEnd, SessionEstablished, SessionFailed, SessionHandle,
    SessionInvitation, SessionManagementRole, SessionResponse, SessionStats, SessionTerminated,
};
