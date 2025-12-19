//! Minimal Custom Runtime Example
//!
//! This example demonstrates how to create an Aura agent with explicit control
//! over all effect handlers using the custom builder pattern.
//!
//! The custom builder uses Rust's type system (typestate pattern) to enforce
//! that all required effects are provided at compile time.
//!
//! # Running
//!
//! ```bash
//! cargo run --package aura-agent --example minimal_custom_runtime
//! ```

use std::sync::Arc;

use aura_agent::AgentBuilder;
use aura_effects::{
    FilesystemStorageHandler, PhysicalTimeHandler, RealConsoleHandler, RealCryptoHandler,
    RealRandomHandler,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for visibility
    tracing_subscriber::fmt::init();

    println!("Creating Aura agent with custom effect handlers...\n");

    // Create individual effect handlers
    // Each handler implements a single effect trait from aura-core
    let crypto = Arc::new(RealCryptoHandler::new());
    let storage = Arc::new(FilesystemStorageHandler::new(
        std::env::temp_dir().join("aura-example"),
    ));
    let time = Arc::new(PhysicalTimeHandler);
    let random = Arc::new(RealRandomHandler);
    let console = Arc::new(RealConsoleHandler);

    // Build the agent using the custom preset
    // All five required effects must be provided - the type system enforces this
    let agent = AgentBuilder::custom()
        .with_crypto(crypto)
        .with_storage(storage)
        .with_time(time)
        .with_random(random)
        .with_console(console)
        .testing_mode() // Use testing mode for this example
        .build()
        .await?;

    println!("Agent created successfully!");
    println!("Authority ID: {:?}", agent.authority_id());

    // The agent is now ready to use
    // In a real application, you would interact with the agent's services

    Ok(())
}
