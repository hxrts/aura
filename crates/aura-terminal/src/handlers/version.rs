//! Version Command Handler
//!
//! Effect-based implementation of the version command.

use crate::handlers::HandlerContext;
use anyhow::Result;

/// Handle version display through effects
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_version(_ctx: &HandlerContext<'_>) -> Result<()> {
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
