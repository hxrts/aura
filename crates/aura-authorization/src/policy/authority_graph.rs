//! Authority graph for tracking delegation relationships

use crate::{Result, Subject};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// A node in the authority graph representing a subject and their authorities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityNode {
    /// The subject this node represents
    pub subject: Subject,

    /// Capabilities this subject has been granted
    pub capabilities: Vec<Uuid>,

    /// Subjects this subject has delegated to
    pub delegated_to: Vec<Subject>,

    /// Subjects this subject has received delegations from
    pub delegated_from: Vec<Subject>,

    /// Whether this subject is trusted (bootstrap trust)
    pub is_trusted: bool,
}

/// Graph tracking all authority relationships between subjects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityGraph {
    /// All subjects and their authority information
    nodes: HashMap<String, AuthorityNode>,

    /// Revoked capabilities that should not be honored
    revoked_capabilities: HashSet<Uuid>,

    /// Root authorities that bootstrap the trust system
    root_authorities: HashSet<String>,
}

impl AuthorityGraph {
    /// Create a new empty authority graph
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            revoked_capabilities: HashSet::new(),
            root_authorities: HashSet::new(),
        }
    }

    /// Add a root authority that can bootstrap trust
    pub fn add_root_authority(&mut self, subject: Subject) {
        let subject_key = subject_to_key(&subject);
        self.root_authorities.insert(subject_key.clone());

        let node = AuthorityNode {
            subject,
            capabilities: Vec::new(),
            delegated_to: Vec::new(),
            delegated_from: Vec::new(),
            is_trusted: true,
        };

        self.nodes.insert(subject_key, node);
    }

    /// Add or update a subject in the graph
    pub fn add_subject(&mut self, subject: Subject) -> Result<()> {
        let subject_key = subject_to_key(&subject);

        if !self.nodes.contains_key(&subject_key) {
            let node = AuthorityNode {
                subject,
                capabilities: Vec::new(),
                delegated_to: Vec::new(),
                delegated_from: Vec::new(),
                is_trusted: self.root_authorities.contains(&subject_key),
            };

            self.nodes.insert(subject_key, node);
        }

        Ok(())
    }

    /// Record a capability delegation
    pub fn add_delegation(
        &mut self,
        delegator: &Subject,
        delegatee: &Subject,
        capability_id: Uuid,
    ) -> Result<()> {
        let delegator_key = subject_to_key(delegator);
        let delegatee_key = subject_to_key(delegatee);

        // Ensure both subjects exist
        self.add_subject(delegator.clone())?;
        self.add_subject(delegatee.clone())?;

        // Update delegator's "delegated_to" list
        if let Some(delegator_node) = self.nodes.get_mut(&delegator_key) {
            if !delegator_node.delegated_to.contains(delegatee) {
                delegator_node.delegated_to.push(delegatee.clone());
            }
        }

        // Update delegatee's "delegated_from" list and add capability
        if let Some(delegatee_node) = self.nodes.get_mut(&delegatee_key) {
            if !delegatee_node.delegated_from.contains(delegator) {
                delegatee_node.delegated_from.push(delegator.clone());
            }
            delegatee_node.capabilities.push(capability_id);
        }

        Ok(())
    }

    /// Revoke a capability
    pub fn revoke_capability(&mut self, capability_id: Uuid) {
        self.revoked_capabilities.insert(capability_id);

        // Remove from all nodes
        for node in self.nodes.values_mut() {
            node.capabilities.retain(|&id| id != capability_id);
        }
    }

    /// Check if a capability is revoked
    pub fn is_revoked(&self, capability_id: &Uuid) -> bool {
        self.revoked_capabilities.contains(capability_id)
    }

    /// Check if a subject has a path to a trusted authority
    pub fn has_trust_path(&self, subject: &Subject) -> bool {
        let subject_key = subject_to_key(subject);
        let mut visited = HashSet::new();
        self.has_trust_path_recursive(&subject_key, &mut visited)
    }

    /// Get all capabilities for a subject
    pub fn get_capabilities(&self, subject: &Subject) -> Vec<Uuid> {
        let subject_key = subject_to_key(subject);
        self.nodes
            .get(&subject_key)
            .map(|node| node.capabilities.clone())
            .unwrap_or_default()
    }

    /// Get all subjects that delegated to this subject
    pub fn get_delegators(&self, subject: &Subject) -> Vec<Subject> {
        let subject_key = subject_to_key(subject);
        self.nodes
            .get(&subject_key)
            .map(|node| node.delegated_from.clone())
            .unwrap_or_default()
    }

    /// Check if a subject has direct authority over a resource
    pub fn has_direct_authority(
        &self,
        subject: &Subject,
        _resource: &crate::Resource,
    ) -> Result<bool> {
        let subject_key = subject_to_key(subject);
        // Check if the subject exists in the graph and has trust
        Ok(self
            .nodes
            .get(&subject_key)
            .map(|node| node.is_trusted || self.has_trust_path(subject))
            .unwrap_or(false))
    }

    /// Recursive helper for trust path checking
    fn has_trust_path_recursive(&self, subject_key: &str, visited: &mut HashSet<String>) -> bool {
        if visited.contains(subject_key) {
            return false; // Avoid cycles
        }

        if let Some(node) = self.nodes.get(subject_key) {
            if node.is_trusted {
                return true;
            }

            visited.insert(subject_key.to_string());

            // Check if any delegator has a trust path
            for delegator in &node.delegated_from {
                let delegator_key = subject_to_key(delegator);
                if self.has_trust_path_recursive(&delegator_key, visited) {
                    return true;
                }
            }

            visited.remove(subject_key);
        }

        false
    }
}

impl Default for AuthorityGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a subject to a string key for the hash map
fn subject_to_key(subject: &Subject) -> String {
    match subject {
        Subject::Device(device_id) => format!("device:{}", device_id),
        Subject::Guardian(guardian_id) => format!("guardian:{}", guardian_id),
        Subject::ThresholdGroup {
            participants,
            threshold,
        } => {
            let mut ids: Vec<String> = participants.iter().map(|id| id.to_string()).collect();
            ids.sort();
            format!("threshold:{}:{}", threshold, ids.join(","))
        }
        Subject::Session { session_id, issuer } => {
            format!("session:{}:{}", session_id, issuer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

    #[test]
    fn test_authority_graph_creation() {
        let graph = AuthorityGraph::new();
        assert!(graph.nodes.is_empty());
        assert!(graph.root_authorities.is_empty());
    }

    #[test]
    fn test_add_root_authority() {
        let effects = Effects::test();
        let mut graph = AuthorityGraph::new();

        let root_device = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        graph.add_root_authority(root_device.clone());

        assert!(graph.has_trust_path(&root_device));
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn test_delegation_tracking() {
        let effects = Effects::test();
        let mut graph = AuthorityGraph::new();

        let delegator = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let delegatee = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let capability_id = effects.gen_uuid();

        // Add root authority
        graph.add_root_authority(delegator.clone());

        // Add delegation
        graph
            .add_delegation(&delegator, &delegatee, capability_id)
            .unwrap();

        // Check delegation was recorded
        let delegatee_capabilities = graph.get_capabilities(&delegatee);
        assert!(delegatee_capabilities.contains(&capability_id));

        let delegators = graph.get_delegators(&delegatee);
        assert!(delegators.contains(&delegator));

        // Check trust path
        assert!(graph.has_trust_path(&delegatee));
    }

    #[test]
    fn test_capability_revocation() {
        let effects = Effects::test();
        let mut graph = AuthorityGraph::new();

        let subject = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let capability_id = effects.gen_uuid();

        graph.add_subject(subject.clone()).unwrap();

        // Manually add capability for testing
        let subject_key = subject_to_key(&subject);
        if let Some(node) = graph.nodes.get_mut(&subject_key) {
            node.capabilities.push(capability_id);
        }

        // Verify capability exists
        assert!(graph.get_capabilities(&subject).contains(&capability_id));
        assert!(!graph.is_revoked(&capability_id));

        // Revoke capability
        graph.revoke_capability(capability_id);

        // Verify capability is revoked and removed
        assert!(graph.is_revoked(&capability_id));
        assert!(!graph.get_capabilities(&subject).contains(&capability_id));
    }

    #[test]
    fn test_subject_key_generation() {
        let effects = Effects::test();
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        let guardian_id = effects.gen_uuid();

        let device_subject = Subject::Device(device_id);
        let guardian_subject = Subject::Guardian(guardian_id);

        let device_key = subject_to_key(&device_subject);
        let guardian_key = subject_to_key(&guardian_subject);

        assert!(device_key.starts_with("device:"));
        assert!(guardian_key.starts_with("guardian:"));
        assert_ne!(device_key, guardian_key);
    }
}
