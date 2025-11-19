//! Version Command Handler
//!
//! Effect-based implementation of the version command.

use anyhow::Result;
use aura_agent::runtime::AuraEffectSystem;
use aura_protocol::effect_traits::ConsoleEffects;

/// Handle version display through effects
pub async fn handle_version(effects: &AuraEffectSystem) -> Result<()> {
    // Display version information through console effects
    let _ = effects
        .log_info(&format!("aura {}", env!("CARGO_PKG_VERSION")))
        .await;

    // Additional version details
    let _ = effects
        .log_info(&format!("Package: {}", env!("CARGO_PKG_NAME")))
        .await;

    let _ = effects
        .log_info(&format!("Description: {}", env!("CARGO_PKG_DESCRIPTION")))
        .await;

    let _ = effects
        .log_info(&format!(
            "Built with: {} {}",
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_VERSION")
        ))
        .await;

    Ok(())
}
