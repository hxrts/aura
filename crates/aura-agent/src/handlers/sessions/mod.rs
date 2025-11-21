//! Session Management Modules
//!
//! This module contains role-focused session management handlers split by concern:
//! - shared: Common types and utilities
//! - coordination: Session coordination handlers
//! - threshold: Threshold operation session handlers  
//! - metadata: Session metadata management

pub mod shared;
pub mod coordination;
pub mod threshold;
pub mod metadata;

// Re-export the main session operations handler
pub use coordination::SessionOperations;

// Re-export common types
pub use shared::{
    SessionHandle, SessionStats, SessionManagementRole, DeviceInfo,
    SessionCreateRequest, SessionInvitation, SessionResponse,
    SessionEstablished, SessionFailed, SessionEnd, SessionTerminated,
    MetadataUpdate, MetadataSync, ParticipantChange, ParticipantUpdate,
};