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

// Re-export common types
