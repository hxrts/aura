// Authority graph for convergent capability state management

use crate::capability::{
    events::{CapabilityDelegation, CapabilityRevocation},
    types::{CapabilityEvent, CapabilityId, CapabilityResult, CapabilityScope, Subject},
    Result,
};
use std::collections::{BTreeMap, BTreeSet};
use tracing::debug;

/// In-memory authority graph built from capability events
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthorityGraph {
    /// All capability delegations by ID
    delegations: BTreeMap<CapabilityId, CapabilityDelegation>,
    /// All capability revocations by capability ID
    revocations: BTreeMap<CapabilityId, CapabilityRevocation>,
    /// Parent-child relationships for efficient traversal
    children: BTreeMap<CapabilityId, BTreeSet<CapabilityId>>,
    /// Subject index for fast lookup
    subject_capabilities: BTreeMap<Subject, BTreeSet<CapabilityId>>,
    /// Root authorities (capabilities with no parent)
    roots: BTreeSet<CapabilityId>,
    /// Last update timestamp for caching
    last_updated: u64,
}

impl AuthorityGraph {
    /// Create empty authority graph
    pub fn new() -> Self {
        Self {
            delegations: BTreeMap::new(),
            revocations: BTreeMap::new(),
            children: BTreeMap::new(),
            subject_capabilities: BTreeMap::new(),
            roots: BTreeSet::new(),
            last_updated: 0,
        }
    }

    /// Build authority graph deterministically from ordered events
    ///
    /// This function reconstructs the authority graph from a sequence of
    /// capability events in deterministic order. Events are applied in
    /// the order provided, ensuring consistent graph construction.
    pub fn from_events(
        events: Vec<CapabilityEvent>,
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        let mut graph = Self::new();

        debug!("Building authority graph from {} events", events.len());

        // Sort events by their deterministic properties for consistency
        let mut sorted_events = events;
        sorted_events.sort_by(|a, b| {
            // First sort by event type (delegations before revocations)
            let type_order = match (a, b) {
                (CapabilityEvent::Delegation(_), CapabilityEvent::Revocation(_)) => {
                    std::cmp::Ordering::Less
                }
                (CapabilityEvent::Revocation(_), CapabilityEvent::Delegation(_)) => {
                    std::cmp::Ordering::Greater
                }
                _ => std::cmp::Ordering::Equal,
            };

            if type_order != std::cmp::Ordering::Equal {
                return type_order;
            }

            // Then sort by capability ID for deterministic ordering
            let id_a = match a {
                CapabilityEvent::Delegation(d) => &d.capability_id,
                CapabilityEvent::Revocation(r) => &r.capability_id,
            };
            let id_b = match b {
                CapabilityEvent::Delegation(d) => &d.capability_id,
                CapabilityEvent::Revocation(r) => &r.capability_id,
            };

            id_a.cmp(id_b)
        });

        // Apply events in deterministic order
        for event in sorted_events {
            match event {
                CapabilityEvent::Delegation(delegation) => {
                    graph.apply_delegation(delegation, effects)?;
                }
                CapabilityEvent::Revocation(revocation) => {
                    graph.apply_revocation(revocation, effects)?;
                }
            }
        }

        debug!(
            "Authority graph built with {} delegations and {} revocations",
            graph.delegations.len(),
            graph.revocations.len()
        );

        Ok(graph)
    }

    /// Rebuild authority graph from journal events (deterministic reconstruction)
    ///
    /// This method provides a canonical way to reconstruct the authority graph
    /// from the complete event log. It ensures deterministic ordering and
    /// proper application of all capability events.
    pub fn rebuild_from_journal(
        journal_events: &[crate::events::Event],
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        let mut capability_events = Vec::new();

        // Extract capability events from journal in chronological order
        for event in journal_events {
            match &event.event_type {
                crate::events::EventType::CapabilityDelegation(delegation) => {
                    capability_events.push(CapabilityEvent::Delegation(delegation.clone()));
                }
                crate::events::EventType::CapabilityRevocation(revocation) => {
                    capability_events.push(CapabilityEvent::Revocation(revocation.clone()));
                }
                _ => {
                    // Skip non-capability events
                }
            }
        }

        // Build graph from extracted events
        Self::from_events(capability_events, effects)
    }

    /// Merge another authority graph using CRDT semantics
    ///
    /// Implements convergent capability CRDT by taking the union of all
    /// delegations and revocations. Conflicts are resolved deterministically
    /// using capability ID ordering and timestamps.
    pub fn merge(&mut self, other: &AuthorityGraph, effects: &aura_crypto::Effects) -> Result<()> {
        debug!(
            "Merging authority graph with {} delegations and {} revocations",
            other.delegations.len(),
            other.revocations.len()
        );

        // Merge delegations with conflict resolution
        for (cap_id, other_delegation) in &other.delegations {
            match self.delegations.get(cap_id) {
                None => {
                    // No conflict, add the delegation
                    self.apply_delegation(other_delegation.clone(), effects)?;
                }
                Some(existing_delegation) => {
                    // Conflict: resolve deterministically using timestamp + issuing device
                    if self.should_prefer_delegation(existing_delegation, other_delegation) {
                        debug!("Keeping existing delegation for {}", cap_id.to_hex());
                    } else {
                        debug!(
                            "Replacing delegation for {} with newer version",
                            cap_id.to_hex()
                        );
                        // Remove and re-add to update indices
                        self.remove_delegation_indices(cap_id);
                        self.apply_delegation(other_delegation.clone(), effects)?;
                    }
                }
            }
        }

        // Merge revocations - take union since revocations are append-only
        for (cap_id, other_revocation) in &other.revocations {
            match self.revocations.get(cap_id) {
                None => {
                    // No conflict, add the revocation
                    self.apply_revocation(other_revocation.clone(), effects)?;
                }
                Some(existing_revocation) => {
                    // Conflict: keep the earlier revocation (fail-closed security)
                    if other_revocation.revoked_at < existing_revocation.revoked_at {
                        debug!(
                            "Replacing revocation for {} with earlier timestamp",
                            cap_id.to_hex()
                        );
                        self.revocations.insert(*cap_id, other_revocation.clone());
                    }
                }
            }
        }

        self.last_updated = effects.now().unwrap_or(0);
        debug!("Authority graph merge complete");

        Ok(())
    }

    /// Determine which delegation to prefer in case of conflict
    ///
    /// Uses deterministic ordering: timestamp first, then issuing device ID
    fn should_prefer_delegation(
        &self,
        existing: &CapabilityDelegation,
        other: &CapabilityDelegation,
    ) -> bool {
        // Prefer the earlier delegation (first-wins semantics)
        if existing.issued_at != other.issued_at {
            return existing.issued_at < other.issued_at;
        }

        // If timestamps are equal, use device ID for deterministic tie-breaking
        existing.issued_by.0 < other.issued_by.0
    }

    /// Remove delegation from indices (for conflict resolution)
    fn remove_delegation_indices(&mut self, cap_id: &CapabilityId) {
        if let Some(delegation) = self.delegations.get(cap_id) {
            // Remove from subject index
            if let Some(subject_caps) = self.subject_capabilities.get_mut(&delegation.subject_id) {
                subject_caps.remove(cap_id);
                if subject_caps.is_empty() {
                    self.subject_capabilities.remove(&delegation.subject_id);
                }
            }

            // Remove from children index
            if let Some(parent_id) = &delegation.parent_id {
                if let Some(children) = self.children.get_mut(parent_id) {
                    children.remove(cap_id);
                    if children.is_empty() {
                        self.children.remove(parent_id);
                    }
                }
            } else {
                // Remove from roots
                self.roots.remove(cap_id);
            }
        }
    }

    /// Get all capability events for CRDT synchronization
    pub fn get_all_events(&self) -> Vec<(CapabilityId, CapabilityEvent)> {
        let mut events = Vec::new();

        // Add all delegations
        for (cap_id, delegation) in &self.delegations {
            events.push((*cap_id, CapabilityEvent::Delegation(delegation.clone())));
        }

        // Add all revocations
        for (cap_id, revocation) in &self.revocations {
            events.push((*cap_id, CapabilityEvent::Revocation(revocation.clone())));
        }

        // Sort by capability ID for deterministic ordering
        events.sort_by(|a, b| a.0.cmp(&b.0));

        events
    }

    /// Apply a capability delegation to the graph
    pub fn apply_delegation(
        &mut self,
        delegation: CapabilityDelegation,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        let capability_id = delegation.capability_id;

        debug!(
            "Applying capability delegation for {}",
            capability_id.to_hex()
        );

        // Update parent-child relationships
        if let Some(parent_id) = &delegation.parent_id {
            self.children
                .entry(*parent_id)
                .or_default()
                .insert(capability_id);
        } else {
            // This is a root authority
            self.roots.insert(capability_id);
        }

        // Update subject index
        self.subject_capabilities
            .entry(delegation.subject_id.clone())
            .or_default()
            .insert(capability_id);

        // Store the delegation
        self.delegations.insert(capability_id, delegation);
        self.last_updated = effects.now().unwrap_or(0);

        Ok(())
    }

    /// Apply a capability revocation to the graph
    pub fn apply_revocation(
        &mut self,
        revocation: CapabilityRevocation,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        let capability_id = revocation.capability_id;

        debug!(
            "Applying capability revocation for {}",
            capability_id.to_hex()
        );

        // Store the revocation
        self.revocations.insert(capability_id, revocation);
        self.last_updated = effects.now().unwrap_or(0);

        Ok(())
    }

    /// Evaluate capability for a subject and scope
    pub fn evaluate_capability(
        &self,
        subject: &Subject,
        scope: &CapabilityScope,
        effects: &aura_crypto::Effects,
    ) -> CapabilityResult {
        debug!(
            "Evaluating capability for subject {} scope {:?}",
            subject, scope
        );

        // Get all capabilities for this subject
        let Some(subject_caps) = self.subject_capabilities.get(subject) else {
            return CapabilityResult::NotFound;
        };

        // Check each capability to see if it grants the requested scope
        for cap_id in subject_caps {
            if let Some(delegation) = self.delegations.get(cap_id) {
                // Check if revoked
                if self.revocations.contains_key(cap_id) {
                    continue;
                }

                // Check if expired
                if delegation.is_expired(effects) {
                    continue;
                }

                // Check if scope matches
                if delegation.scope.subsumes(scope) {
                    debug!("Found matching capability: {}", cap_id.to_hex());
                    return CapabilityResult::Granted;
                }
            }
        }

        CapabilityResult::NotFound
    }

    /// Get all subjects with a specific scope
    pub fn get_subjects_with_scope(
        &self,
        scope: &CapabilityScope,
        effects: &aura_crypto::Effects,
    ) -> Vec<Subject> {
        let mut subjects = Vec::new();

        for (subject, cap_ids) in &self.subject_capabilities {
            for cap_id in cap_ids {
                if let Some(delegation) = self.delegations.get(cap_id) {
                    // Skip if revoked or expired
                    if self.revocations.contains_key(cap_id) || delegation.is_expired(effects) {
                        continue;
                    }

                    // Check if this capability grants the scope
                    if delegation.scope.subsumes(scope) {
                        subjects.push(subject.clone());
                        break; // Found one matching capability for this subject
                    }
                }
            }
        }

        subjects
    }

    /// Record a Keyhive delegation in the authority graph
    pub fn record_delegation(
        &mut self,
        capability_id: &str,
        device_id: &aura_types::DeviceId,
        permissions: &[String],
        delegated_at: u64,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        use crate::capability::events::CapabilityDelegation;
        use crate::capability::types::{CapabilityId, CapabilityScope, Subject};

        let cap_id = CapabilityId::from_hex(capability_id)?;
        let subject = Subject::Device(*device_id);
        let scope = CapabilityScope::from_permissions(permissions);

        let delegation = CapabilityDelegation {
            capability_id: cap_id,
            parent_id: None, // Root delegation
            subject_id: subject.clone(),
            scope,
            expiry: None,  // Keyhive delegations don't expire
            proof: vec![], // TODO: Add proper cryptographic proof
            issued_at: delegated_at,
            issued_by: *device_id, // Device that received the delegation
        };

        self.delegations.insert(cap_id, delegation);
        self.subject_capabilities
            .entry(subject)
            .or_default()
            .insert(cap_id);
        self.roots.insert(cap_id);
        self.last_updated = current_timestamp_with_effects(effects);

        Ok(())
    }

    /// Record a Keyhive revocation in the authority graph
    pub fn record_revocation(
        &mut self,
        capability_id: &str,
        device_id: &aura_types::DeviceId,
        revoked_at: u64,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        use crate::capability::events::CapabilityRevocation;
        use crate::capability::types::{CapabilityId, Subject};

        let cap_id = CapabilityId::from_hex(capability_id)?;
        let subject = Subject::Device(*device_id);

        let revocation = CapabilityRevocation {
            capability_id: cap_id,
            revoked_at,
            reason: "Keyhive capability revoked".to_string(),
            proof: vec![],         // TODO: Add proper cryptographic proof
            issued_by: *device_id, // Device that triggered the revocation
        };

        self.revocations.insert(cap_id, revocation);

        // Remove from subject's capabilities
        if let Some(caps) = self.subject_capabilities.get_mut(&subject) {
            caps.remove(&cap_id);
        }

        // Remove from roots if it was a root
        self.roots.remove(&cap_id);
        self.last_updated = current_timestamp_with_effects(effects);

        Ok(())
    }

    /// Find capabilities that should be revoked due to cascading effects
    pub fn find_cascading_revocations(
        &self,
        capability_id: &str,
        device_id: &aura_types::DeviceId,
    ) -> Vec<(String, aura_types::DeviceId)> {
        use crate::capability::types::{CapabilityId, Subject};

        let mut cascading = Vec::new();
        let cap_id = match CapabilityId::from_hex(capability_id) {
            Ok(id) => id,
            Err(_) => return cascading,
        };

        // Find all capabilities that were delegated by this capability
        if let Some(children) = self.children.get(&cap_id) {
            for child_id in children {
                if let Some(child_delegation) = self.delegations.get(child_id) {
                    // If the delegation was issued by this device, it should be revoked
                    if child_delegation.issued_by == *device_id {
                        if let Subject::Device(target_device) = &child_delegation.subject_id {
                            cascading.push((child_id.to_hex(), *target_device));
                        }
                    }
                }
            }
        }

        cascading
    }

    /// Apply a single capability event to the authority graph
    pub fn apply_event(
        &mut self,
        event: crate::capability::types::CapabilityEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        use crate::capability::types::CapabilityEvent;

        match event {
            CapabilityEvent::Delegation(delegation) => {
                // Add to delegations
                self.delegations
                    .insert(delegation.capability_id, delegation.clone());

                // Update subject index
                self.subject_capabilities
                    .entry(delegation.subject_id.clone())
                    .or_default()
                    .insert(delegation.capability_id);

                // Update parent-child relationships if this has a parent
                let delegating_device = delegation.issued_by;
                let delegating_subject = Subject::Device(delegating_device);
                if let Some(parent_caps) = self.subject_capabilities.get(&delegating_subject) {
                    for parent_id in parent_caps {
                        self.children
                            .entry(*parent_id)
                            .or_default()
                            .insert(delegation.capability_id);
                    }
                } else {
                    // No parent found, this is a root capability
                    self.roots.insert(delegation.capability_id);
                }
            }
            CapabilityEvent::Revocation(revocation) => {
                // Add to revocations
                self.revocations
                    .insert(revocation.capability_id, revocation.clone());

                // Remove from subject capabilities (capability is now revoked)
                for caps in self.subject_capabilities.values_mut() {
                    caps.remove(&revocation.capability_id);
                }

                // Remove from roots if it was a root
                self.roots.remove(&revocation.capability_id);
            }
        }

        self.last_updated = current_timestamp_with_effects(effects);
        Ok(())
    }
}

// Helper function to get current timestamp
fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> u64 {
    effects.now().unwrap_or(0)
}

impl Default for AuthorityGraph {
    fn default() -> Self {
        Self::new()
    }
}
