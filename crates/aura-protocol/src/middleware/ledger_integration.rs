//! Ledger integration middleware that bridges Automerge ledger with event watcher
//!
//! This middleware provides bidirectional integration:
//! - Observes ledger changes through event watcher
//! - Applies operations to ledger through effect system
//! - Synchronizes state across devices

use super::event_watcher::{EventFilter, EventWatcherMiddleware};
use super::handler::{AuraProtocolHandler, ProtocolResult};
use aura_journal::automerge::{
    AutomergeLedgerHandler, LedgerEffect, AutomergeOperation, AutomergeActorId,
    AutomergeSyncProtocol, SyncMessage, LedgerValue,
};
use aura_types::DeviceId;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Configuration for ledger integration
#[derive(Debug, Clone)]
pub struct LedgerIntegrationConfig {
    /// Device name for logging
    pub device_name: String,
    /// Whether to auto-sync with peers
    pub auto_sync: bool,
    /// Sync interval
    pub sync_interval: Duration,
    /// Whether to start event watching immediately
    pub auto_start: bool,
}

impl Default for LedgerIntegrationConfig {
    fn default() -> Self {
        Self {
            device_name: "ledger_integration".to_string(),
            auto_sync: true,
            sync_interval: Duration::from_secs(5),
            auto_start: true,
        }
    }
}

/// Ledger integration middleware
pub struct LedgerIntegrationMiddleware<H> {
    inner: H,
    device_id: DeviceId,
    ledger_handler: Arc<RwLock<AutomergeLedgerHandler>>,
    event_watcher: Arc<RwLock<EventWatcherMiddleware<H>>>,
    sync_protocol: Arc<RwLock<AutomergeSyncProtocol>>,
    config: LedgerIntegrationConfig,
}

impl<H> LedgerIntegrationMiddleware<H>
where
    H: AuraProtocolHandler<DeviceId = uuid::Uuid> + Clone + Send + 'static,
{
    /// Create new ledger integration middleware
    pub async fn new(
        inner: H,
        device_id: DeviceId,
        ledger_handler: Arc<RwLock<AutomergeLedgerHandler>>,
        config: LedgerIntegrationConfig,
    ) -> Self {
        // Get ledger for event watcher (temporary hack - need better abstraction)
        let ledger = {
            // Create a temporary AccountLedger wrapper for event watcher
            // In production, this would be properly integrated
            let handler = ledger_handler.read().await;
            let effect = LedgerEffect::GetDevices;
            let _devices = handler.handle(effect).await.ok();
            
            // For now, create a dummy ledger
            let effects = aura_crypto::Effects::test(42);
            let account_id = aura_types::AccountId::new_with_effects(&effects);
            let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
            let device = aura_journal::DeviceMetadata {
                device_id: device_id.into(),
                device_name: config.device_name.clone(),
                device_type: aura_journal::DeviceType::Native,
                public_key: signing_key.verifying_key(),
                added_at: 1000,
                last_seen: 1000,
                dkd_commitment_proofs: std::collections::BTreeMap::new(),
                next_nonce: 0,
                used_nonces: std::collections::BTreeSet::new(),
                key_share_epoch: 0,
            };
            let state = aura_journal::AccountState::new(
                account_id,
                signing_key.verifying_key(),
                device,
                2,
                3,
            );
            Arc::new(RwLock::new(aura_journal::AccountLedger::new(state).unwrap()))
        };
        
        // Create event watcher
        let event_watcher = EventWatcherMiddleware::new(
            inner.clone(),
            config.device_name.clone(),
            ledger,
        );
        
        // Register callbacks for protocol coordination
        let handler_clone = ledger_handler.clone();
        let device_id_clone = device_id.clone();
        event_watcher.register_callback(
            EventFilter::Type(super::event_watcher::EventTypeFilter::GrantOperationLock),
            Arc::new(move |event| {
                let handler = handler_clone.clone();
                let device_id = device_id_clone.clone();
                tokio::spawn(async move {
                    // React to lock grant by checking if we should start a protocol
                    debug!(
                        device = %device_id,
                        event_id = ?event.event_id,
                        "Operation lock granted, checking protocol state"
                    );
                });
                true // Continue processing other callbacks
            }),
        ).await;
        
        // Create sync protocol
        let automerge_state = {
            // TODO: Get actual Automerge state from ledger handler
            // For now, this is a placeholder
            Arc::new(RwLock::new(
                aura_journal::automerge::AutomergeAccountState::new(
                    aura_types::AccountId::new_with_effects(&aura_crypto::Effects::test(42)),
                    aura_crypto::Ed25519SigningKey::from_bytes(&aura_crypto::Effects::test(42).random_bytes::<32>()).verifying_key(),
                ).unwrap()
            ))
        };
        
        let sync_protocol = AutomergeSyncProtocol::new(
            device_id.into(),
            automerge_state,
        );
        
        Self {
            inner,
            device_id: device_id.into(),
            ledger_handler,
            event_watcher: Arc::new(RwLock::new(event_watcher)),
            sync_protocol: Arc::new(RwLock::new(sync_protocol)),
            config,
        }
    }
    
    /// Apply a ledger operation and wait for it to be observed
    pub async fn apply_operation(&mut self, op: AutomergeOperation) -> ProtocolResult<()> {
        info!(
            device = %self.device_id,
            operation = ?op,
            "Applying ledger operation"
        );
        
        // Get current event count
        let last_index = self.event_watcher.read().await
            .last_processed_index().await;
        
        // Apply operation through effect handler
        let effect = LedgerEffect::ApplyOperation {
            op: op.clone(),
            actor_id: self.device_id.into(),
        };
        
        let mut handler = self.ledger_handler.write().await;
        match handler.handle(effect).await {
            Ok(LedgerValue::Changes(changes)) => {
                debug!(
                    device = %self.device_id,
                    changes = changes.len(),
                    "Applied operation, got changes"
                );
            }
            Err(e) => {
                return Err(crate::middleware::handler::ProtocolError::Protocol {
                    message: format!("Failed to apply operation: {}", e),
                });
            }
            _ => {}
        }
        
        // Wait for event watcher to observe the change
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        while self.event_watcher.read().await.last_processed_index().await == last_index {
            if tokio::time::Instant::now() >= deadline {
                debug!(
                    device = %self.device_id,
                    "Timed out waiting for event observation"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        
        Ok(())
    }
    
    /// Sync with a peer
    pub async fn sync_with_peer(&mut self, peer: DeviceId) -> ProtocolResult<()> {
        info!(
            device = %self.device_id,
            peer = %peer,
            "Syncing with peer"
        );
        
        // Generate sync message
        let sync_msg = self.sync_protocol.read().await
            .generate_sync_message(peer.into())
            .await
            .map_err(|e| crate::middleware::handler::ProtocolError::Protocol {
                message: format!("Failed to generate sync message: {}", e),
            })?;
        
        // In a real implementation, this would send over network
        // For now, just log
        debug!(
            device = %self.device_id,
            peer = %peer,
            message_size = sync_msg.automerge_message.len(),
            epoch = sync_msg.epoch,
            "Generated sync message"
        );
        
        Ok(())
    }
    
    /// Process incoming sync message
    pub async fn receive_sync(&mut self, msg: SyncMessage) -> ProtocolResult<()> {
        info!(
            device = %self.device_id,
            from = %msg.from_device,
            epoch = msg.epoch,
            "Receiving sync message"
        );
        
        let result = self.sync_protocol.write().await
            .receive_sync_message(msg)
            .await
            .map_err(|e| crate::middleware::handler::ProtocolError::Protocol {
                message: format!("Failed to receive sync message: {}", e),
            })?;
        
        debug!(
            device = %self.device_id,
            changes_applied = result.changes_applied,
            new_epoch = result.new_epoch,
            "Sync complete"
        );
        
        Ok(())
    }
    
    /// Start background sync task
    pub fn start_sync_task(&self) {
        if !self.config.auto_sync {
            return;
        }
        
        let device_id = self.device_id;
        let sync_protocol = self.sync_protocol.clone();
        let interval = self.config.sync_interval;
        
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            
            loop {
                interval_timer.tick().await;
                
                // In a real implementation, this would discover peers and sync
                debug!(
                    device = %device_id,
                    "Sync task heartbeat"
                );
            }
        });
    }
}

// Implement protocol handler by delegating to inner
#[async_trait]
impl<H> AuraProtocolHandler for LedgerIntegrationMiddleware<H>
where
    H: AuraProtocolHandler + Send,
    H::DeviceId: Send + Sync,
    H::SessionId: Send + Sync,
    H::Message: Send + Sync,
{
    type DeviceId = H::DeviceId;
    type SessionId = H::SessionId;
    type Message = H::Message;
    
    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        self.inner.send_message(to, msg).await
    }
    
    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message> {
        self.inner.receive_message(from).await
    }
    
    async fn broadcast(&mut self, recipients: &[Self::DeviceId], msg: Self::Message) -> ProtocolResult<()> {
        self.inner.broadcast(recipients, msg).await
    }
    
    async fn parallel_send(&mut self, sends: &[(Self::DeviceId, Self::Message)]) -> ProtocolResult<()> {
        self.inner.parallel_send(sends).await
    }
    
    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: std::collections::HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId> {
        self.inner.start_session(participants, protocol_type, metadata).await
    }
    
    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()> {
        self.inner.end_session(session_id).await
    }
    
    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<super::handler::SessionInfo> {
        self.inner.get_session_info(session_id).await
    }
    
    async fn list_sessions(&mut self) -> ProtocolResult<Vec<super::handler::SessionInfo>> {
        self.inner.list_sessions().await
    }
    
    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: std::collections::HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        self.inner.verify_capability(operation, resource, context).await
    }
    
    async fn create_authorization_proof(
        &mut self,
        operation: &str,
        resource: &str,
        context: std::collections::HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>> {
        self.inner.create_authorization_proof(operation, resource, context).await
    }
    
    fn device_id(&self) -> Self::DeviceId {
        self.inner.device_id()
    }
    
    async fn setup(&mut self) -> ProtocolResult<()> {
        // Setup inner handler
        self.inner.setup().await?;
        
        // Start event watching if configured
        if self.config.auto_start {
            self.event_watcher.write().await.start_watching().await;
        }
        
        // Start sync task
        self.start_sync_task();
        
        Ok(())
    }
    
    async fn teardown(&mut self) -> ProtocolResult<()> {
        // Stop event watching
        self.event_watcher.write().await.stop_watching().await;
        
        // Teardown inner handler
        self.inner.teardown().await
    }
}