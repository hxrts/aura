//! Recovery coordinator infrastructure
//!
//! This module provides the base coordinator trait and common implementations
//! for all recovery operations, eliminating code duplication while maintaining
//! flexibility for specialized coordinator logic.

pub mod base;

// Re-export the main coordinator interfaces
pub use base::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator};
