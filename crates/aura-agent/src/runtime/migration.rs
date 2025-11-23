//! Migration utilities
//!
//! Placeholder for migration utilities to help transition from
//! the old architecture to the new authority-centric design.

/// Migration coordinator
#[derive(Debug)]
pub struct MigrationCoordinator;

impl MigrationCoordinator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MigrationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}
