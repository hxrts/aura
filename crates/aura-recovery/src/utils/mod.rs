//! Common utilities for recovery operations
//!
//! This module provides shared utilities to eliminate code duplication
//! across the various recovery coordinators while maintaining loose coupling.

pub mod authorization;
pub mod evidence;
pub mod signatures;

// Re-export commonly used utilities
pub use authorization::AuthorizationHelper;
pub use evidence::EvidenceBuilder;
pub use signatures::SignatureUtils;
