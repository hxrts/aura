use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const TOOL_API_VERSIONS: [&str; 3] = ["1.0", "0.2", "0.1"];
pub const TOOL_API_DEFAULT_VERSION: &str = TOOL_API_VERSIONS[0];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NegotiationRequest {
    pub client_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NegotiationResult {
    pub negotiated_version: String,
    pub supported_versions: Vec<String>,
}

pub fn negotiate(client_versions: &[String]) -> Result<NegotiationResult> {
    for supported in TOOL_API_VERSIONS {
        if client_versions.iter().any(|client| client == supported) {
            return Ok(NegotiationResult {
                negotiated_version: supported.to_string(),
                supported_versions: TOOL_API_VERSIONS.iter().map(ToString::to_string).collect(),
            });
        }
    }

    bail!(
        "no compatible tool api version found. client_versions={client_versions:?} supported_versions={TOOL_API_VERSIONS:?}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_selects_highest_shared_version() {
        let client_versions = vec!["0.1".to_string(), "1.0".to_string()];
        let result = match negotiate(&client_versions) {
            Ok(result) => result,
            Err(error) => panic!("negotiation failed: {error}"),
        };
        assert_eq!(result.negotiated_version, "1.0");
    }

    #[test]
    fn negotiation_rejects_unsupported_versions() {
        let client_versions = vec!["9.9".to_string(), "8.1".to_string()];
        let error = match negotiate(&client_versions) {
            Ok(_) => panic!("negotiation should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("no compatible tool api version"));
    }
}
