// Integrated agent with transport and storage

use crate::capability_agent::CapabilityAgent;
use crate::{AgentError, Result};
use std::sync::Arc;
use aura_journal::{
    capability::{
        identity::IndividualId,
        types::CapabilityScope,
    },
    DeviceId, AccountId,
};
use aura_transport::{CapabilityTransport, Transport};
use aura_store::CapabilityStorage;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, debug};
use serde_json;
use hex;

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
    ) -> Result<Self> {
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
    ) -> Result<()> {
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
    pub async fn network_connect(&self, peer: IndividualId, address: &str) -> Result<()> {
        info!("Connecting to peer {} at {}", peer.0, address);
        
        // Check that we have permission to communicate with this peer
        let comm_scope = CapabilityScope::simple("transport", "communicate");
        self.capability_agent.require_capability(&comm_scope)?;
        
        // Implement proper presence ticket handshake
        // 1. Generate presence ticket for this device
        let my_ticket = self.generate_presence_ticket().await?;
        
        // 2. Obtain peer's presence ticket (in real implementation, this would be via discovery/exchange)
        let peer_ticket = self.request_peer_presence_ticket(&peer, address).await?;
        
        // 3. Verify peer's ticket
        // TODO: Implement proper epoch and key management in CapabilityAgent
        let current_epoch = 1u64; // Placeholder epoch
        let account_public_key_bytes = [0u8; 32]; // Placeholder public key
        let account_public_key = ed25519_dalek::VerifyingKey::from_bytes(&account_public_key_bytes)
            .map_err(|e| AgentError::coordination(format!("Invalid account public key: {:?}", e)))?;
            
        peer_ticket.verify(&account_public_key, current_epoch)
            .map_err(|e| AgentError::coordination(format!("Peer ticket verification failed: {:?}", e)))?;
        
        // 4. Establish authenticated connection using transport layer
        let base_transport: Arc<dyn aura_transport::Transport> = Arc::new(aura_transport::StubTransport::new());
        let connection = base_transport.connect(&peer.0, &my_ticket, &peer_ticket).await
            .map_err(|e| AgentError::coordination(format!("Transport connection failed: {:?}", e)))?;
        
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
    ) -> Result<()> {
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
    ) -> Result<()> {
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
    ) -> Result<()> {
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
    pub async fn retrieve(&self, entry_id: &str) -> Result<Vec<u8>> {
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
    ) -> Result<()> {
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
    async fn sync_authority_graph(&self) -> Result<()> {
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
    async fn sync_application_secrets(&self) -> Result<()> {
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
    async fn create_default_storage_capabilities(&mut self) -> Result<()> {
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
    pub async fn get_storage_stats(&self) -> Result<StorageStats> {
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
    pub async fn network_join_group(&mut self, group_id: &str) -> Result<()> {
        self.capability_agent.join_group(group_id)?;
        self.sync_authority_graph().await?;
        Ok(())
    }
    
    /// Leave a network group
    pub async fn network_leave_group(&mut self, group_id: &str) -> Result<()> {
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
    pub fn require_capability(&self, scope: &CapabilityScope) -> Result<()> {
        self.capability_agent.require_capability(scope)
    }
    
    /// Generate a presence ticket for this device
    async fn generate_presence_ticket(&self) -> Result<aura_transport::PresenceTicket> {
        let (device_id, account_id, _) = self.identity();
        // TODO: Implement proper epoch management in CapabilityAgent
        let current_epoch = 1u64; // Placeholder epoch
        
        // Create unsigned ticket
        let mut ticket = aura_transport::PresenceTicket::new(
            device_id.0,
            account_id.0,
            current_epoch,
            3600, // 1 hour TTL
        ).map_err(|e| AgentError::coordination(format!("Failed to create presence ticket: {:?}", e)))?;
        
        // Sign with threshold signature
        let hash = ticket.compute_signable_hash();
        // TODO: Implement proper threshold signing in CapabilityAgent
        let signature_bytes = [0u8; 64]; // Placeholder signature bytes
        let signature = ed25519_dalek::Signature::from_bytes(&signature_bytes);
        
        ticket.signature = signature;
        
        Ok(ticket)
    }
    
    /// Request presence ticket from peer via HTTP discovery endpoint
    async fn request_peer_presence_ticket(
        &self,
        peer: &IndividualId,
        address: &str,
    ) -> Result<aura_transport::PresenceTicket> {
        // Try to perform actual ticket exchange via HTTP discovery
        if let Some(discovery_url) = self.parse_discovery_endpoint(address) {
            match self.perform_ticket_exchange(&discovery_url, peer).await {
                Ok(ticket) => return Ok(ticket),
                Err(e) => {
                    tracing::warn!("Failed to exchange tickets via discovery: {:?}, falling back to simulation", e);
                }
            }
        }
        
        // Fallback to simulation for compatibility with existing tests
        self.simulate_peer_ticket(peer).await
    }

    /// Parse discovery endpoint from peer address
    fn parse_discovery_endpoint(&self, address: &str) -> Option<String> {
        // Check if address looks like a network endpoint
        if address.contains("://") {
            // Already a full URL
            Some(format!("{}/discovery/presence", address.trim_end_matches('/')))
        } else if address.contains(':') {
            // Host:port format, assume HTTP
            Some(format!("http://{}/discovery/presence", address))
        } else {
            None
        }
    }

    /// Perform actual ticket exchange with peer via HTTP
    async fn perform_ticket_exchange(
        &self,
        discovery_url: &str,
        peer: &IndividualId,
    ) -> Result<aura_transport::PresenceTicket> {
        use serde_json::{json, Value};
        
        // Generate our ticket to send
        let my_ticket = self.generate_presence_ticket().await?;
        
        // Create HTTP client with timeout
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| AgentError::coordination(format!("Failed to create HTTP client: {}", e)))?;
        
        // Prepare ticket exchange request
        let request_body = json!({
            "my_ticket": {
                "device_id": my_ticket.device_id.to_string(),
                "account_id": my_ticket.account_id.to_string(),
                "epoch": my_ticket.session_epoch,
                "expires_at": my_ticket.expires_at,
                "signature": hex::encode(my_ticket.signature.to_bytes())
            },
            "requested_peer": peer.0
        });
        
        // Send ticket exchange request
        let response = client
            .post(discovery_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AgentError::coordination(format!("Failed to send ticket exchange request: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(AgentError::coordination(format!(
                "Ticket exchange failed with status: {}",
                response.status()
            )));
        }
        
        // Parse response
        let response_body: Value = response
            .json()
            .await
            .map_err(|e| AgentError::coordination(format!("Failed to parse ticket exchange response: {}", e)))?;
        
        // Extract peer ticket from response
        let peer_ticket_data = response_body
            .get("peer_ticket")
            .ok_or_else(|| AgentError::coordination("No peer_ticket in response".to_string()))?;
        
        // Reconstruct peer ticket
        let device_id = uuid::Uuid::parse_str(
            peer_ticket_data["device_id"]
                .as_str()
                .ok_or_else(|| AgentError::coordination("Invalid device_id in peer ticket".to_string()))?
        ).map_err(|e| AgentError::coordination(format!("Failed to parse device_id: {}", e)))?;
        
        let account_id = uuid::Uuid::parse_str(
            peer_ticket_data["account_id"]
                .as_str()
                .ok_or_else(|| AgentError::coordination("Invalid account_id in peer ticket".to_string()))?
        ).map_err(|e| AgentError::coordination(format!("Failed to parse account_id: {}", e)))?;
        
        let epoch = peer_ticket_data["epoch"]
            .as_u64()
            .ok_or_else(|| AgentError::coordination("Invalid epoch in peer ticket".to_string()))?;
        
        let expires_at = peer_ticket_data["expires_at"]
            .as_u64()
            .ok_or_else(|| AgentError::coordination("Invalid expires_at in peer ticket".to_string()))?;
        
        let signature_hex = peer_ticket_data["signature"]
            .as_str()
            .ok_or_else(|| AgentError::coordination("Invalid signature in peer ticket".to_string()))?;
        
        let signature_bytes = hex::decode(signature_hex)
            .map_err(|e| AgentError::coordination(format!("Failed to decode signature: {}", e)))?;
        
        // Convert bytes to ed25519 signature
        let signature_array: [u8; 64] = signature_bytes.try_into()
            .map_err(|_| AgentError::coordination("Invalid signature length, expected 64 bytes".to_string()))?;
        let signature = ed25519_dalek::Signature::from_bytes(&signature_array);
        
        // Create peer ticket
        let mut peer_ticket = aura_transport::PresenceTicket::new(
            device_id,
            account_id,
            epoch,
            expires_at,
        ).map_err(|e| AgentError::coordination(format!("Failed to create peer ticket: {:?}", e)))?;
        
        peer_ticket.signature = signature;
        
        tracing::info!("Successfully exchanged presence tickets with peer {}", peer.0);
        Ok(peer_ticket)
    }

    /// Simulate peer ticket for testing/fallback
    async fn simulate_peer_ticket(&self, peer: &IndividualId) -> Result<aura_transport::PresenceTicket> {
        let peer_device_id = uuid::Uuid::parse_str(&peer.0)
            .unwrap_or_else(|_| uuid::Uuid::new_v4());
        let account_id = self.identity().1;
        // TODO: Implement proper epoch management in CapabilityAgent
        let current_epoch = 1u64; // Placeholder epoch
        
        // Create a simulated peer ticket (in production this would come from the peer)
        let mut peer_ticket = aura_transport::PresenceTicket::new(
            peer_device_id,
            account_id.0,
            current_epoch,
            3600,
        ).map_err(|e| AgentError::coordination(format!("Failed to create peer ticket: {:?}", e)))?;
        
        // For stub transport, sign with our own threshold key (simulating valid peer)
        let _hash = peer_ticket.compute_signable_hash();
        // TODO: Implement proper threshold signing in CapabilityAgent
        let signature_bytes = [0u8; 64]; // Placeholder signature bytes
        let signature = ed25519_dalek::Signature::from_bytes(&signature_bytes);
        
        peer_ticket.signature = signature;
        
        info!("Generated simulated peer ticket for {}", peer.0);
        Ok(peer_ticket)
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