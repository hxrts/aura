// Capability-driven agent with no legacy authorization

use aura_journal::{
    bootstrap::{BootstrapConfig, BootstrapManager},
    capability::{
        authority_graph::AuthorityGraph,
        identity::{IdentityCapabilityManager, IndividualId},
        types::{CapabilityScope, Subject, CapabilityResult},
        events::{CapabilityDelegation, CapabilityRevocation},
    },
    AccountId, DeviceId,
};
use aura_cgka::{BeeKemManager, CausalEncryption};
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

/// Capability-driven agent with integrated CGKA and identity management
///
/// The `CapabilityAgent` provides core capability-based authorization and CGKA functionality
/// without external dependencies like transport or storage. This makes it ideal for:
///
/// - **Testing**: Unit tests without infrastructure dependencies
/// - **Embedded systems**: Minimal resource environments
/// - **Library integration**: When you need just the core logic
///
/// For full system integration with network and storage, use [`IntegratedAgent`] instead.
///
/// # Example
///
/// ```rust,ignore
/// use aura_agent::CapabilityAgent;
/// use aura_journal::{DeviceId, AccountId};
///
/// let device_id = DeviceId::new();
/// let account_id = AccountId::new();
/// let mut agent = CapabilityAgent::new(device_id, account_id);
///
/// // Bootstrap new account
/// agent.bootstrap_account(vec![device_id], 2)?;
///
/// // Check capabilities
/// let scope = CapabilityScope::simple("mls", "admin");
/// if agent.has_capability(&scope) {
///     // Create MLS group
///     agent.create_mls_group("team-chat", vec![])?;
/// }
/// ```
pub struct CapabilityAgent {
    /// Device identity
    pub device_id: DeviceId,
    /// Individual identity derived from device
    pub individual_id: IndividualId,
    /// Account identifier
    pub account_id: AccountId,
    /// Capability authority graph
    pub authority_graph: AuthorityGraph,
    /// Identity management
    pub identity_manager: IdentityCapabilityManager,
    /// BeeKEM CGKA manager
    pub cgka_manager: BeeKemManager,
    /// Causal encryption
    pub causal_encryption: CausalEncryption,
    /// Bootstrap manager for account initialization
    pub bootstrap_manager: BootstrapManager,
    /// Injectable effects for deterministic testing
    pub effects: aura_crypto::Effects,
}

impl CapabilityAgent {
    /// Create new capability agent
    pub fn new(device_id: DeviceId, account_id: AccountId) -> Self {
        let individual_id = IndividualId::from_device(&device_id);
        let effects = aura_crypto::Effects::test(); // Use test effects by default
        
        info!("Creating capability agent for device {} (individual: {})", 
              device_id.0, individual_id.0);
        
        Self {
            device_id,
            individual_id,
            account_id,
            authority_graph: AuthorityGraph::new(),
            identity_manager: IdentityCapabilityManager::new(),
            cgka_manager: BeeKemManager::new(effects.clone()),
            causal_encryption: CausalEncryption::new(),
            bootstrap_manager: BootstrapManager::new(),
            effects,
        }
    }
    
    /// Create new capability agent with injected effects
    pub fn with_effects(device_id: DeviceId, account_id: AccountId, effects: aura_crypto::Effects) -> Self {
        let individual_id = IndividualId::from_device(&device_id);
        
        info!("Creating capability agent for device {} (individual: {}) with custom effects", 
              device_id.0, individual_id.0);
        
        Self {
            device_id,
            individual_id,
            account_id,
            authority_graph: AuthorityGraph::new(),
            identity_manager: IdentityCapabilityManager::new(),
            cgka_manager: BeeKemManager::new(effects.clone()),
            causal_encryption: CausalEncryption::new(),
            bootstrap_manager: BootstrapManager::new(),
            effects,
        }
    }
    
    /// Bootstrap new account with this device as root authority
    pub fn bootstrap_account(&mut self, initial_devices: Vec<DeviceId>, threshold: u16, effects: &aura_crypto::Effects) -> Result<(), AgentError> {
        info!("Bootstrapping account with {} devices (threshold: {})", 
              initial_devices.len(), threshold);
        
        let config = BootstrapConfig::new(self.account_id, initial_devices, threshold);
        let result = self.bootstrap_manager.bootstrap_account(config, effects)
            .map_err(|e| AgentError::BootstrapError(e.to_string()))?;
        
        // Apply genesis delegations to authority graph
        for delegation in result.genesis_delegations {
            self.authority_graph.apply_delegation(delegation, &self.effects)
                .map_err(|e| AgentError::CapabilityError(e.to_string()))?;
        }
        
        // Register identity mappings
        for (device_id, individual_id) in result.identity_mappings {
            self.identity_manager.register_device_identity(device_id, individual_id);
        }
        
        info!("Account bootstrap complete");
        
        Ok(())
    }
    
    /// Check if agent has specific capability
    pub fn check_capability(&self, scope: &CapabilityScope) -> bool {
        let subject = self.individual_id.to_subject();
        let result = self.authority_graph.evaluate_capability(&subject, scope, &self.effects);
        matches!(result, CapabilityResult::Granted)
    }
    
    /// Check if agent has specific capability
    pub fn has_capability(&self, scope: &CapabilityScope) -> bool {
        self.check_capability(scope)
    }
    
    /// Require specific capability or return error
    pub fn require_capability(&self, scope: &CapabilityScope) -> Result<(), AgentError> {
        if self.has_capability(scope) {
            Ok(())
        } else {
            Err(AgentError::InsufficientCapability(format!(
                "Required capability not found: {}:{}", 
                scope.namespace, scope.operation
            )))
        }
    }
    
    /// Delegate capability to another subject
    pub fn delegate_capability(
        &mut self,
        parent_capability: CapabilityScope,
        target_subject: Subject,
        new_scope: CapabilityScope,
        expiry: Option<u64>,
    ) -> Result<CapabilityDelegation, AgentError> {
        // Check that we have delegation authority for the parent capability
        let delegation_scope = CapabilityScope::simple("capability", "delegate");
        self.require_capability(&delegation_scope)?;
        
        // Also check we have the parent capability
        self.require_capability(&parent_capability)?;
        
        info!("Delegating capability {} to subject {}", 
              new_scope.namespace, target_subject.0);
        
        // Find parent capability ID
        // For simplicity, we'll create a new root delegation
        // In production, this would properly chain from existing capability
        
        let delegation = CapabilityDelegation::new(
            None, // Simplified - should find actual parent ID
            target_subject,
            new_scope,
            expiry,
            vec![0u8; 64], // Placeholder signature
            self.device_id,
            &self.effects,
        );
        
        // Apply to authority graph
        self.authority_graph.apply_delegation(delegation.clone(), &self.effects)
            .map_err(|e| AgentError::CapabilityError(e.to_string()))?;
        
        debug!("Capability delegation created: {}", delegation.capability_id.as_hex());
        
        Ok(delegation)
    }
    
    /// Revoke capability
    pub fn revoke_capability(
        &mut self,
        capability_id: aura_journal::capability::types::CapabilityId,
        reason: String,
    ) -> Result<CapabilityRevocation, AgentError> {
        // Check that we have revocation authority
        let revocation_scope = CapabilityScope::simple("capability", "revoke");
        self.require_capability(&revocation_scope)?;
        
        info!("Revoking capability {}: {}", capability_id.as_hex(), reason);
        
        let revocation = CapabilityRevocation::new(
            capability_id,
            reason,
            vec![0u8; 64], // Placeholder signature
            self.device_id,
            &self.effects,
        );
        
        // Apply to authority graph
        self.authority_graph.apply_revocation(revocation.clone(), &self.effects)
            .map_err(|e| AgentError::CapabilityError(e.to_string()))?;
        
        debug!("Capability revoked");
        
        Ok(revocation)
    }
    
    /// Create MLS group with capability-driven membership
    pub fn create_group(&mut self, group_id: &str, initial_members: Vec<IndividualId>) -> Result<(), AgentError> {
        // Check MLS admin capability
        let mls_admin_scope = CapabilityScope::simple("mls", "admin");
        self.require_capability(&mls_admin_scope)?;
        
        info!("Creating MLS group '{}' with {} members", group_id, initial_members.len());
        
        // Create capability delegations for group membership
        let member_delegations = self.bootstrap_manager.create_mls_group(group_id, initial_members, &self.effects)
            .map_err(|e| AgentError::BootstrapError(e.to_string()))?;
        
        // Apply delegations to authority graph
        for delegation in member_delegations {
            self.authority_graph.apply_delegation(delegation, &self.effects)
                .map_err(|e| AgentError::CapabilityError(e.to_string()))?;
        }
        
        // Initialize CGKA group
        self.cgka_manager.initialize_group(group_id.to_string(), &self.authority_graph)
            .map_err(|e| AgentError::CgkaError(e.to_string()))?;
        
        info!("MLS group '{}' created successfully", group_id);
        
        Ok(())
    }
    
    /// Process capability changes and update CGKA state
    pub fn process_capability_changes(&mut self, group_id: &str) -> Result<(), AgentError> {
        debug!("Processing capability changes for group '{}'", group_id);
        
        let operations = self.cgka_manager.process_capability_changes(group_id, &self.authority_graph)
            .map_err(|e| AgentError::CgkaError(e.to_string()))?;
        
        if !operations.is_empty() {
            info!("Generated {} CGKA operations from capability changes", operations.len());
            
            // Apply operations to CGKA state
            for operation in operations {
                self.cgka_manager.apply_operation(group_id, operation)
                    .map_err(|e| AgentError::CgkaError(e.to_string()))?;
            }
        }
        
        Ok(())
    }
    
    /// Encrypt data using causal encryption
    pub fn encrypt(&mut self, data: &[u8], context: &str, group_id: &str) -> Result<Vec<u8>, AgentError> {
        // Check that we have access to the group
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        self.require_capability(&member_scope)?;
        
        // Get application secret from CGKA
        let app_secret = self.cgka_manager.get_application_secret(group_id)
            .ok_or_else(|| AgentError::CgkaError("No application secret available".to_string()))?;
        
        // Add application secret to causal encryption
        self.causal_encryption.add_application_secret(app_secret.clone());
        
        // Derive causal key for this context
        let epoch = app_secret.epoch;
        let _causal_key = self.causal_encryption.derive_causal_key(context, epoch)
            .map_err(|e| AgentError::CgkaError(e.to_string()))?;
        
        // Encrypt data
        let ciphertext = self.causal_encryption.encrypt(data, context)
            .map_err(|e| AgentError::CgkaError(e.to_string()))?;
        
        debug!("Encrypted {} bytes for context '{}'", data.len(), context);
        
        // Return serialized ciphertext
        serde_json::to_vec(&ciphertext)
            .map_err(|e| AgentError::SerializationError(e.to_string()))
    }
    
    /// Decrypt data using causal encryption
    pub fn decrypt(&self, ciphertext_bytes: &[u8], group_id: &str) -> Result<Vec<u8>, AgentError> {
        // Check that we have access to the group
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        self.require_capability(&member_scope)?;
        
        // Deserialize ciphertext
        let ciphertext = serde_json::from_slice(ciphertext_bytes)
            .map_err(|e| AgentError::SerializationError(e.to_string()))?;
        
        // Decrypt data
        let plaintext = self.causal_encryption.decrypt(&ciphertext)
            .map_err(|e| AgentError::CgkaError(e.to_string()))?;
        
        debug!("Decrypted {} bytes", plaintext.len());
        
        Ok(plaintext)
    }
    
    /// List groups this agent is a member of
    pub fn list_groups(&self) -> Vec<String> {
        // Find all MLS member capabilities
        let groups = Vec::new();
        
        // This is simplified - in practice we'd iterate through the authority graph
        // and find all capabilities with namespace "mls" and operation "member"
        
        debug!("Found {} group memberships", groups.len());
        
        groups
    }
    
    /// List agent's current capabilities
    pub fn list_capabilities(&self) -> Vec<CapabilityScope> {
        // This would extract all capabilities for this agent's subject
        // For now, return empty list
        Vec::new()
    }
    
    
    // ========== Additional standard methods ==========
    
    /// Join an existing group (placeholder for future implementation)
    pub fn join_group(&mut self, group_id: &str) -> Result<(), AgentError> {
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        self.require_capability(&member_scope)?;
        
        info!("Joining group '{}'", group_id);
        // TODO: Implement actual group joining logic
        Ok(())
    }
    
    /// Leave a group (placeholder for future implementation)
    pub fn leave_group(&mut self, group_id: &str) -> Result<(), AgentError> {
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        self.require_capability(&member_scope)?;
        
        info!("Leaving group '{}'", group_id);
        // TODO: Implement actual group leaving logic
        Ok(())
    }
    
    /// Get agent identity information
    pub fn identity(&self) -> (DeviceId, AccountId, IndividualId) {
        (self.device_id, self.account_id, self.individual_id.clone())
    }
    
    /// Check if agent is member of specific group
    pub fn is_group_member(&self, group_id: &str) -> bool {
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        self.check_capability(&member_scope)
    }
}

// AgentError is defined in lib.rs
pub use crate::AgentError;

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Device identifier
    pub device_id: DeviceId,
    /// Account identifier
    pub account_id: AccountId,
    /// Threshold configuration
    pub threshold: u16,
    /// Enable CGKA
    pub enable_cgka: bool,
    /// Enable causal encryption
    pub enable_causal_encryption: bool,
}

impl AgentConfig {
    /// Create new agent configuration
    pub fn new(device_id: DeviceId, account_id: AccountId) -> Self {
        Self {
            device_id,
            account_id,
            threshold: 2, // Default 2-of-3
            enable_cgka: true,
            enable_causal_encryption: true,
        }
    }
}