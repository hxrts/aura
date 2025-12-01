//! Migration utilities
//!
//! Provides scaffolding for future data and configuration migrations between
//! runtime versions. Concrete migration steps will be added alongside schema
//! changes in authority-centric releases.

/// Migration coordinator
#[derive(Debug)]
pub struct MigrationCoordinator;

impl MigrationCoordinator {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MigrationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}
