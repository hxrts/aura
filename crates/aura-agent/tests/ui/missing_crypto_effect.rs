//! Test that `build()` is not available when CryptoEffects is missing.
//!
//! Even with four out of five required effects provided,
//! `build()` should not be callable.

use std::sync::Arc;
use aura_agent::AgentBuilder;
use aura_effects::{
    FilesystemStorageHandler, PhysicalTimeHandler,
    RealConsoleHandler, RealRandomHandler,
};

fn main() {
    // Create a temp path for storage
    let temp_dir = std::path::PathBuf::from("/tmp/test");

    // Should NOT compile: missing with_crypto()
    let _builder = AgentBuilder::custom()
        .with_storage(Arc::new(FilesystemStorageHandler::new(temp_dir)))
        .with_time(Arc::new(PhysicalTimeHandler::new()))
        .with_random(Arc::new(RealRandomHandler::new()))
        .with_console(Arc::new(RealConsoleHandler::new()))
        .build();
}
