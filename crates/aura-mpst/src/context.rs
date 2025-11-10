//! Context Isolation Enforcement
//!
//! This module provides context isolation mechanisms for choreographic protocols.
//! Context isolation ensures that information cannot flow between different
//! privacy contexts, maintaining unlinkability properties.
//!
//! # Context Types
//!
//! Aura uses three main context types:
//! - **RID (Relationship ID)**: Isolates different relationships
//! - **GID (Group ID)**: Isolates different group memberships
//! - **DKD (Derived Key Domain)**: Isolates cryptographic operations
//!
//! # Isolation Rules
//!
//! 1. **Context Barriers**: Operations cannot cross context boundaries
//! 2. **Unlinkability**: `τ[c1↔c2] ≈_ext τ` (computationally indistinguishable)
//! 3. **Information Flow**: `κ₁ ≠ κ₂` prevents cross-context flow

use aura_core::{AuraError, AuraResult, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

/// Context identifier types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContextType {
    /// Relationship context (RID)
    Relationship(Uuid),
    /// Group context (GID)
    Group(Uuid),
    /// Key derivation context (DKD)
    KeyDerivation(Uuid),
    /// Session context
    Session(SessionId),
    /// Custom context type
    Custom(String, Uuid),
}

impl std::fmt::Display for ContextType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextType::Relationship(id) => write!(f, "RID:{}", id),
            ContextType::Group(id) => write!(f, "GID:{}", id),
            ContextType::KeyDerivation(id) => write!(f, "DKD:{}", id),
            ContextType::Session(id) => write!(f, "SID:{}", id),
            ContextType::Custom(name, id) => write!(f, "{}:{}", name, id),
        }
    }
}

impl ContextType {
    /// Create a new relationship context
    #[allow(clippy::disallowed_methods)]
    pub fn new_relationship() -> Self {
        ContextType::Relationship(Uuid::new_v4())
    }

    /// Create a new group context
    #[allow(clippy::disallowed_methods)]
    pub fn new_group() -> Self {
        ContextType::Group(Uuid::new_v4())
    }

    /// Create a new key derivation context
    #[allow(clippy::disallowed_methods)]
    pub fn new_key_derivation() -> Self {
        ContextType::KeyDerivation(Uuid::new_v4())
    }

    /// Create a new session context
    pub fn new_session() -> Self {
        ContextType::Session(SessionId::new())
    }

    /// Create a custom context
    #[allow(clippy::disallowed_methods)]
    pub fn custom(name: impl Into<String>) -> Self {
        ContextType::Custom(name.into(), Uuid::new_v4())
    }

    /// Get the context UUID
    pub fn id(&self) -> Uuid {
        match self {
            ContextType::Relationship(id) => *id,
            ContextType::Group(id) => *id,
            ContextType::KeyDerivation(id) => *id,
            ContextType::Session(session_id) => session_id.0, // Assuming SessionId wraps Uuid
            ContextType::Custom(_, id) => *id,
        }
    }

    /// Check if two contexts are of the same type (ignoring ID)
    pub fn same_type(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// Context barrier enforcing isolation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBarrier {
    /// Contexts that are isolated by this barrier
    pub isolated_contexts: HashSet<ContextType>,
    /// Description of what this barrier protects
    pub description: String,
    /// Whether the barrier is active
    pub active: bool,
}

impl ContextBarrier {
    /// Create a new context barrier
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            isolated_contexts: HashSet::new(),
            description: description.into(),
            active: true,
        }
    }

    /// Add a context to the isolation set
    pub fn isolate(mut self, context: ContextType) -> Self {
        self.isolated_contexts.insert(context);
        self
    }

    /// Check if information flow between contexts is allowed
    pub fn allows_flow(&self, from: &ContextType, to: &ContextType) -> bool {
        if !self.active {
            return true;
        }

        // Same context always allows flow
        if from == to {
            return true;
        }

        // If either context is isolated, flow is not allowed
        !(self.isolated_contexts.contains(from) || self.isolated_contexts.contains(to))
    }

    /// Enforce context isolation for a flow
    pub fn enforce_isolation(&self, from: &ContextType, to: &ContextType) -> AuraResult<()> {
        if self.allows_flow(from, to) {
            Ok(())
        } else {
            Err(AuraError::permission_denied(format!(
                "Context isolation violation: {} -> {} blocked by barrier '{}'",
                from, to, self.description
            )))
        }
    }

    /// Deactivate the barrier
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Activate the barrier
    pub fn activate(&mut self) {
        self.active = true;
    }
}

/// Information flow between contexts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InformationFlow {
    /// Source context
    pub from: ContextType,
    /// Destination context
    pub to: ContextType,
    /// Type of information flowing
    pub info_type: String,
    /// Amount of information (for budget tracking)
    pub amount: u64,
    /// When the flow occurred
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl InformationFlow {
    /// Create a new information flow record
    #[allow(clippy::disallowed_methods)]
    pub fn new(
        from: ContextType,
        to: ContextType,
        info_type: impl Into<String>,
        amount: u64,
    ) -> Self {
        Self {
            from,
            to,
            info_type: info_type.into(),
            amount,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Check if this is a cross-context flow
    pub fn is_cross_context(&self) -> bool {
        self.from != self.to
    }

    /// Check if flow involves isolated contexts
    pub fn violates_barrier(&self, barrier: &ContextBarrier) -> bool {
        !barrier.allows_flow(&self.from, &self.to)
    }
}

/// Context isolation manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextIsolation {
    /// Active context barriers
    pub barriers: Vec<ContextBarrier>,
    /// Information flow log
    pub flows: Vec<InformationFlow>,
    /// Maximum flows to keep in memory
    pub max_flows: usize,
    /// Current active context (if any)
    pub current_context: Option<ContextType>,
}

impl ContextIsolation {
    /// Create a new context isolation manager
    pub fn new() -> Self {
        Self {
            barriers: Vec::new(),
            flows: Vec::new(),
            max_flows: 1000,
            current_context: None,
        }
    }

    /// Add a context barrier
    pub fn add_barrier(&mut self, barrier: ContextBarrier) {
        self.barriers.push(barrier);
    }

    /// Set the current active context
    pub fn set_context(&mut self, context: ContextType) {
        self.current_context = Some(context);
    }

    /// Clear the current context
    pub fn clear_context(&mut self) {
        self.current_context = None;
    }

    /// Check if information flow is allowed
    pub fn check_flow(&self, from: &ContextType, to: &ContextType) -> AuraResult<()> {
        for barrier in &self.barriers {
            barrier.enforce_isolation(from, to)?;
        }
        Ok(())
    }

    /// Record an information flow
    pub fn record_flow(
        &mut self,
        from: ContextType,
        to: ContextType,
        info_type: impl Into<String>,
        amount: u64,
    ) -> AuraResult<()> {
        // Check if flow is allowed
        self.check_flow(&from, &to)?;

        // Record the flow
        let flow = InformationFlow::new(from, to, info_type, amount);
        self.flows.push(flow);

        // Trim flows if necessary
        if self.flows.len() > self.max_flows {
            self.flows.remove(0);
        }

        Ok(())
    }

    /// Get flows between specific contexts
    pub fn flows_between(&self, from: &ContextType, to: &ContextType) -> Vec<&InformationFlow> {
        self.flows
            .iter()
            .filter(|f| f.from == *from && f.to == *to)
            .collect()
    }

    /// Get all cross-context flows
    pub fn cross_context_flows(&self) -> Vec<&InformationFlow> {
        self.flows.iter().filter(|f| f.is_cross_context()).collect()
    }

    /// Check for context isolation violations
    pub fn check_violations(&self) -> Vec<String> {
        let mut violations = Vec::new();

        for flow in &self.flows {
            for barrier in &self.barriers {
                if flow.violates_barrier(barrier) {
                    violations.push(format!(
                        "Flow {} -> {} violates barrier '{}'",
                        flow.from, flow.to, barrier.description
                    ));
                }
            }
        }

        violations
    }

    /// Validate that context isolation is properly maintained
    pub fn validate(&self) -> AuraResult<()> {
        let violations = self.check_violations();
        if violations.is_empty() {
            Ok(())
        } else {
            Err(AuraError::permission_denied(format!(
                "Context isolation violations: {}",
                violations.join(", ")
            )))
        }
    }
}

impl Default for ContextIsolation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_type_creation() {
        let rid = ContextType::new_relationship();
        let gid = ContextType::new_group();

        assert!(matches!(rid, ContextType::Relationship(_)));
        assert!(matches!(gid, ContextType::Group(_)));
        assert!(!rid.same_type(&gid));
    }

    #[test]
    fn test_context_barrier() {
        let rid1 = ContextType::new_relationship();
        let rid2 = ContextType::new_relationship();

        let barrier = ContextBarrier::new("Test isolation").isolate(rid1.clone());

        assert!(barrier.allows_flow(&rid1, &rid1)); // Same context
        assert!(!barrier.allows_flow(&rid1, &rid2)); // Different contexts
    }

    #[test]
    fn test_context_isolation() {
        let mut isolation = ContextIsolation::new();
        let rid1 = ContextType::new_relationship();
        let rid2 = ContextType::new_relationship();

        let barrier = ContextBarrier::new("Test barrier").isolate(rid1.clone());
        isolation.add_barrier(barrier);

        // Flow should be blocked
        assert!(isolation.record_flow(rid1, rid2, "test_info", 100).is_err());
    }

    #[test]
    fn test_information_flow() {
        let rid1 = ContextType::new_relationship();
        let rid2 = ContextType::new_relationship();

        let flow = InformationFlow::new(rid1, rid2, "metadata", 50);
        assert!(flow.is_cross_context());
        assert_eq!(flow.info_type, "metadata");
        assert_eq!(flow.amount, 50);
    }

    #[test]
    fn test_context_display() {
        let rid = ContextType::new_relationship();
        assert!(rid.to_string().starts_with("RID:"));

        let custom = ContextType::custom("test");
        assert!(custom.to_string().starts_with("test:"));
    }
}
