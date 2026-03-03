//! Policy registry for fact type to policy mappings.
//!
//! This module provides a registry that maps fact types to their
//! delivery policies, allowing app-level control over ack tracking lifecycle.
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_app::policies::{PolicyRegistry, DropWhenFullyAcked, DropWhenFinalized};
//!
//! let mut registry = PolicyRegistry::new();
//!
//! // Register policies for specific fact types
//! registry.register("MessageSent", DropWhenFullyAcked);
//! registry.register("InvitationAccepted", DropWhenFinalized);
//!
//! // Get policy for a fact
//! if let Some(policy) = registry.get_policy("MessageSent") {
//!     if policy.should_drop_tracking(&consistency, &expected) {
//!         // Drop ack tracking
//!     }
//! }
//! ```

use super::{boxed, BoxedPolicy, DeliveryPolicy, DropWhenFinalized};
use aura_core::types::facts::FactTypeId;
use std::any::TypeId;
use std::collections::HashMap;

// =============================================================================
// String-Based Policy Registry
// =============================================================================

/// Registry for mapping fact type names to delivery policies.
///
/// Use this for runtime registration where fact types are identified by string.
#[derive(Clone)]
pub struct PolicyRegistry {
    policies: HashMap<FactTypeId, BoxedPolicy>,
    default_policy: BoxedPolicy,
}

impl Default for PolicyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyRegistry {
    /// Create a new registry with default policy
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            default_policy: boxed(DropWhenFinalized),
        }
    }

    /// Create a new registry with a custom default policy
    pub fn with_default<P: DeliveryPolicy + 'static>(default: P) -> Self {
        Self {
            policies: HashMap::new(),
            default_policy: boxed(default),
        }
    }

    /// Register a policy for a fact type
    pub fn register<P: DeliveryPolicy + 'static>(&mut self, fact_type: &FactTypeId, policy: P) {
        self.policies.insert(fact_type.clone(), boxed(policy));
    }

    /// Register a boxed policy for a fact type
    pub fn register_boxed(&mut self, fact_type: &FactTypeId, policy: BoxedPolicy) {
        self.policies.insert(fact_type.clone(), policy);
    }

    /// Get the policy for a fact type
    ///
    /// Returns the registered policy, or the default if not registered.
    pub fn get_policy(&self, fact_type: &FactTypeId) -> &BoxedPolicy {
        self.policies.get(fact_type).unwrap_or(&self.default_policy)
    }

    /// Check if a policy is registered for a fact type
    pub fn has_policy(&self, fact_type: &FactTypeId) -> bool {
        self.policies.contains_key(fact_type)
    }

    /// Remove a policy registration
    pub fn unregister(&mut self, fact_type: &FactTypeId) -> Option<BoxedPolicy> {
        self.policies.remove(fact_type)
    }

    /// Compatibility shim for string fact type registration.
    pub fn register_str<P: DeliveryPolicy + 'static>(&mut self, fact_type: &str, policy: P) {
        let typed = FactTypeId::from(fact_type);
        self.register(&typed, policy);
    }

    /// Compatibility shim for string fact type lookup.
    pub fn get_policy_str(&self, fact_type: &str) -> &BoxedPolicy {
        let typed = FactTypeId::from(fact_type);
        self.get_policy(&typed)
    }

    /// Compatibility shim for string fact type checks.
    pub fn has_policy_str(&self, fact_type: &str) -> bool {
        let typed = FactTypeId::from(fact_type);
        self.has_policy(&typed)
    }

    /// Compatibility shim for string fact type unregistration.
    pub fn unregister_str(&mut self, fact_type: &str) -> Option<BoxedPolicy> {
        let typed = FactTypeId::from(fact_type);
        self.unregister(&typed)
    }

    /// Get the default policy
    pub fn default_policy(&self) -> &BoxedPolicy {
        &self.default_policy
    }

    /// Set a new default policy
    pub fn set_default<P: DeliveryPolicy + 'static>(&mut self, policy: P) {
        self.default_policy = boxed(policy);
    }

    /// Get the number of registered policies
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// Iterate over registered fact types
    pub fn fact_types(&self) -> impl Iterator<Item = &FactTypeId> {
        self.policies.keys()
    }
}

impl std::fmt::Debug for PolicyRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyRegistry")
            .field("registered_count", &self.policies.len())
            .field(
                "fact_types",
                &self
                    .policies
                    .keys()
                    .map(FactTypeId::as_str)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

// =============================================================================
// Type-Based Policy Registry
// =============================================================================

/// Registry for mapping Rust types to delivery policies.
///
/// Use this for compile-time type-safe policy registration.
#[derive(Clone)]
pub struct TypedPolicyRegistry {
    policies: HashMap<TypeId, BoxedPolicy>,
    default_policy: BoxedPolicy,
}

impl Default for TypedPolicyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TypedPolicyRegistry {
    /// Create a new typed registry with default policy
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            default_policy: boxed(DropWhenFinalized),
        }
    }

    /// Create a new typed registry with a custom default policy
    pub fn with_default<P: DeliveryPolicy + 'static>(default: P) -> Self {
        Self {
            policies: HashMap::new(),
            default_policy: boxed(default),
        }
    }

    /// Register a policy for a fact type (generic over fact type)
    pub fn register<F: 'static, P: DeliveryPolicy + 'static>(&mut self, policy: P) {
        self.policies.insert(TypeId::of::<F>(), boxed(policy));
    }

    /// Get the policy for a fact type
    pub fn get_policy<F: 'static>(&self) -> &BoxedPolicy {
        self.policies
            .get(&TypeId::of::<F>())
            .unwrap_or(&self.default_policy)
    }

    /// Check if a policy is registered for a fact type
    pub fn has_policy<F: 'static>(&self) -> bool {
        self.policies.contains_key(&TypeId::of::<F>())
    }

    /// Remove a policy registration
    pub fn unregister<F: 'static>(&mut self) -> Option<BoxedPolicy> {
        self.policies.remove(&TypeId::of::<F>())
    }

    /// Get the default policy
    pub fn default_policy(&self) -> &BoxedPolicy {
        &self.default_policy
    }

    /// Get the number of registered policies
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }
}

impl std::fmt::Debug for TypedPolicyRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedPolicyRegistry")
            .field("registered_count", &self.policies.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::{DropWhenFinalizedAndFullyAcked, DropWhenFullyAcked};

    // Dummy fact types for testing
    struct MessageSent;
    struct InvitationAccepted;

    #[test]
    fn test_policy_registry_register_and_get() {
        let mut registry = PolicyRegistry::new();
        let message_sent = FactTypeId::from("MessageSent");
        let invitation_accepted = FactTypeId::from("InvitationAccepted");
        let unknown = FactTypeId::from("Unknown");

        registry.register(&message_sent, DropWhenFullyAcked);
        registry.register(&invitation_accepted, DropWhenFinalized);

        assert!(registry.has_policy(&message_sent));
        assert!(registry.has_policy(&invitation_accepted));
        assert!(!registry.has_policy(&unknown));

        let policy = registry.get_policy(&message_sent);
        assert_eq!(policy.name(), "DropWhenFullyAcked");

        let policy = registry.get_policy(&invitation_accepted);
        assert_eq!(policy.name(), "DropWhenFinalized");

        // Unknown gets default
        let policy = registry.get_policy(&unknown);
        assert_eq!(policy.name(), "DropWhenFinalized");
    }

    #[test]
    fn test_policy_registry_custom_default() {
        let registry = PolicyRegistry::with_default(DropWhenFullyAcked);
        let unknown = FactTypeId::from("Unknown");

        let policy = registry.get_policy(&unknown);
        assert_eq!(policy.name(), "DropWhenFullyAcked");
    }

    #[test]
    fn test_policy_registry_unregister() {
        let mut registry = PolicyRegistry::new();
        let message_sent = FactTypeId::from("MessageSent");

        registry.register(&message_sent, DropWhenFullyAcked);
        assert!(registry.has_policy(&message_sent));

        registry.unregister(&message_sent);
        assert!(!registry.has_policy(&message_sent));
    }

    #[test]
    fn test_typed_policy_registry() {
        let mut registry = TypedPolicyRegistry::new();

        registry.register::<MessageSent, _>(DropWhenFullyAcked);
        registry.register::<InvitationAccepted, _>(DropWhenFinalizedAndFullyAcked);

        assert!(registry.has_policy::<MessageSent>());
        assert!(registry.has_policy::<InvitationAccepted>());

        let policy = registry.get_policy::<MessageSent>();
        assert_eq!(policy.name(), "DropWhenFullyAcked");

        let policy = registry.get_policy::<InvitationAccepted>();
        assert_eq!(policy.name(), "DropWhenFinalizedAndFullyAcked");
    }

    #[test]
    fn test_registry_debug() {
        let mut registry = PolicyRegistry::new();
        let foo = FactTypeId::from("Foo");
        let bar = FactTypeId::from("Bar");
        registry.register(&foo, DropWhenFinalized);
        registry.register(&bar, DropWhenFullyAcked);

        let debug = format!("{registry:?}");
        assert!(debug.contains("PolicyRegistry"));
        assert!(debug.contains("2")); // registered_count
    }
}
