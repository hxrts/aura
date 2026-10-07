//! Version Command Handler
//!
//! Returns structured `CliOutput`; needs no runtime or account.

use crate::handlers::CliOutput;

/// Version information for `aura version`.
#[must_use]
pub fn version_output() -> CliOutput {
    let mut output = CliOutput::new();
    output.kv("Version", format!("aura {}", env!("CARGO_PKG_VERSION")));
    output.kv("Package", env!("CARGO_PKG_NAME"));
    output.kv("Description", env!("CARGO_PKG_DESCRIPTION"));
    output.kv("Repository", env!("CARGO_PKG_REPOSITORY"));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_output_names_the_package_version() {
        let output = version_output();
        assert_eq!(
            output.stdout_lines()[0],
            format!("Version: aura {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            output.to_json()["sections"][0]["fields"]["Package"],
            env!("CARGO_PKG_NAME")
        );
    }
}
