//! Capability-based guards for effect system
//!
//! This module implements the non-interference property from the formal specification:
//! "For any effect `e` guarded by capability predicate `Γ ⊢ e : allowed`,
//! executing `e` from `caps = C` is only permitted if `C ⊓ need(e) = need(e)`."

use async_trait::async_trait;
use aura_core::{
    semilattice::{JoinSemilattice, MeetSemiLattice},
    Cap, Fact, Journal, MessageContext,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// Errors that can occur during capability checking
#[derive(Debug, Error, Clone, PartialEq)]
pub enum CapabilityError {
    /// Effect requires capabilities not possessed by the current context
    #[error("Insufficient capabilities: need {required:?}, have {available:?}")]
    InsufficientCapabilities {
        required: Vec<String>,
        available: Vec<String>,
    },

    /// Effect is not authorized for the current context
    #[error("Effect not authorized in context {context}")]
    NotAuthorized { context: String },

    /// Capability system error
    #[error("Capability system error: {reason}")]
    SystemError { reason: String },

    /// Journal access denied
    #[error("Journal access denied for operation: {operation}")]
    JournalAccessDenied { operation: String },
}

/// Result type for capability operations
pub type CapabilityResult<T> = std::result::Result<T, CapabilityError>;

/// Represents the capability requirements for an effect
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectRequirement {
    /// Specific permissions needed
    pub permissions: BTreeSet<String>,
    /// Resources the effect operates on
    pub resources: BTreeSet<String>,
    /// Minimum authentication level required
    pub min_auth_level: AuthLevel,
    /// Whether this effect modifies state
    pub modifies_state: bool,
}

impl EffectRequirement {
    /// Create a new effect requirement
    pub fn new() -> Self {
        Self {
            permissions: BTreeSet::new(),
            resources: BTreeSet::new(),
            min_auth_level: AuthLevel::None,
            modifies_state: false,
        }
    }

    /// Add a required permission
    pub fn require_permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.insert(permission.into());
        self
    }

    /// Add a required resource
    pub fn require_resource(mut self, resource: impl Into<String>) -> Self {
        self.resources.insert(resource.into());
        self
    }

    /// Set minimum authentication level
    pub fn require_auth_level(mut self, level: AuthLevel) -> Self {
        self.min_auth_level = level;
        self
    }

    /// Mark as state-modifying
    pub fn modifies_state(mut self) -> Self {
        self.modifies_state = true;
        self
    }

    /// Check if this requirement is satisfied by the given capabilities
    pub fn is_satisfied_by(&self, caps: &Cap) -> bool {
        // Check permissions - all required permissions must be present
        for permission in &self.permissions {
            if !caps.allows(permission) {
                return false;
            }
        }

        // Check resources - capabilities must apply to required resources
        for resource in &self.resources {
            if !caps.applies_to(resource) {
                return false;
            }
        }

        // TODO: Check authentication level when implemented

        true
    }
}

impl Default for EffectRequirement {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication levels for capability requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuthLevel {
    /// No authentication required
    None = 0,
    /// Basic device authentication
    Device = 1,
    /// Multi-factor authentication
    MultiFactor = 2,
    /// Threshold signature required
    Threshold = 3,
}

/// Trait for effects that require capability checking
#[async_trait]
pub trait GuardedEffect {
    /// Get the capability requirements for this effect
    fn capability_requirements(&self) -> EffectRequirement;

    /// Get the operation name for logging/auditing
    fn operation_name(&self) -> &'static str;

    /// Execute the effect after capability checking passes
    async fn execute_guarded(&self, context: &GuardedContext) -> CapabilityResult<()>;
}

/// Context for executing guarded effects
#[derive(Debug, Clone)]
pub struct GuardedContext {
    /// Current journal state with facts and capabilities
    pub journal: Journal,
    /// Message context for privacy partitioning
    pub message_context: MessageContext,
    /// Current timestamp for time-based capabilities
    pub timestamp: u64,
    /// Authentication level achieved
    pub auth_level: AuthLevel,
}

impl GuardedContext {
    /// Create a new guarded context
    pub fn new(
        journal: Journal,
        message_context: MessageContext,
        timestamp: u64,
        auth_level: AuthLevel,
    ) -> Self {
        Self {
            journal,
            message_context,
            timestamp,
            auth_level,
        }
    }

    /// Check if the context has sufficient capabilities for a requirement
    pub fn satisfies_requirement(&self, requirement: &EffectRequirement) -> CapabilityResult<()> {
        // Check capability satisfaction
        if !requirement.is_satisfied_by(&self.journal.caps) {
            return Err(CapabilityError::InsufficientCapabilities {
                required: requirement.permissions.iter().cloned().collect(),
                available: self.journal.caps.permissions().iter().cloned().collect(),
            });
        }

        // Check time-based validity
        if !self.journal.caps.is_valid_at(self.timestamp) {
            return Err(CapabilityError::NotAuthorized {
                context: "capabilities expired".to_string(),
            });
        }

        // Check authentication level
        if self.auth_level < requirement.min_auth_level {
            return Err(CapabilityError::InsufficientCapabilities {
                required: vec![format!("auth_level:{:?}", requirement.min_auth_level)],
                available: vec![format!("auth_level:{:?}", self.auth_level)],
            });
        }

        Ok(())
    }

    /// Create a restricted context with reduced capabilities (⊓ operation)
    pub fn restrict_capabilities(&self, constraint: Cap) -> Self {
        let mut restricted = self.clone();
        restricted.journal.caps = self.journal.caps.meet(&constraint);
        restricted
    }

    /// Merge facts into the journal (⊔ operation)
    pub fn merge_facts(&mut self, facts: Fact) {
        self.journal.facts = self.journal.facts.join(&facts);
    }

    /// Check authorization for a specific operation
    pub fn is_authorized(&self, permission: &str, resource: &str) -> bool {
        self.journal
            .is_authorized(permission, resource, self.timestamp)
    }
}

/// Capability guard that enforces non-interference property
pub struct CapabilityGuard {
    /// Whether to enforce capability checking (can be disabled for testing)
    enforce_capabilities: bool,
    /// Audit log for capability checks
    audit_log: Vec<CapabilityAuditEntry>,
}

impl CapabilityGuard {
    /// Create a new capability guard with enforcement enabled
    pub fn new() -> Self {
        Self {
            enforce_capabilities: true,
            audit_log: Vec::new(),
        }
    }

    /// Create a capability guard for testing (enforcement disabled)
    pub fn for_testing() -> Self {
        Self {
            enforce_capabilities: false,
            audit_log: Vec::new(),
        }
    }

    /// Execute a guarded effect with capability checking
    pub async fn execute_guarded_effect<E: GuardedEffect>(
        &mut self,
        effect: &E,
        context: &GuardedContext,
    ) -> CapabilityResult<()> {
        let requirement = effect.capability_requirements();
        let operation = effect.operation_name();

        // Create audit entry
        let audit_entry = CapabilityAuditEntry {
            operation: operation.to_string(),
            timestamp: context.timestamp,
            message_context: context.message_context.clone(),
            requirement: requirement.clone(),
            available_permissions: context.journal.caps.permissions().clone(),
            result: CapabilityCheckResult::Pending,
        };

        // Check capabilities if enforcement is enabled
        if self.enforce_capabilities {
            match context.satisfies_requirement(&requirement) {
                Ok(()) => {
                    // Execute the effect
                    let result = effect.execute_guarded(context).await;

                    // Update audit log
                    let final_audit = CapabilityAuditEntry {
                        result: match &result {
                            Ok(()) => CapabilityCheckResult::Allowed,
                            Err(e) => CapabilityCheckResult::Denied(e.to_string()),
                        },
                        ..audit_entry
                    };
                    self.audit_log.push(final_audit);

                    result
                }
                Err(e) => {
                    // Capability check failed
                    let denied_audit = CapabilityAuditEntry {
                        result: CapabilityCheckResult::Denied(e.to_string()),
                        ..audit_entry
                    };
                    self.audit_log.push(denied_audit);
                    Err(e)
                }
            }
        } else {
            // Enforcement disabled - just execute
            let result = effect.execute_guarded(context).await;
            let bypass_audit = CapabilityAuditEntry {
                result: CapabilityCheckResult::Bypassed,
                ..audit_entry
            };
            self.audit_log.push(bypass_audit);
            result
        }
    }

    /// Get the audit log for review
    pub fn audit_log(&self) -> &[CapabilityAuditEntry] {
        &self.audit_log
    }

    /// Clear the audit log
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    /// Enable or disable capability enforcement
    pub fn set_enforcement(&mut self, enforce: bool) {
        self.enforce_capabilities = enforce;
    }
}

impl Default for CapabilityGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Audit entry for capability checks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityAuditEntry {
    /// Operation that was checked
    pub operation: String,
    /// When the check occurred
    pub timestamp: u64,
    /// Context the operation was attempted in
    pub message_context: MessageContext,
    /// What was required
    pub requirement: EffectRequirement,
    /// What capabilities were available
    pub available_permissions: BTreeSet<String>,
    /// Result of the check
    pub result: CapabilityCheckResult,
}

/// Result of a capability check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityCheckResult {
    /// Check is in progress
    Pending,
    /// Operation was allowed
    Allowed,
    /// Operation was denied
    Denied(String),
    /// Enforcement was bypassed (testing mode)
    Bypassed,
}

/// Common effect requirements for journal operations
pub struct JournalRequirements;

impl JournalRequirements {
    /// Requirements for reading facts
    pub fn read_facts() -> EffectRequirement {
        EffectRequirement::new()
            .require_permission("journal:read")
            .require_resource("facts")
            .require_auth_level(AuthLevel::Device)
    }

    /// Requirements for merging facts
    pub fn merge_facts() -> EffectRequirement {
        EffectRequirement::new()
            .require_permission("journal:write")
            .require_resource("facts")
            .require_auth_level(AuthLevel::Device)
            .modifies_state()
    }

    /// Requirements for reading capabilities
    pub fn read_caps() -> EffectRequirement {
        EffectRequirement::new()
            .require_permission("journal:read")
            .require_resource("capabilities")
            .require_auth_level(AuthLevel::Device)
    }

    /// Requirements for refining capabilities
    pub fn refine_caps() -> EffectRequirement {
        EffectRequirement::new()
            .require_permission("journal:admin")
            .require_resource("capabilities")
            .require_auth_level(AuthLevel::MultiFactor)
            .modifies_state()
    }

    /// Requirements for administrative operations
    pub fn admin_operation() -> EffectRequirement {
        EffectRequirement::new()
            .require_permission("journal:admin")
            .require_resource("*")
            .require_auth_level(AuthLevel::Threshold)
            .modifies_state()
    }
}

/// Macro to define guarded effects easily
#[macro_export]
macro_rules! define_guarded_effect {
    ($name:ident, $op_name:expr, $requirement:expr, $execute:expr) => {
        pub struct $name;

        #[async_trait::async_trait]
        impl GuardedEffect for $name {
            fn capability_requirements(&self) -> EffectRequirement {
                $requirement
            }

            fn operation_name(&self) -> &'static str {
                $op_name
            }

            async fn execute_guarded(&self, context: &GuardedContext) -> CapabilityResult<()> {
                $execute(context).await
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::FactValue;

    // Test effect implementation
    struct TestReadEffect;

    #[async_trait]
    impl GuardedEffect for TestReadEffect {
        fn capability_requirements(&self) -> EffectRequirement {
            JournalRequirements::read_facts()
        }

        fn operation_name(&self) -> &'static str {
            "test_read"
        }

        async fn execute_guarded(&self, _context: &GuardedContext) -> CapabilityResult<()> {
            // Simulate reading facts
            Ok(())
        }
    }

    struct TestWriteEffect;

    #[async_trait]
    impl GuardedEffect for TestWriteEffect {
        fn capability_requirements(&self) -> EffectRequirement {
            JournalRequirements::merge_facts()
        }

        fn operation_name(&self) -> &'static str {
            "test_write"
        }

        async fn execute_guarded(&self, _context: &GuardedContext) -> CapabilityResult<()> {
            // Simulate writing facts
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_capability_guard_allows_sufficient_caps() {
        let mut guard = CapabilityGuard::new();

        // Create context with read permission
        let caps = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
        ]);
        let journal = Journal::with_caps(caps);
        let context = GuardedContext::new(
            journal,
            MessageContext::dkd_context("test", [0u8; 32]),
            1000,
            AuthLevel::Device,
        );

        // Should allow read effect
        let read_effect = TestReadEffect;
        let result = guard.execute_guarded_effect(&read_effect, &context).await;
        assert!(result.is_ok());

        // Should allow write effect
        let write_effect = TestWriteEffect;
        let result = guard.execute_guarded_effect(&write_effect, &context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_capability_guard_denies_insufficient_caps() {
        let mut guard = CapabilityGuard::new();

        // Create context with only read permission
        let caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let journal = Journal::with_caps(caps);
        let context = GuardedContext::new(
            journal,
            MessageContext::dkd_context("test", [0u8; 32]),
            1000,
            AuthLevel::Device,
        );

        // Should allow read effect
        let read_effect = TestReadEffect;
        let result = guard.execute_guarded_effect(&read_effect, &context).await;
        assert!(result.is_ok());

        // Should deny write effect (insufficient permissions)
        let write_effect = TestWriteEffect;
        let result = guard.execute_guarded_effect(&write_effect, &context).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CapabilityError::InsufficientCapabilities { .. }
        ));
    }

    #[tokio::test]
    async fn test_capability_restriction() {
        let caps = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
            "journal:admin".to_string(),
        ]);
        let journal = Journal::with_caps(caps);
        let context = GuardedContext::new(
            journal,
            MessageContext::dkd_context("test", [0u8; 32]),
            1000,
            AuthLevel::Device,
        );

        // Restrict to read-only
        let readonly_constraint = Cap::with_permissions(vec!["journal:read".to_string()]);
        let restricted_context = context.restrict_capabilities(readonly_constraint);

        // Should still allow read
        assert!(restricted_context.is_authorized("journal:read", "*"));

        // Should not allow write or admin
        assert!(!restricted_context.is_authorized("journal:write", "*"));
        assert!(!restricted_context.is_authorized("journal:admin", "*"));
    }

    #[tokio::test]
    async fn test_fact_merging() {
        let caps = Cap::with_permissions(vec!["journal:write".to_string()]);
        let journal = Journal::with_caps(caps);
        let mut context = GuardedContext::new(
            journal,
            MessageContext::dkd_context("test", [0u8; 32]),
            1000,
            AuthLevel::Device,
        );

        // Initial state
        assert!(context.journal.facts.is_empty());

        // Merge some facts
        let new_facts = Fact::with_value("test_key", FactValue::String("test_value".to_string()));
        context.merge_facts(new_facts);

        // Should have the new facts
        assert!(!context.journal.facts.is_empty());
        assert!(context.journal.facts.contains_key("test_key"));
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let mut guard = CapabilityGuard::new();

        let caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let journal = Journal::with_caps(caps);
        let context = GuardedContext::new(
            journal,
            MessageContext::dkd_context("test", [0u8; 32]),
            1000,
            AuthLevel::Device,
        );

        // Execute some operations
        let read_effect = TestReadEffect;
        let _ = guard.execute_guarded_effect(&read_effect, &context).await;

        let write_effect = TestWriteEffect;
        let _ = guard.execute_guarded_effect(&write_effect, &context).await;

        // Check audit log
        let audit_log = guard.audit_log();
        assert_eq!(audit_log.len(), 2);

        // First operation should succeed
        assert!(matches!(
            audit_log[0].result,
            CapabilityCheckResult::Allowed
        ));

        // Second operation should fail
        assert!(matches!(
            audit_log[1].result,
            CapabilityCheckResult::Denied(_)
        ));
    }
}
