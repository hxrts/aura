//! Version Command Handler
//!
//! Effect-based implementation of the version command.

use anyhow::Result;
use aura_agent::AuraEffectSystem;
use aura_protocol::effect_traits::ConsoleEffects;

/// Handle version display through effects
pub async fn handle_version(_effects: &AuraEffectSystem) -> Result<()> {
    // Display version information through console effects
    println!("aura {}", env!("CARGO_PKG_VERSION"));

    // Additional version details
    println!("Package: {}", env!("CARGO_PKG_NAME"));

    println!("Description: {}", env!("CARGO_PKG_DESCRIPTION"));

    println!(
        "Built with: {} {}",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    );

    Ok(())
}
