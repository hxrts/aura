//! Legacy Capability System
//!
//! This module provides the legacy capability types that are expected by the existing tests.
//! These implement meet-semilattice laws for capability restriction and delegation.
//!
//! ## Theoretical Foundation (§2.1 and §2.4)
//!
//! From the Aura theoretical model:
//! - Capabilities are meet-semilattice elements (C, ⊓, ⊤)
//! - Meet operation is associative, commutative, and idempotent
//! - Refinement operations can only reduce authority through the meet operation (monotonic restriction)
//! - The operation refine_caps(c) never increases authority
//!
//! This implementation realizes these mathematical properties through the MeetSemiLattice trait.

use aura_core::semilattice::{MeetSemiLattice, Top};
use aura_core::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Individual capability that can be granted
///
/// Represents atomic authority units in the capability lattice.
/// These form the basis elements that compose via meet operations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Read access to resources matching a pattern
    Read { resource_pattern: String },
    /// Write access to resources matching a pattern
    Write { resource_pattern: String },
    /// Execute permission for specific operations
    Execute { operation: String },
    /// Delegation permission with maximum depth
    Delegate { max_depth: u32 },
    /// All permissions - represents ⊤ (top element) in the meet-semilattice
    /// Per §2.1: "type Cap // partially ordered set (≤), with meet ⊓ and top ⊤"
    All,
    /// No permissions - semantically represents minimal authority
    /// Note: This is NOT the bottom element of meet-semilattice (which would be empty set)
    None,
}

/// Set of capabilities implementing meet-semilattice laws
///
/// Implements the mathematical structure (C, ⊓, ⊤) from §2.1 of the theoretical model.
/// This realizes "Capabilities (Meet-Semilattice)" where:
/// - x ⊓ y = y ⊓ x (commutative)
/// - x ⊓ (y ⊓ z) = (x ⊓ y) ⊓ z (associative)
/// - x ⊓ x = x (idempotent)
///
/// Per §2.4 Semantic Laws: "The operation refine_caps c never increases authority"
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    capabilities: BTreeSet<Capability>,
}

impl CapabilitySet {
    /// Create new capability set from a collection
    ///
    /// Normalizes contradictory states by removing None when All is present.
    /// This maintains logical consistency since having both "all permissions"
    /// and "no permissions" is semantically meaningless.
    ///
    /// This normalization ensures the capability set forms a valid element
    /// in the meet-semilattice by preventing contradictory states.
    pub fn from_capabilities(mut caps: BTreeSet<Capability>) -> Self {
        // Normalize contradictory states: All + None = All (most permissive wins)
        // This prevents logically invalid capability sets from being created
        if caps.contains(&Capability::All) && caps.contains(&Capability::None) {
            caps.remove(&Capability::None);
        }
        Self { capabilities: caps }
    }

    /// Create capability set from permission strings
    pub fn from_permissions(permissions: &[&str]) -> Self {
        let caps = permissions
            .iter()
            .map(|perm| {
                if perm.starts_with("read:") {
                    Capability::Read {
                        resource_pattern: perm.strip_prefix("read:").unwrap().to_string(),
                    }
                } else if perm.starts_with("write:") {
                    Capability::Write {
                        resource_pattern: perm.strip_prefix("write:").unwrap().to_string(),
                    }
                } else if perm.starts_with("execute:") {
                    Capability::Execute {
                        operation: perm.strip_prefix("execute:").unwrap().to_string(),
                    }
                } else {
                    // Default to read permission
                    Capability::Read {
                        resource_pattern: perm.to_string(),
                    }
                }
            })
            .collect();

        // Use the normalized constructor
        Self::from_capabilities(caps)
    }

    /// Create empty capability set
    pub fn empty() -> Self {
        Self {
            capabilities: BTreeSet::new(),
        }
    }

    /// Iterator over capabilities
    pub fn capabilities(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.iter()
    }

    /// Check if this capability set permits a permission
    ///
    /// Implements the capability check from §1.3 Operational Semantics:
    /// "Each side effect or message action a carries a required capability predicate need(a)"
    /// This method evaluates whether need(permission) ≤ self.capabilities
    pub fn permits(&self, permission: &str) -> bool {
        for cap in &self.capabilities {
            match cap {
                Capability::Read { resource_pattern } if permission.starts_with("read:") => {
                    let resource = permission.strip_prefix("read:").unwrap();
                    if resource_matches(resource, resource_pattern) {
                        return true;
                    }
                }
                Capability::Write { resource_pattern } if permission.starts_with("write:") => {
                    let resource = permission.strip_prefix("write:").unwrap();
                    if resource_matches(resource, resource_pattern) {
                        return true;
                    }
                }
                Capability::Execute { operation } if permission.starts_with("execute:") => {
                    let op = permission.strip_prefix("execute:").unwrap();
                    if op == operation {
                        return true;
                    }
                }
                // All capability permits everything - it's the top element ⊤
                Capability::All => return true,
                _ => continue,
            }
        }
        false
    }

    /// Check if this capability set is a subset of another
    ///
    /// In the capability lattice, a set A is a subset of B if all capabilities
    /// in A are also logically present in B. The All capability represents
    /// universal permission, so it subsumes all other capabilities.
    pub fn is_subset_of(&self, other: &Self) -> bool {
        // If other contains All, then this is always a subset
        if other.capabilities.contains(&Capability::All) {
            return true;
        }

        // If this contains All but other doesn't, then this is not a subset
        if self.capabilities.contains(&Capability::All) {
            return false;
        }

        // For all other cases, check each capability individually
        for cap in &self.capabilities {
            // Skip None capabilities as they don't represent actual authority
            if *cap == Capability::None {
                continue;
            }

            let found = other.capabilities.iter().any(|other_cap| cap == other_cap);

            if !found {
                return false;
            }
        }

        true
    }
}

impl MeetSemiLattice for CapabilitySet {
    /// Meet operation: greatest lower bound (intersection of capabilities)
    ///
    /// Implements the meet operation ⊓ from §1.4 Algebraic Laws:
    /// - "(refine γ₁; refine γ₂) ≡ refine(γ₁ ⊓ γ₂)" - sequential refinements compose via meet
    ///
    /// From §2.4 Semantic Laws:
    /// - "Meet laws apply to capabilities. These operations are associative, commutative, and idempotent."
    /// - "The operation refine_caps c never increases authority."
    ///
    /// This implementation ensures:
    /// 1. Top element identity: X ⊓ ⊤ = X where ⊤ = {All}
    /// 2. Commutativity: A ⊓ B = B ⊓ A
    /// 3. Associativity: (A ⊓ B) ⊓ C = A ⊓ (B ⊓ C)
    /// 4. Idempotency: A ⊓ A = A
    /// 5. Monotonic restriction: result ≤ both operands
    fn meet(&self, other: &Self) -> Self {
        // Top element identity: X ⊓ ⊤ = X where ⊤ = {All}
        // Per §2.1: "type Cap // partially ordered set (≤), with meet ⊓ and top ⊤"
        // The pure singleton set {All} acts as the identity element for meet
        if self.capabilities.len() == 1 && self.capabilities.contains(&Capability::All) {
            return other.clone();
        }
        if other.capabilities.len() == 1 && other.capabilities.contains(&Capability::All) {
            return self.clone();
        }

        // Handle None capabilities by filtering them out when only one side has them
        let self_has_none = self.capabilities.contains(&Capability::None);
        let other_has_none = other.capabilities.contains(&Capability::None);

        let self_caps = if self_has_none && !other_has_none {
            // Filter None from self when other doesn't have it
            self.capabilities
                .iter()
                .filter(|&cap| cap != &Capability::None)
                .cloned()
                .collect()
        } else {
            self.capabilities.clone()
        };

        let other_caps = if other_has_none && !self_has_none {
            // Filter None from other when self doesn't have it
            other
                .capabilities
                .iter()
                .filter(|&cap| cap != &Capability::None)
                .cloned()
                .collect()
        } else {
            other.capabilities.clone()
        };

        // Standard intersection for all cases (including mixed sets with All)
        // This implements the mathematical meet operation as set intersection.
        // Per §1.4 Monotonic Restriction: "C_{t+1} = C_t ⊓ γ ⟹ C_{t+1} ≤ C_t"
        // The intersection ensures the result is always more restrictive (smaller) than inputs
        Self::from_capabilities(self_caps.intersection(&other_caps).cloned().collect())
    }
}

impl Top for CapabilitySet {
    /// Top element is the set containing All capability
    ///
    /// From §2.1: "type Cap // partially ordered set (≤), with meet ⊓ and top ⊤"
    /// The singleton set {All} serves as the top element (⊤) of the capability lattice.
    ///
    /// This satisfies the top element property: ∀x. x ⊓ ⊤ = x
    fn top() -> Self {
        Self {
            capabilities: [Capability::All].into_iter().collect(),
        }
    }
}

/// Simple pattern matching for resource permissions
fn resource_matches(resource: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        resource.starts_with(prefix)
    } else {
        resource == pattern
    }
}

/// Policy for capability evaluation
///
/// Implements the policy component from §5.2 Web-of-Trust Model:
/// "The effective capability at A is: Caps_A = (LocalGrants_A ⊓ ⋂_{(A,x) ∈ E} Delegation_{x → A}) ⊓ Policy_A"
///
/// This structure holds the local policy constraints that are always applied
/// via meet operations to ensure "Local sovereignty: Policy_A is always in the meet"
#[derive(Debug, Clone)]
pub struct Policy {
    device_capabilities: std::collections::HashMap<DeviceId, CapabilitySet>,
}

impl Policy {
    /// Create new empty policy
    pub fn new() -> Self {
        Self {
            device_capabilities: std::collections::HashMap::new(),
        }
    }

    /// Set capabilities for a device
    /// These represent the LocalGrants_A component before policy refinement
    pub fn set_device_capabilities(&mut self, device_id: DeviceId, capabilities: CapabilitySet) {
        self.device_capabilities.insert(device_id, capabilities);
    }

    /// Get capabilities for a device
    pub fn get_device_capabilities(&self, device_id: &DeviceId) -> Option<&CapabilitySet> {
        self.device_capabilities.get(device_id)
    }
}

/// Evaluation context for capabilities
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    pub device_id: DeviceId,
    pub operation_context: String,
}

impl EvaluationContext {
    /// Create new evaluation context
    pub fn new(device_id: DeviceId, operation_context: String) -> Self {
        Self {
            device_id,
            operation_context,
        }
    }
}

/// Local checks for capability evaluation
///
/// These represent context-specific constraints that are always applied
/// during capability evaluation. Examples include time-based restrictions,
/// operation-specific limits, or environmental constraints.
#[derive(Debug, Clone)]
pub struct LocalChecks {
    /// Time-based capability restrictions
    pub time_restrictions: Vec<TimeRestriction>,
    /// Operation-specific constraints
    pub operation_constraints: Vec<OperationConstraint>,
    /// Resource access limitations
    pub resource_limits: Vec<ResourceLimit>,
}

#[derive(Debug, Clone)]
pub struct TimeRestriction {
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub allowed_capabilities: CapabilitySet,
}

#[derive(Debug, Clone)]
pub struct OperationConstraint {
    pub operation_pattern: String,
    pub max_frequency: Option<u32>,
    pub required_context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResourceLimit {
    pub resource_pattern: String,
    pub max_access_count: Option<u32>,
    pub size_limit: Option<u64>,
}

impl LocalChecks {
    /// Create empty local checks (no restrictions)
    pub fn empty() -> Self {
        Self {
            time_restrictions: Vec::new(),
            operation_constraints: Vec::new(),
            resource_limits: Vec::new(),
        }
    }

    /// Create local checks with time restrictions
    pub fn with_time_restrictions(restrictions: Vec<TimeRestriction>) -> Self {
        Self {
            time_restrictions: restrictions,
            operation_constraints: Vec::new(),
            resource_limits: Vec::new(),
        }
    }

    /// Add operation constraint
    pub fn add_operation_constraint(&mut self, constraint: OperationConstraint) {
        self.operation_constraints.push(constraint);
    }

    /// Add resource limit
    pub fn add_resource_limit(&mut self, limit: ResourceLimit) {
        self.resource_limits.push(limit);
    }
}

/// Single delegation link in a chain
#[derive(Debug, Clone)]
pub struct DelegationLink {
    pub from: DeviceId,
    pub to: DeviceId,
    pub capabilities: CapabilitySet,
    pub max_depth: u32,
}

impl DelegationLink {
    /// Create new delegation link
    pub fn new(from: DeviceId, to: DeviceId, capabilities: CapabilitySet, max_depth: u32) -> Self {
        Self {
            from,
            to,
            capabilities,
            max_depth,
        }
    }
}

/// Chain of delegations
///
/// Implements delegation chains from §5.2 Web-of-Trust Model where each
/// delegation is a "meet-closed element d ∈ Cap, scoped to contexts"
///
/// The chain enforces "Compositionality: Combining multiple delegations uses ⊓ (never widens)"
#[derive(Debug, Clone)]
pub struct DelegationChain {
    links: Vec<DelegationLink>,
}

impl DelegationChain {
    /// Create new delegation chain
    pub fn new() -> Self {
        Self { links: Vec::new() }
    }

    /// Add delegation to chain
    /// Each delegation link further restricts authority through meet operations
    pub fn add_delegation(&mut self, link: DelegationLink) -> Result<(), crate::errors::WotError> {
        self.links.push(link);
        Ok(())
    }

    /// Calculate effective capabilities after applying delegation chain
    ///
    /// Implements the delegation composition from §5.2:
    /// "Combining multiple delegations uses ⊓ (never widens)"
    ///
    /// Each delegation in the chain applies as a meet operation,
    /// ensuring monotonic restriction of authority.
    pub fn effective_capabilities(&self, base: &CapabilitySet) -> CapabilitySet {
        let mut result = base.clone();

        // Apply each delegation link as a meet operation
        // This ensures: base ⊓ d1 ⊓ d2 ⊓ ... ⊓ dn
        // where each di is a delegation that can only restrict, never expand authority
        for link in &self.links {
            result = result.meet(&link.capabilities);
        }

        result
    }
}

/// Evaluate capabilities given policy, delegations, local checks, and context
///
/// This function implements capability evaluation from §1.3 Operational Semantics
/// and §5.2 Web-of-Trust Model.
///
/// The evaluation follows the formula from §5.2:
/// "Caps_A = (LocalGrants_A ⊓ ⋂_{(A,x) ∈ E} Delegation_{x → A}) ⊓ Policy_A"
///
/// Implementation ensures monotonic restriction through meet-semilattice operations:
/// 1. Start with LocalGrants_A from policy
/// 2. Apply each delegation via meet operation (∩ delegations)
/// 3. Apply local policy constraints via meet operation
pub fn evaluate_capabilities(
    policy: &Policy,
    delegations: &[DelegationLink],
    local_checks: &LocalChecks,
    context: &EvaluationContext,
) -> Result<CapabilitySet, crate::errors::WotError> {
    // Step 1: Get base capabilities from policy (LocalGrants_A component)
    let mut result =
        if let Some(base_capabilities) = policy.get_device_capabilities(&context.device_id) {
            base_capabilities.clone()
        } else {
            // No local grants means starting with empty capability set
            CapabilitySet::empty()
        };

    // Step 2: Apply delegations via meet operations
    // Implements: ⋂_{(A,x) ∈ E} Delegation_{x → A}
    // Each delegation can only restrict capabilities, never expand them
    for delegation in delegations {
        // Only apply delegations targeted at this device
        if delegation.to == context.device_id {
            // Apply delegation via meet operation to ensure monotonic restriction
            result = result.meet(&delegation.capabilities);

            // Validate delegation depth limits
            if delegation.max_depth == 0 {
                // Depth-limited delegation that has reached its limit
                // Return empty capabilities to prevent further delegation
                return Ok(CapabilitySet::empty());
            }
        }
    }

    // Step 3: Apply local policy constraints via meet operation
    // This implements the Policy_A component in the formula
    // Local checks could include time-based constraints, context validation, etc.
    let local_policy_caps = apply_local_checks(local_checks, &context)?;
    result = result.meet(&local_policy_caps);

    Ok(result)
}

/// Apply local policy checks to determine allowed capabilities
///
/// This function evaluates local constraints like time restrictions,
/// operation limits, and resource access controls. The result is combined
/// with base capabilities via meet operation to ensure restrictions are enforced.
///
/// Returns capabilities that are currently allowed based on local policy.
fn apply_local_checks(
    local_checks: &LocalChecks,
    context: &EvaluationContext,
) -> Result<CapabilitySet, crate::errors::WotError> {
    use aura_core::time::current_unix_timestamp;

    // Start with top capabilities (no restrictions)
    let mut allowed = CapabilitySet::top();

    // Apply time-based restrictions
    let current_time = current_unix_timestamp();
    for time_restriction in &local_checks.time_restrictions {
        let within_time_window = match (time_restriction.start_time, time_restriction.end_time) {
            (Some(start), Some(end)) => current_time >= start && current_time <= end,
            (Some(start), None) => current_time >= start,
            (None, Some(end)) => current_time <= end,
            (None, None) => true, // No time restriction
        };

        if within_time_window {
            // Time window is active, so these capabilities are allowed
            allowed = allowed.meet(&time_restriction.allowed_capabilities);
        } else {
            // Outside time window, so no capabilities from this restriction
            allowed = allowed.meet(&CapabilitySet::empty());
        }
    }

    // Apply operation-specific constraints
    for operation_constraint in &local_checks.operation_constraints {
        if context
            .operation_context
            .contains(&operation_constraint.operation_pattern)
        {
            // Check required context
            if let Some(ref required_context) = operation_constraint.required_context {
                if !context.operation_context.contains(required_context) {
                    // Required context not present, restrict to empty capabilities
                    allowed = allowed.meet(&CapabilitySet::empty());
                }
            }

            // Note: Frequency limiting would require external state tracking
            // This is omitted for now as it requires persistent storage
        }
    }

    // Apply resource access limitations
    for resource_limit in &local_checks.resource_limits {
        if context
            .operation_context
            .contains(&resource_limit.resource_pattern)
        {
            // Note: Access count and size limit enforcement would require external state
            // This is omitted for now as it requires persistent storage and context

            // For now, we allow access but this is where limits would be enforced
        }
    }

    Ok(allowed)
}
