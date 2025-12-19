//! Test that `build()` is not available when ConsoleEffects is missing.

use std::sync::Arc;
use aura_agent::AgentBuilder;
use aura_effects::{
    FilesystemStorageHandler, PhysicalTimeHandler,
    RealCryptoHandler, RealRandomHandler,
};

fn main() {
    // Create a temp path for storage
    let temp_dir = std::path::PathBuf::from("/tmp/test");

    // Should NOT compile: missing with_console()
    let _builder = AgentBuilder::custom()
        .with_crypto(Arc::new(RealCryptoHandler::new()))
        .with_storage(Arc::new(FilesystemStorageHandler::new(temp_dir)))
        .with_time(Arc::new(PhysicalTimeHandler::new()))
        .with_random(Arc::new(RealRandomHandler::new()))
        .build();
}
