//! Migration Infrastructure
//!
//! Provides the framework for data and schema migrations between protocol versions.

#![allow(dead_code)] // Infrastructure module, not yet wired into runtime

use async_trait::async_trait;
use aura_core::protocol::{CURRENT_VERSION, MIN_SUPPORTED_VERSION};
use aura_core::SemanticVersion;
use std::sync::Arc;
use thiserror::Error;

/// Error type for migration operations.
#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("Migration validation failed: {0}")]
    ValidationFailed(String),
    #[error("Migration execution failed: {0}")]
    ExecutionFailed(String),
    #[error("No migration path from {from} to {to}")]
    NoMigrationPath {
        from: SemanticVersion,
        to: SemanticVersion,
    },
    #[error("Already at target version {0}")]
    AlreadyCurrent(SemanticVersion),
    #[error("Internal migration error: {0}")]
    Internal(String),
}

/// Context provided to migrations.
pub struct MigrationContext {
    pub from_version: SemanticVersion,
    pub to_version: SemanticVersion,
    pub started_at_ms: u64,
    pub dry_run: bool,
}

impl MigrationContext {
    pub fn new(from_version: SemanticVersion, to_version: SemanticVersion, now_ms: u64) -> Self {
        Self {
            from_version,
            to_version,
            started_at_ms: now_ms,
            dry_run: false,
        }
    }

    pub fn dry_run(
        from_version: SemanticVersion,
        to_version: SemanticVersion,
        now_ms: u64,
    ) -> Self {
        Self {
            from_version,
            to_version,
            started_at_ms: now_ms,
            dry_run: true,
        }
    }
}

/// Trait for individual migrations.
#[async_trait]
pub trait Migration: Send + Sync {
    fn source_version(&self) -> SemanticVersion;
    fn target_version(&self) -> SemanticVersion;
    fn name(&self) -> &str;
    async fn validate(&self, ctx: &MigrationContext) -> Result<(), MigrationError>;
    async fn execute(&self, ctx: &MigrationContext) -> Result<(), MigrationError>;
    async fn rollback(&self, _ctx: &MigrationContext) -> Result<bool, MigrationError> {
        Ok(false)
    }
}

/// Result of a migration attempt.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    pub from_version: SemanticVersion,
    pub to_version: SemanticVersion,
    pub migrations_applied: usize,
    pub duration_ms: u64,
}

/// Coordinates protocol version migrations.
pub struct MigrationCoordinator {
    current_version: SemanticVersion,
    min_version: SemanticVersion,
    migrations: Vec<Arc<dyn Migration>>,
}

impl std::fmt::Debug for MigrationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MigrationCoordinator")
            .field("current_version", &self.current_version)
            .field("min_version", &self.min_version)
            .field("migrations_count", &self.migrations.len())
            .finish()
    }
}

impl MigrationCoordinator {
    pub fn new() -> Self {
        Self {
            current_version: CURRENT_VERSION,
            min_version: MIN_SUPPORTED_VERSION,
            migrations: Vec::new(),
        }
    }

    pub fn with_versions(current: SemanticVersion, min: SemanticVersion) -> Self {
        Self {
            current_version: current,
            min_version: min,
            migrations: Vec::new(),
        }
    }

    pub fn register(&mut self, migration: Arc<dyn Migration>) {
        self.migrations.push(migration);
        self.migrations.sort_by(|a, b| {
            a.target_version()
                .partial_cmp(&b.target_version())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    pub fn needs_migration(&self, from: SemanticVersion) -> bool {
        from < self.current_version && from >= self.min_version
    }

    pub fn get_migration_path(
        &self,
        from: SemanticVersion,
        to: SemanticVersion,
    ) -> Option<Vec<&dyn Migration>> {
        if from >= to {
            return Some(Vec::new());
        }
        let mut path = Vec::new();
        let mut current = from;
        for migration in &self.migrations {
            if migration.source_version() < current {
                continue;
            }
            if migration.source_version() <= current && migration.target_version() > current {
                path.push(migration.as_ref());
                current = migration.target_version();
                if current >= to {
                    break;
                }
            }
        }
        if current >= to {
            Some(path)
        } else {
            None
        }
    }

    pub async fn migrate(
        &self,
        from: SemanticVersion,
        to: Option<SemanticVersion>,
        now_ms: u64,
    ) -> Result<MigrationResult, MigrationError> {
        let target = to.unwrap_or(self.current_version);
        if from >= target {
            return Err(MigrationError::AlreadyCurrent(from));
        }
        if from < self.min_version {
            return Err(MigrationError::NoMigrationPath { from, to: target });
        }
        let path = self
            .get_migration_path(from, target)
            .ok_or(MigrationError::NoMigrationPath { from, to: target })?;
        if path.is_empty() {
            return Ok(MigrationResult {
                from_version: from,
                to_version: from,
                migrations_applied: 0,
                duration_ms: 0,
            });
        }
        let mut current_version = from;
        for migration in &path {
            let ctx = MigrationContext::new(current_version, migration.target_version(), now_ms);
            migration.validate(&ctx).await.map_err(|e| {
                MigrationError::ValidationFailed(format!(
                    "Migration {} validation failed: {e}",
                    migration.name()
                ))
            })?;
            migration.execute(&ctx).await.map_err(|e| {
                MigrationError::ExecutionFailed(format!(
                    "Migration {} execution failed: {e}",
                    migration.name()
                ))
            })?;
            tracing::info!(migration = %migration.name(), from = %current_version, to = %migration.target_version(), "Migration applied successfully");
            current_version = migration.target_version();
        }
        Ok(MigrationResult {
            from_version: from,
            to_version: current_version,
            migrations_applied: path.len(),
            duration_ms: 0,
        })
    }

    pub async fn validate_migration(
        &self,
        from: SemanticVersion,
        to: Option<SemanticVersion>,
        now_ms: u64,
    ) -> Result<usize, MigrationError> {
        let target = to.unwrap_or(self.current_version);
        let path = self
            .get_migration_path(from, target)
            .ok_or(MigrationError::NoMigrationPath { from, to: target })?;
        for migration in &path {
            let ctx = MigrationContext::dry_run(from, migration.target_version(), now_ms);
            migration.validate(&ctx).await?;
        }
        Ok(path.len())
    }

    pub fn list_migrations(&self) -> Vec<MigrationInfo> {
        self.migrations
            .iter()
            .map(|m| MigrationInfo {
                name: m.name().to_string(),
                source_version: m.source_version(),
                target_version: m.target_version(),
            })
            .collect()
    }
}

impl Default for MigrationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct MigrationInfo {
    pub name: String,
    pub source_version: SemanticVersion,
    pub target_version: SemanticVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMigration {
        name: String,
        source: SemanticVersion,
        target: SemanticVersion,
        should_fail_validation: bool,
        should_fail_execution: bool,
    }

    impl TestMigration {
        fn new(name: &str, source: SemanticVersion, target: SemanticVersion) -> Self {
            Self {
                name: name.to_string(),
                source,
                target,
                should_fail_validation: false,
                should_fail_execution: false,
            }
        }
    }

    #[async_trait]
    impl Migration for TestMigration {
        fn source_version(&self) -> SemanticVersion {
            self.source
        }
        fn target_version(&self) -> SemanticVersion {
            self.target
        }
        fn name(&self) -> &str {
            &self.name
        }
        async fn validate(&self, _ctx: &MigrationContext) -> Result<(), MigrationError> {
            if self.should_fail_validation {
                Err(MigrationError::ValidationFailed("Test failure".to_string()))
            } else {
                Ok(())
            }
        }
        async fn execute(&self, _ctx: &MigrationContext) -> Result<(), MigrationError> {
            if self.should_fail_execution {
                Err(MigrationError::ExecutionFailed("Test failure".to_string()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_needs_migration() {
        let coordinator = MigrationCoordinator::with_versions(
            SemanticVersion::new(1, 2, 0),
            SemanticVersion::new(1, 0, 0),
        );
        assert!(coordinator.needs_migration(SemanticVersion::new(1, 0, 0)));
        assert!(coordinator.needs_migration(SemanticVersion::new(1, 1, 0)));
        assert!(!coordinator.needs_migration(SemanticVersion::new(1, 2, 0)));
        assert!(!coordinator.needs_migration(SemanticVersion::new(1, 3, 0)));
        assert!(!coordinator.needs_migration(SemanticVersion::new(0, 9, 0)));
    }

    #[test]
    fn test_migration_path() {
        let mut coordinator = MigrationCoordinator::with_versions(
            SemanticVersion::new(1, 3, 0),
            SemanticVersion::new(1, 0, 0),
        );
        coordinator.register(Arc::new(TestMigration::new(
            "migrate_1_0_to_1_1",
            SemanticVersion::new(1, 0, 0),
            SemanticVersion::new(1, 1, 0),
        )));
        coordinator.register(Arc::new(TestMigration::new(
            "migrate_1_1_to_1_2",
            SemanticVersion::new(1, 1, 0),
            SemanticVersion::new(1, 2, 0),
        )));
        coordinator.register(Arc::new(TestMigration::new(
            "migrate_1_2_to_1_3",
            SemanticVersion::new(1, 2, 0),
            SemanticVersion::new(1, 3, 0),
        )));
        let path = coordinator
            .get_migration_path(SemanticVersion::new(1, 0, 0), SemanticVersion::new(1, 3, 0))
            .unwrap();
        assert_eq!(path.len(), 3);
        let path = coordinator
            .get_migration_path(SemanticVersion::new(1, 1, 0), SemanticVersion::new(1, 3, 0))
            .unwrap();
        assert_eq!(path.len(), 2);
        let path = coordinator
            .get_migration_path(SemanticVersion::new(1, 3, 0), SemanticVersion::new(1, 3, 0))
            .unwrap();
        assert!(path.is_empty());
    }

    #[tokio::test]
    async fn test_migrate_success() {
        let mut coordinator = MigrationCoordinator::with_versions(
            SemanticVersion::new(1, 2, 0),
            SemanticVersion::new(1, 0, 0),
        );
        coordinator.register(Arc::new(TestMigration::new(
            "migrate_1_0_to_1_1",
            SemanticVersion::new(1, 0, 0),
            SemanticVersion::new(1, 1, 0),
        )));
        coordinator.register(Arc::new(TestMigration::new(
            "migrate_1_1_to_1_2",
            SemanticVersion::new(1, 1, 0),
            SemanticVersion::new(1, 2, 0),
        )));
        let result = coordinator
            .migrate(SemanticVersion::new(1, 0, 0), None, 12345)
            .await
            .unwrap();
        assert_eq!(result.from_version, SemanticVersion::new(1, 0, 0));
        assert_eq!(result.to_version, SemanticVersion::new(1, 2, 0));
        assert_eq!(result.migrations_applied, 2);
    }

    #[tokio::test]
    async fn test_migrate_already_current() {
        let coordinator = MigrationCoordinator::with_versions(
            SemanticVersion::new(1, 0, 0),
            SemanticVersion::new(1, 0, 0),
        );
        let result = coordinator
            .migrate(SemanticVersion::new(1, 0, 0), None, 12345)
            .await;
        assert!(matches!(result, Err(MigrationError::AlreadyCurrent(_))));
    }

    #[test]
    fn test_list_migrations() {
        let mut coordinator = MigrationCoordinator::new();
        coordinator.register(Arc::new(TestMigration::new(
            "test_migration",
            SemanticVersion::new(1, 0, 0),
            SemanticVersion::new(1, 1, 0),
        )));
        let migrations = coordinator.list_migrations();
        assert_eq!(migrations.len(), 1);
        assert_eq!(migrations[0].name, "test_migration");
    }
}
