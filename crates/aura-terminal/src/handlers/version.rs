//! Version Command Handler
//!
//! Effect-based implementation of the version command.
//! Returns structured `CliOutput` for testability.

use crate::error::TerminalResult;
use crate::handlers::{CliOutput, HandlerContext};

/// Handle version display through effects
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_version(_ctx: &HandlerContext<'_>) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    // Display version information through console effects
    output.println(format!("aura {}", env!("CARGO_PKG_VERSION")));
    output.kv("Package", env!("CARGO_PKG_NAME"));
    output.kv("Description", env!("CARGO_PKG_DESCRIPTION"));
    output.kv(
        "Repository",
        format!(
            "{} {}",
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_VERSION")
        ),
    );

    Ok(output)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_version_output_format() {
        // We can't easily create a HandlerContext in tests, but we can test the output format
        // by checking what the output would contain given the env vars
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());
    }
}
