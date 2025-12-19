//! Test that `build()` is not available when StorageEffects is missing.

use std::sync::Arc;
use aura_agent::AgentBuilder;
use aura_effects::{
    PhysicalTimeHandler, RealConsoleHandler,
    RealCryptoHandler, RealRandomHandler,
};

fn main() {
    // Should NOT compile: missing with_storage()
    let _builder = AgentBuilder::custom()
        .with_crypto(Arc::new(RealCryptoHandler::new()))
        .with_time(Arc::new(PhysicalTimeHandler::new()))
        .with_random(Arc::new(RealRandomHandler::new()))
        .with_console(Arc::new(RealConsoleHandler::new()))
        .build();
}
