//! Test that `build()` is not available when RandomEffects is missing.

use std::sync::Arc;
use aura_agent::AgentBuilder;
use aura_effects::{
    FilesystemStorageHandler, PhysicalTimeHandler,
    RealConsoleHandler, RealCryptoHandler,
};

fn main() {
    // Create a temp path for storage
    let temp_dir = std::path::PathBuf::from("/tmp/test");

    // Should NOT compile: missing with_random()
    let _builder = AgentBuilder::custom()
        .with_crypto(Arc::new(RealCryptoHandler::new()))
        .with_storage(Arc::new(FilesystemStorageHandler::new(temp_dir)))
        .with_time(Arc::new(PhysicalTimeHandler::new()))
        .with_console(Arc::new(RealConsoleHandler::new()))
        .build();
}
