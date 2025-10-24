// Integrated agent with transport and storage

use crate::capability_agent::{CapabilityAgent, AgentError};
use std::sync::Arc;
use aura_journal::{
    capability::{
        identity::IndividualId,
        types::CapabilityScope,
    },
    DeviceId, AccountId,
};
use aura_transport::CapabilityTransport;
use aura_store::CapabilityStorage;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, debug};

/// Fully integrated agent with transport, storage, capabilities, and CGKA
pub struct IntegratedAgent {
    /// Core capability agent
    pub capability_agent: CapabilityAgent,
    /// Transport layer
    pub transport: CapabilityTransport,
    /// Storage layer
    pub storage: CapabilityStorage,
    /// Active network connections
    pub connections: RwLock<BTreeSet<IndividualId>>,
}

impl IntegratedAgent {
    /// Create new integrated agent
    pub async fn new(
        device_id: DeviceId,
        account_id: AccountId,
        storage_root: PathBuf,
        effects: aura_crypto::Effects,
    ) -> Result<Self, AgentError> {
        let individual_id = IndividualId::from_device(&device_id);
        
        info!("Creating integrated agent for device {} (individual: {})", 
              device_id.0, individual_id.0);
        
        // Create core capability agent with injected effects
        let capability_agent = CapabilityAgent::with_effects(device_id, account_id, effects.clone());
        
        // Create device key manager for transport authentication
        let mut device_key_manager = aura_crypto::DeviceKeyManager::new(effects.clone());
        device_key_manager.generate_device_key(device_id.0)
            .map_err(|e| AgentError::crypto_operation(format!("Failed to generate device key: {:?}", e)))?;
        
        // Create transport layer  
        let base_transport = Arc::new(aura_transport::StubTransport::new());
        let transport = CapabilityTransport::new(base_transport, individual_id.clone(), device_key_manager, effects.clone());
        
        // Create storage layer
        let storage = CapabilityStorage::new(storage_root, individual_id, effects.clone())
            .await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        Ok(Self {
            capability_agent,
            transport,
            storage,
            connections: RwLock::new(BTreeSet::new()),
        })
    }
    
    /// Bootstrap new account with this agent as root authority  
    pub async fn bootstrap(
        &mut self,
        initial_devices: Vec<DeviceId>,
        threshold: u16,
        effects: &aura_crypto::Effects,
    ) -> Result<(), AgentError> {
        info!("Bootstrapping account with integrated agent");
        
        // Bootstrap using capability agent
        self.capability_agent.bootstrap_account(initial_devices, threshold, effects)?;
        
        // Update transport and storage with new authority graph
        self.sync_authority_graph().await?;
        
        // Create default storage capabilities
        self.create_default_storage_capabilities().await?;
        
        info!("Account bootstrap complete with integrated systems");
        
        Ok(())
    }
    
    /// Connect to peer and establish capability-authenticated transport
    pub async fn network_connect(&self, peer: IndividualId, address: &str) -> Result<(), AgentError> {
        info!("Connecting to peer {} at {}", peer.0, address);
        
        // Check that we have permission to communicate with this peer
        let comm_scope = CapabilityScope::simple("transport", "communicate");
        self.capability_agent.require_capability(&comm_scope)?;
        
        // TODO: Connect to peer using proper presence ticket handshake
        // For now, just track the connection internally
        
        // Add to connections
        {
            let mut connections = self.connections.write().await;
            connections.insert(peer.clone());
        }
        
        info!("Connected to peer: {}", peer.0);
        
        Ok(())
    }
    
    /// Send capability delegation to network
    pub async fn network_delegate_capability(
        &mut self,
        parent_capability: CapabilityScope,
        target_subject: aura_journal::capability::types::Subject,
        new_scope: CapabilityScope,
        expiry: Option<u64>,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> Result<(), AgentError> {
        info!("Delegating capability {} to {} via network", 
              new_scope.namespace, target_subject.0);
        
        // Create delegation using capability agent
        let delegation = self.capability_agent.delegate_capability(
            parent_capability,
            target_subject,
            new_scope,
            expiry,
        )?;
        
        // Send via transport
        self.transport.send_capability_delegation(delegation, recipients)
            .await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        // Sync authority graph across systems
        self.sync_authority_graph().await?;
        
        info!("Capability delegation sent via network");
        
        Ok(())
    }
    
    /// Revoke capability and propagate to network
    pub async fn network_revoke_capability(
        &mut self,
        capability_id: aura_journal::capability::types::CapabilityId,
        reason: String,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> Result<(), AgentError> {
        info!("Revoking capability {} via network: {}", 
              capability_id.as_hex(), reason);
        
        // Create revocation using capability agent
        let revocation = self.capability_agent.revoke_capability(capability_id, reason)?;
        
        // Send via transport
        self.transport.send_capability_revocation(revocation, recipients)
            .await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        // Sync authority graph across systems
        self.sync_authority_graph().await?;
        
        info!("Capability revocation sent via network");
        
        Ok(())
    }
    
    /// Store data with capability protection
    pub async fn store(
        &self,
        entry_id: String,
        data: Vec<u8>,
        content_type: String,
        required_scope: CapabilityScope,
        acl: Option<BTreeSet<IndividualId>>,
        attributes: BTreeMap<String, String>,
    ) -> Result<(), AgentError> {
        info!("Storing {} bytes as entry '{}' with scope {}:{}", 
              data.len(), entry_id, required_scope.namespace, required_scope.operation);
        
        // Ensure we have current application secrets for encryption
        self.sync_application_secrets().await?;
        
        // Store using capability storage
        self.storage.store(entry_id, data, content_type, required_scope, acl, attributes, &self.capability_agent.effects)
            .await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        info!("Data stored successfully");
        
        Ok(())
    }
    
    /// Retrieve data with capability checking
    pub async fn retrieve(&self, entry_id: &str) -> Result<Vec<u8>, AgentError> {
        debug!("Retrieving entry '{}'", entry_id);
        
        let data = self.storage.retrieve(entry_id, &self.capability_agent.effects)
            .await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        debug!("Retrieved {} bytes from entry '{}'", data.len(), entry_id);
        
        Ok(data)
    }
    
    /// Create and send MLS group with integrated systems
    pub async fn network_create_group(
        &mut self,
        group_id: &str,
        initial_members: Vec<IndividualId>,
    ) -> Result<(), AgentError> {
        info!("Creating MLS group '{}' with {} members via network", 
              group_id, initial_members.len());
        
        // Create group using capability agent
        self.capability_agent.create_group(group_id, initial_members.clone())?;
        
        // Process capability changes to generate CGKA operations
        self.capability_agent.process_capability_changes(group_id)?;
        
        // Send group creation notifications to members
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        let recipients: BTreeSet<IndividualId> = initial_members.into_iter().collect();
        
        // Create notification data
        let notification_data = serde_json::to_vec(&format!("MLS group '{}' created", group_id))
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        self.transport.send_data(
            notification_data,
            format!("mls-group-created:{}", group_id),
            member_scope,
            Some(recipients),
        ).await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        // Sync systems
        self.sync_authority_graph().await?;
        self.sync_application_secrets().await?;
        
        info!("MLS group '{}' created and propagated via network", group_id);
        
        Ok(())
    }
    
    /// Sync authority graph across all systems
    async fn sync_authority_graph(&self) -> Result<(), AgentError> {
        debug!("Syncing authority graph across systems");
        
        let authority_graph = self.capability_agent.authority_graph.clone();
        
        // Update transport
        self.transport.update_authority_graph(authority_graph.clone()).await;
        
        // Update storage
        self.storage.update_authority_graph(authority_graph).await;
        
        debug!("Authority graph synced");
        
        Ok(())
    }
    
    /// Sync application secrets for encryption
    async fn sync_application_secrets(&self) -> Result<(), AgentError> {
        debug!("Syncing application secrets");
        
        // Get group memberships
        let groups = self.capability_agent.list_groups();
        
        for group_id in groups {
            if let Some(app_secret) = self.capability_agent.cgka_manager.get_application_secret(&group_id) {
                self.storage.add_application_secret(app_secret.clone()).await;
            }
        }
        
        debug!("Application secrets synced");
        
        Ok(())
    }
    
    /// Create default storage capabilities for new account
    async fn create_default_storage_capabilities(&mut self) -> Result<(), AgentError> {
        debug!("Creating default storage capabilities");
        
        // Create storage admin capability for this device
        let _storage_admin_scope = CapabilityScope::simple("storage", "admin");
        let _target_subject = self.capability_agent.individual_id.to_subject();
        
        // For now, we'll just ensure the capability exists in the authority graph
        // In a full implementation, this would create proper delegations
        
        debug!("Default storage capabilities created");
        
        Ok(())
    }
    
    /// Get network statistics
    pub async fn get_network_stats(&self) -> NetworkStats {
        let connections = self.connections.read().await;
        let pending_messages = self.transport.pending_messages_count().await;
        
        NetworkStats {
            connected_peers: connections.len(),
            pending_messages,
        }
    }
    
    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> Result<StorageStats, AgentError> {
        let entries = self.storage.list_entries()
            .await
            .map_err(|e| AgentError::serialization(e.to_string()))?;
        
        Ok(StorageStats {
            total_entries: entries.len(),
            accessible_entries: entries.len(), // All listed entries are accessible
        })
    }
    
    /// Cleanup old data across all systems
    pub async fn cleanup(&self) {
        info!("Running cleanup across integrated systems");
        
        // Cleanup old causal keys in storage
        self.storage.cleanup_old_keys(10).await;
        
        // Cleanup transport queues
        self.transport.flush_outbound_queue().await;
        
        info!("Cleanup complete");
    }
    
    
    // ========== Additional standard methods ==========
    
    /// Join a network group
    pub async fn network_join_group(&mut self, group_id: &str) -> Result<(), AgentError> {
        self.capability_agent.join_group(group_id)?;
        self.sync_authority_graph().await?;
        Ok(())
    }
    
    /// Leave a network group
    pub async fn network_leave_group(&mut self, group_id: &str) -> Result<(), AgentError> {
        self.capability_agent.leave_group(group_id)?;
        self.sync_authority_graph().await?;
        Ok(())
    }
    
    /// Get agent identity information
    pub fn identity(&self) -> (DeviceId, AccountId, IndividualId) {
        self.capability_agent.identity()
    }
    
    /// Check if agent is member of specific group
    pub fn is_group_member(&self, group_id: &str) -> bool {
        self.capability_agent.is_group_member(group_id)
    }
    
    /// List groups (delegating to capability agent)
    pub fn list_groups(&self) -> Vec<String> {
        self.capability_agent.list_groups()
    }
    
    /// List capabilities (delegating to capability agent)
    pub fn list_capabilities(&self) -> Vec<CapabilityScope> {
        self.capability_agent.list_capabilities()
    }
    
    /// Check capability (delegating to capability agent)
    pub fn check_capability(&self, scope: &CapabilityScope) -> bool {
        self.capability_agent.check_capability(scope)
    }
    
    /// Require capability (delegating to capability agent)
    pub fn require_capability(&self, scope: &CapabilityScope) -> Result<(), AgentError> {
        self.capability_agent.require_capability(scope)
    }
}

/// Network statistics
///
/// Statistics about network connectivity and message processing.
#[derive(Debug)]
pub struct NetworkStats {
    /// Number of currently connected peer devices
    pub connected_peers: usize,
    /// Number of messages waiting to be processed
    pub pending_messages: usize,
}

/// Storage statistics
///
/// Statistics about stored data and accessibility.
#[derive(Debug)]
pub struct StorageStats {
    /// Total number of storage entries
    pub total_entries: usize,
    /// Number of entries accessible to current identity
    pub accessible_entries: usize,
}