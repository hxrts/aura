//! Data integrity, digest stability, and migration validation tests.

#[path = "support.rs"]
mod shared_support;

mod integrity {
    mod anti_entropy_digest_stability;
    mod anti_entropy_idempotence;
    mod migration_validation;
}
