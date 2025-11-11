//! Capability Guard System
//!
//! This module provides capability-based authorization guards for choreographic protocols.
//! Guards enable compile-time and runtime verification that protocol participants have
//! the necessary capabilities before proceeding with operations.
//!
//! # Syntax
//!
//! Guards use the syntax: `[guard: need(m) ≤ caps]` where:
//! - `need(m)` is the capability requirement for message m
//! - `caps` is the participant's current capability set
//! - `≤` is the meet-semilattice ordering (refinement relation)
//!
//! # Examples
//!
//! ```ignore
//! // Protocol with capability guards
//! choreography! {
//!     Alice[guard: need(admin_request) ≤ caps] -> Bob: AdminRequest;
//!     Bob -> Alice: AdminResponse;
//! }
//! ```

use aura_core::{AuraError, AuraResult, Cap};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Capability guard definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGuard {
    /// The capability requirement
    pub required: Cap,
    /// Optional description of what this guard protects
    pub description: Option<String>,
}

impl CapabilityGuard {
    /// Create a new capability guard
    pub fn new(required: Cap) -> Self {
        Self {
            required,
            description: None,
        }
    }

    /// Create a guard with description
    pub fn with_description(required: Cap, description: impl Into<String>) -> Self {
        Self {
            required,
            description: Some(description.into()),
        }
    }

    /// Check if the given capabilities satisfy this guard
    pub fn check(&self, capabilities: &Cap) -> bool {
        // Check if required ≤ capabilities using meet-semilattice ordering
        // This means: capabilities.meet(required) == required
        use aura_core::MeetSemiLattice;
        capabilities.meet(&self.required) == self.required
    }

    /// Enforce this guard, returning error if check fails
    pub fn enforce(&self, capabilities: &Cap) -> AuraResult<()> {
        if self.check(capabilities) {
            Ok(())
        } else {
            let msg = match &self.description {
                Some(desc) => format!("Guard failed: {} (required: {:?})", desc, self.required),
                None => format!("Guard failed (required: {:?})", self.required),
            };
            Err(AuraError::permission_denied(msg))
        }
    }
}

/// Protocol with capability guards
pub trait GuardedProtocol {
    /// Get all guards defined in this protocol
    fn guards(&self) -> &HashMap<String, CapabilityGuard>;

    /// Check all guards against given capabilities
    fn check_all_guards(&self, capabilities: &Cap) -> Vec<String> {
        let mut failed = Vec::new();
        for (name, guard) in self.guards() {
            if !guard.check(capabilities) {
                failed.push(name.clone());
            }
        }
        failed
    }

    /// Enforce all guards
    fn enforce_all_guards(&self, capabilities: &Cap) -> AuraResult<()> {
        let failed = self.check_all_guards(capabilities);
        if failed.is_empty() {
            Ok(())
        } else {
            Err(AuraError::permission_denied(format!(
                "Guards failed: {}",
                failed.join(", ")
            )))
        }
    }
}

/// Guard syntax parsing and compilation
pub struct GuardSyntax;

impl GuardSyntax {
    /// Parse a guard expression from string
    /// Format: "guard: need(requirement) ≤ caps"
    pub fn parse(expr: &str) -> AuraResult<CapabilityGuard> {
        let trimmed = expr.trim();
        if !trimmed.starts_with("guard:") {
            return Err(AuraError::invalid(
                "Guard expression must start with 'guard:'",
            ));
        }

        let remainder = trimmed
            .strip_prefix("guard:")
            .expect("already checked with starts_with")
            .trim();

        // Allow inline descriptions after `//`
        let (core_expr, description) = match remainder.split_once("//") {
            Some((core, desc)) => (core.trim(), Some(desc.trim().to_string())),
            None => (remainder, None),
        };

        let need_prefix = "need(";
        if !core_expr.starts_with(need_prefix) {
            return Err(AuraError::invalid(
                "Guard expression must include 'need(<permissions>)'",
            ));
        }

        let after_need = &core_expr[need_prefix.len()..];
        let close_idx = after_need.find(')').ok_or_else(|| {
            AuraError::invalid("Guard expression is missing closing ')' for need(...) block")
        })?;

        let requirement_block = &after_need[..close_idx];
        let mut permissions = Vec::new();
        for token in requirement_block.split(|c| c == ',' || c == '|' || c == '&') {
            let token = token.trim();
            if !token.is_empty() {
                permissions.push(token.to_string());
            }
        }

        if permissions.is_empty() {
            return Err(AuraError::invalid(
                "Guard expression must list at least one requirement inside need(...)",
            ));
        }

        let mut comparator_section = after_need[close_idx + 1..].trim_start();
        // Accept either unicode ≤ or ASCII <= / =<
        let consumed = if comparator_section.starts_with('≤') {
            Some('≤'.len_utf8())
        } else if comparator_section.starts_with("<=") {
            Some(2)
        } else if comparator_section.starts_with("=<") {
            Some(2)
        } else {
            None
        };

        let consumed = consumed.ok_or_else(|| {
            AuraError::invalid("Guard expression must include '≤ caps' comparator after need(...)")
        })?;

        comparator_section = comparator_section[consumed..].trim_start();

        // Expect a capability identifier such as `caps`, `caps_Alice`, etc.
        // We only validate that it starts with `caps`.
        if !comparator_section.starts_with("caps") {
            return Err(AuraError::invalid(
                "Guard expression must compare against a capability identifier starting with 'caps'",
            ));
        }

        // Remove identifier token
        let rest = comparator_section["caps".len()..]
            .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.');

        let description = description
            .or_else(|| {
                let trimmed = rest.trim();
                if trimmed.starts_with("desc:") {
                    Some(
                        trimmed
                            .trim_start_matches("desc:")
                            .trim()
                            .trim_matches('"')
                            .to_string(),
                    )
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty());

        // Build the capability requirement
        let mut cap = Cap::new();
        for permission in permissions {
            cap.add_permission(permission);
        }

        Ok(match description {
            Some(desc) => CapabilityGuard::with_description(cap, desc),
            None => CapabilityGuard::new(cap),
        })
    }

    /// Compile guard syntax to runtime guard
    pub fn compile(expr: &str) -> AuraResult<CapabilityGuard> {
        Self::parse(expr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::Cap;

    #[test]
    fn test_capability_guard_creation() {
        let cap = Cap::top(); // Most permissive capability
        let guard = CapabilityGuard::new(cap.clone());
        assert_eq!(guard.required, cap);
    }

    #[test]
    fn test_guard_check_success() {
        // Create a guard that requires a specific capability
        let required = Cap::top();
        let guard = CapabilityGuard::new(required.clone());

        // Check with same capability should succeed
        assert!(guard.check(&required));
    }

    #[test]
    fn test_guard_enforcement() {
        let required = Cap::top();
        let guard = CapabilityGuard::new(required.clone());

        // Should succeed with sufficient capabilities
        assert!(guard.enforce(&required).is_ok());

        // Test with more restrictive capabilities would fail in real implementation
        // This is a placeholder test
    }

    #[test]
    fn test_guard_with_description() {
        let cap = Cap::top();
        let guard = CapabilityGuard::with_description(cap, "Admin access required");
        assert!(guard.description.is_some());
        assert_eq!(guard.description.unwrap(), "Admin access required");
    }

    #[test]
    fn parse_simple_guard_expression() {
        let guard =
            GuardSyntax::parse("guard: need(admin_request) <= caps_Admin").expect("parse ok");
        assert!(guard.description.is_none());
        assert!(guard.required.allows("admin_request"));
    }

    #[test]
    fn parse_guard_with_description_comment() {
        let guard = GuardSyntax::parse(
            "guard: need(tree_modify | tree_vote) ≤ caps_Tree // tree maintenance",
        )
        .expect("parse ok");
        assert_eq!(guard.description.as_deref(), Some("tree maintenance"));
        assert!(guard.required.allows("tree_modify"));
        assert!(guard.required.allows("tree_vote"));
    }

    #[test]
    fn parse_guard_rejects_missing_need() {
        let err = GuardSyntax::parse("guard: caps").unwrap_err();
        assert!(format!("{}", err).contains("need("));
    }
}
