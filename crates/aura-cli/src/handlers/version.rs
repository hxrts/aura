//! Version Command Handler
//!
//! Effect-based implementation of the version command.

use anyhow::Result;
use aura_protocol::{AuraEffectSystem, ConsoleEffects};

/// Handle version display through effects
pub async fn handle_version(effects: &AuraEffectSystem) -> Result<()> {
    // Display version information through console effects
    effects.log_info(&format!("aura {}", env!("CARGO_PKG_VERSION")), &[]);

    // Additional version details
    effects.log_info(&format!("Package: {}", env!("CARGO_PKG_NAME")), &[]);

    effects.log_info(
        &format!("Description: {}", env!("CARGO_PKG_DESCRIPTION")),
        &[],
    );

    effects.log_info(
        &format!(
            "Built with: {} {}",
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_VERSION")
        ),
        &[],
    );

    Ok(())
}
