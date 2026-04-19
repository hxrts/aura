//! Common parsing helpers for workflow inputs.
#![allow(dead_code)]
// Parsing helpers are shared across workflow modules and target-specific test
// builds; strict all-target dead-code analysis does not see every call path.

use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;

/// Parse an AuthorityId from user input.
pub fn parse_authority_id(input: &str) -> Result<AuthorityId, AuraError> {
    input
        .parse::<AuthorityId>()
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID: {input}")))
}

/// Parse a ContextId from user input.
pub fn parse_context_id(input: &str) -> Result<ContextId, AuraError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AuraError::not_found("Home context not available"));
    }

    trimmed
        .parse::<ContextId>()
        .map_err(|_| AuraError::invalid(format!("Invalid context ID: {trimmed}")))
}
