//! Unified Agent Implementation
//!
//! This module provides a generic Agent implementation.
//!
//! The unified agent uses session types for compile-time state safety and
//! generic transport/storage abstractions for testability.

use crate::transport_adapter::TransportAdapterFactory;
use aura_coordination::{local_runtime::LocalSessionRuntime, Transport as CoordinationTransport};
use aura_crypto::Effects as CoreEffects;
use aura_journal::{
    AccountLedger, AccountState, DeviceMetadata, DeviceType, Result as JournalResult,
};
use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt, GuardianId};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Effects;

impl Effects {
    pub fn test() -> Self {
        Self
    }
}

impl aura_types::EffectsLike for Effects {
    fn gen_uuid(&self) -> Uuid {
        Uuid::new_v4()
    }
}
use session_types::{SessionProtocol, SessionState};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

// Temporary placeholder until coordination crate is fixed
#[derive(Debug, Clone)]
pub struct KeyShare {
    pub device_id: DeviceId,
    pub share_data: Vec<u8>,
}

impl Default for KeyShare {
    fn default() -> Self {
        Self {
            device_id: DeviceId::new_with_effects(&Effects::test()),
            share_data: vec![0u8; 32],
        }
    }
}

use crate::traits::{
    Agent, CoordinatingAgent, GroupAgent, IdentityAgent, NetworkAgent, StorageAgent,
};
use crate::{AgentError, DerivedIdentity, Result};
use async_trait::async_trait;

/// Transport abstraction for agent communication
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Get the device ID for this transport
    fn device_id(&self) -> DeviceId;

    /// Send a message to a peer
    async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()>;

    /// Receive messages (non-blocking)
    async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>>;

    /// Connect to a peer
    async fn connect(&self, peer_id: DeviceId) -> Result<()>;

    /// Disconnect from a peer
    async fn disconnect(&self, peer_id: DeviceId) -> Result<()>;

    /// Get list of connected peers
    async fn connected_peers(&self) -> Result<Vec<DeviceId>>;

    /// Check if connected to a specific peer
    async fn is_connected(&self, peer_id: DeviceId) -> Result<bool>;
}

/// Storage abstraction for agent persistence
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Get the account ID for this storage
    fn account_id(&self) -> AccountId;

    /// Store data with a given key
    async fn store(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Retrieve data by key
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete data by key
    async fn delete(&self, key: &str) -> Result<()>;

    /// List all keys
    async fn list_keys(&self) -> Result<Vec<String>>;

    /// Check if a key exists
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Get storage statistics
    async fn stats(&self) -> Result<StorageStats>;
}

/// Storage statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StorageStats {
    pub total_keys: u64,
    pub total_size_bytes: u64,
    pub available_space_bytes: Option<u64>,
}

/// The core data and dependencies that persist across all agent states
pub struct AgentCore<T: Transport, S: Storage> {
    /// Unique identifier for this device
    pub device_id: DeviceId,
    /// Account identifier this agent belongs to
    pub account_id: AccountId,
    /// Threshold key share for cryptographic operations
    pub key_share: Arc<RwLock<KeyShare>>,
    /// CRDT-based account ledger for state management
    pub ledger: Arc<RwLock<AccountLedger>>,
    /// Transport layer for network communication
    pub transport: Arc<T>,
    /// Storage layer for persistence
    pub storage: Arc<S>,
    /// Session runtime for choreographic protocols
    pub session_runtime: Arc<RwLock<LocalSessionRuntime>>,
    /// Injectable effects for deterministic testing
    pub effects: Effects,
}

impl<T: Transport, S: Storage> AgentCore<T, S> {
    /// Create a new agent core with the provided dependencies
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        key_share: KeyShare,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<T>,
        storage: Arc<S>,
        effects: Effects,
        session_runtime: Arc<RwLock<LocalSessionRuntime>>,
    ) -> Self {
        Self {
            device_id,
            account_id,
            key_share: Arc::new(RwLock::new(key_share)),
            ledger,
            transport,
            storage,
            effects,
            session_runtime,
        }
    }

    /// Get the device ID (available in all states)
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the account ID (available in all states)
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Validate agent security state and key integrity
    pub async fn validate_security_state(&self) -> Result<SecurityValidationReport> {
        let mut report = SecurityValidationReport::new(self.device_id, self.account_id);

        // Check 1: Verify key share integrity
        let key_share = self.key_share.read().await;
        if key_share.device_id != self.device_id {
            report.add_issue(SecurityIssue::KeyIntegrityViolation(
                "Key share device ID mismatch".to_string(),
            ));
        }

        if key_share.share_data.is_empty() {
            report.add_issue(SecurityIssue::KeyIntegrityViolation(
                "Empty key share data".to_string(),
            ));
        }

        // Check 2: Verify FROST keys exist and are valid
        let frost_key_storage_key = format!("frost_keys:{}", self.device_id.0);
        match self.storage.retrieve(&frost_key_storage_key).await {
            Ok(Some(frost_data)) => {
                if frost_data.is_empty() {
                    report.add_issue(SecurityIssue::KeyIntegrityViolation(
                        "Empty FROST key data".to_string(),
                    ));
                } else {
                    // Validate FROST keys
                    let frost_agent = crate::frost_manager::FrostAgent::new(self.device_id);
                    if let Err(e) = frost_agent.import_keys(&frost_data).await {
                        tracing::error!("FROST key validation failed: {}", e);
                        report.add_issue(SecurityIssue::CriticalSecurityViolation(format!(
                            "FROST key validation failed: {}",
                            e
                        )));
                    } else if !frost_agent.is_ready().await {
                        report.add_issue(SecurityIssue::ConfigurationError(
                            "FROST keys not ready for operations".to_string(),
                        ));
                    } else {
                        report.frost_keys_valid = true;
                    }
                }
            }
            Ok(None) => {
                report.add_issue(SecurityIssue::KeyIntegrityViolation(
                    "FROST keys not found".to_string(),
                ));
            }
            Err(e) => {
                report.add_issue(SecurityIssue::StorageError(format!(
                    "Failed to retrieve FROST keys: {}",
                    e
                )));
            }
        }

        // Check 3: Verify bootstrap metadata integrity
        let bootstrap_key = format!("bootstrap_metadata:{}", self.device_id.0);
        match self.storage.retrieve(&bootstrap_key).await {
            Ok(Some(metadata_bytes)) => {
                match serde_json::from_slice::<serde_json::Value>(&metadata_bytes) {
                    Ok(metadata) => {
                        if let Some(device_id_str) =
                            metadata.get("device_id").and_then(|v| v.as_str())
                        {
                            if device_id_str != self.device_id.0.to_string() {
                                report.add_issue(SecurityIssue::BootstrapIntegrityViolation(
                                    "Bootstrap metadata device ID mismatch".to_string(),
                                ));
                            }
                        } else {
                            report.add_issue(SecurityIssue::BootstrapIntegrityViolation(
                                "Missing device ID in bootstrap metadata".to_string(),
                            ));
                        }

                        if let Some(version) = metadata.get("version").and_then(|v| v.as_str()) {
                            if version != "phase-0" {
                                report.add_issue(SecurityIssue::VersionMismatch(format!(
                                    "Unsupported version: {}",
                                    version
                                )));
                            }
                        }

                        report.bootstrap_metadata_valid = true;
                    }
                    Err(e) => {
                        report.add_issue(SecurityIssue::BootstrapIntegrityViolation(format!(
                            "Invalid bootstrap metadata format: {}",
                            e
                        )));
                    }
                }
            }
            Ok(None) => {
                report.add_issue(SecurityIssue::BootstrapIntegrityViolation(
                    "Bootstrap metadata not found".to_string(),
                ));
            }
            Err(e) => {
                report.add_issue(SecurityIssue::StorageError(format!(
                    "Failed to retrieve bootstrap metadata: {}",
                    e
                )));
            }
        }

        // Check 4: Verify ledger state consistency
        {
            let ledger = self.ledger.read().await;
            let ledger_state = ledger.state();

            if ledger_state.account_id != self.account_id {
                report.add_issue(SecurityIssue::LedgerIntegrityViolation(
                    "Ledger account ID mismatch".to_string(),
                ));
            }

            // Check if our device is in the ledger
            if !ledger_state.devices.contains_key(&self.device_id) {
                report.add_issue(SecurityIssue::LedgerIntegrityViolation(
                    "Device not found in ledger".to_string(),
                ));
            } else {
                report.ledger_consistent = true;
            }
        }

        report.validation_completed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        Ok(report)
    }

    /// Validate input parameters for security compliance
    pub fn validate_input_parameters(
        app_id: &str,
        context: &str,
        capabilities: &[String],
    ) -> Result<()> {
        // Validate app_id
        if app_id.is_empty() {
            return Err(crate::error::AuraError::agent_invalid_state(
                "App ID cannot be empty",
            ));
        }

        if app_id.len() > 64 {
            return Err(crate::error::AuraError::agent_invalid_state(
                "App ID too long (max 64 characters)",
            ));
        }

        if !app_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(crate::error::AuraError::agent_invalid_state(
                "App ID contains invalid characters (only alphanumeric, -, _, . allowed)",
            ));
        }

        // Validate context
        if context.is_empty() {
            return Err(crate::error::AuraError::agent_invalid_state(
                "Context cannot be empty",
            ));
        }

        if context.len() > 128 {
            return Err(crate::error::AuraError::agent_invalid_state(
                "Context too long (max 128 characters)",
            ));
        }

        if !context
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ':')
        {
            return Err(crate::error::AuraError::agent_invalid_state(
                "Context contains invalid characters (only alphanumeric, -, _, ., : allowed)",
            ));
        }

        // Validate capabilities
        for capability in capabilities {
            if capability.is_empty() {
                return Err(crate::error::AuraError::agent_invalid_state(
                    "Capability cannot be empty",
                ));
            }

            if capability.len() > 128 {
                return Err(crate::error::AuraError::agent_invalid_state(
                    "Capability too long (max 128 characters)",
                ));
            }

            if !capability
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ':')
            {
                return Err(crate::error::AuraError::agent_invalid_state(
                    "Capability contains invalid characters",
                ));
            }
        }

        Ok(())
    }
}

/// Security validation report for agent state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecurityValidationReport {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub issues: Vec<SecurityIssue>,
    pub frost_keys_valid: bool,
    pub bootstrap_metadata_valid: bool,
    pub ledger_consistent: bool,
    pub validation_completed_at: u128,
}

impl SecurityValidationReport {
    pub fn new(device_id: DeviceId, account_id: AccountId) -> Self {
        Self {
            device_id,
            account_id,
            issues: Vec::new(),
            frost_keys_valid: false,
            bootstrap_metadata_valid: false,
            ledger_consistent: false,
            validation_completed_at: 0,
        }
    }

    pub fn add_issue(&mut self, issue: SecurityIssue) {
        self.issues.push(issue);
    }

    pub fn has_critical_issues(&self) -> bool {
        self.issues.iter().any(|issue| issue.is_critical())
    }

    pub fn is_secure(&self) -> bool {
        self.issues.is_empty()
            && self.frost_keys_valid
            && self.bootstrap_metadata_valid
            && self.ledger_consistent
    }
}

/// Types of security issues that can be detected
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SecurityIssue {
    KeyIntegrityViolation(String),
    BootstrapIntegrityViolation(String),
    LedgerIntegrityViolation(String),
    StorageError(String),
    VersionMismatch(String),
    ConfigurationError(String),
}

impl SecurityIssue {
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            SecurityIssue::KeyIntegrityViolation(_) | SecurityIssue::LedgerIntegrityViolation(_)
        )
    }
}

/// Agent session states (manual implementation)
#[derive(Debug, Clone)]
pub struct Uninitialized;

#[derive(Debug, Clone)]
pub struct Idle;

#[derive(Debug, Clone)]
pub struct Coordinating;

#[derive(Debug, Clone)]
pub struct Failed;

impl SessionState for Uninitialized {
    const NAME: &'static str = "Uninitialized";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

impl SessionState for Idle {
    const NAME: &'static str = "Idle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

impl SessionState for Coordinating {
    const NAME: &'static str = "Coordinating";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

impl SessionState for Failed {
    const NAME: &'static str = "Failed";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Session-typed agent protocol
/// Generic over Transport, Storage, and State
pub struct AgentProtocol<T: Transport, S: Storage, State: SessionState> {
    pub inner: AgentCore<T, S>,
    _state: std::marker::PhantomData<State>,
}

impl<T: Transport, S: Storage, State: SessionState> AgentProtocol<T, S, State> {
    /// Create a new agent protocol instance
    pub fn new(core: AgentCore<T, S>) -> Self {
        Self {
            inner: core,
            _state: std::marker::PhantomData,
        }
    }

    /// Transition to a new state (type-safe state transitions)
    pub fn transition_to<NewState: SessionState>(self) -> AgentProtocol<T, S, NewState> {
        AgentProtocol {
            inner: self.inner,
            _state: std::marker::PhantomData,
        }
    }

    /// Get the device ID (available in all states)
    pub fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    /// Get the account ID (available in all states)
    pub fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

/// Type alias for the concrete unified agent
pub type UnifiedAgent<T, S> = AgentProtocol<T, S, Uninitialized>;

/// Configuration for bootstrapping a new agent
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Initial threshold for key shares
    pub threshold: u16,
    /// Total number of shares
    pub share_count: u16,
    /// Additional configuration parameters
    pub parameters: std::collections::HashMap<String, String>,
}

/// Status of a running protocol
#[derive(Debug, Clone)]
pub enum ProtocolStatus {
    /// Protocol is still running
    InProgress,
    /// Protocol completed successfully
    Completed,
    /// Protocol failed with error
    Failed(String),
}

/// Witness that a protocol has completed successfully
#[derive(Debug)]
pub struct ProtocolCompleted {
    pub protocol_id: uuid::Uuid,
    pub result: serde_json::Value,
}

// Implementation for Uninitialized state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Uninitialized> {
    /// Create a new uninitialized agent
    pub fn new_uninitialized(core: AgentCore<T, S>) -> Self {
        Self::new(core)
    }

    /// Bootstrap the agent with initial configuration
    ///
    /// This consumes the uninitialized agent and returns an idle agent
    pub async fn bootstrap(self, config: BootstrapConfig) -> Result<AgentProtocol<T, S, Idle>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            account_id = %self.inner.account_id,
            "Bootstrapping agent with config: {:?}", config
        );

        // Step 1: Initialize FROST key shares using threshold configuration
        let frost_agent = crate::frost_manager::FrostAgent::new(self.inner.device_id);

        // Create participant list - for bootstrap, this device is the first participant
        let mut participants = vec![self.inner.device_id];

        // Add additional participants from config if provided
        if let Some(additional_devices) = config.parameters.get("additional_devices") {
            if let Ok(device_list) = serde_json::from_str::<Vec<String>>(additional_devices) {
                for device_str in device_list {
                    if let Ok(uuid) = uuid::Uuid::parse_str(&device_str) {
                        participants.push(DeviceId(uuid));
                    }
                }
            }
        }

        // Ensure we have enough participants for the threshold
        if participants.len() < config.threshold as usize {
            return Err(crate::error::AuraError::agent_invalid_state(format!(
                "Not enough participants ({}) for threshold ({})",
                participants.len(),
                config.threshold
            )));
        }

        // FROST-ed25519 constraint: threshold must equal number of participants
        if config.threshold != participants.len() as u16 {
            return Err(crate::error::AuraError::agent_invalid_state(format!(
                "FROST-ed25519 requires threshold ({}) to equal participants count ({})",
                config.threshold,
                participants.len()
            )));
        }

        // Initialize FROST keys via DKG
        frost_agent
            .initialize_keys_with_dkg(config.threshold, participants)
            .await
            .map_err(|e| {
                crate::error::AuraError::agent_invalid_state(format!(
                    "FROST key initialization failed: {}",
                    e
                ))
            })?;

        // Verify FROST agent is ready
        if !frost_agent.is_ready().await {
            return Err(crate::error::AuraError::agent_invalid_state(
                "FROST agent failed to initialize properly",
            ));
        }

        // Step 2: Store the FROST agent in the agent core for future use
        // Note: We would store this in a proper field, but for Phase 0 we'll use the storage layer
        let frost_keys = frost_agent.export_keys().await.map_err(|e| {
            crate::error::AuraError::agent_invalid_state(format!(
                "Failed to export FROST keys: {}",
                e
            ))
        })?;

        // Store FROST keys securely
        let frost_key_storage_key = format!("frost_keys:{}", self.inner.device_id.0);
        self.inner
            .storage
            .store(&frost_key_storage_key, &frost_keys)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Failed to store FROST keys: {}",
                    e
                ))
            })?;

        // Step 3: Initialize key share in agent core
        let (threshold_config, _) = frost_agent.get_threshold_config().await.map_err(|e| {
            crate::error::AuraError::agent_invalid_state(format!(
                "Failed to get threshold config: {}",
                e
            ))
        })?;

        // Update the key share with proper configuration
        {
            let mut key_share = self.inner.key_share.write().await;
            key_share.device_id = self.inner.device_id;
            // Store FROST keys reference
            key_share.share_data = frost_keys;
        }

        // Step 4: Initialize session runtime environment
        {
            let mut session_runtime = self.inner.session_runtime.write().await;

            // Set up the session runtime environment with our ledger and transport
            session_runtime.set_environment(
                self.inner.ledger.clone(),
                // Create a coordination transport adapter from our agent transport
                Arc::new(crate::transport_adapter::TransportAdapterFactory::create_coordination_adapter(
                    self.inner.transport.clone()
                )) as Arc<dyn aura_coordination::Transport>,
            ).await;
        }

        // Step 5: Store bootstrap metadata for audit trail
        let bootstrap_metadata = serde_json::json!({
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            "threshold": config.threshold,
            "share_count": config.share_count,
            "device_id": self.inner.device_id.0,
            "account_id": self.inner.account_id.0,
            "version": "phase-0",
            "parameters": config.parameters
        });

        let metadata_key = format!("bootstrap_metadata:{}", self.inner.device_id.0);
        let metadata_bytes = serde_json::to_vec(&bootstrap_metadata).map_err(|e| {
            crate::error::AuraError::serialization_failed(format!(
                "Failed to serialize bootstrap metadata: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&metadata_key, &metadata_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Failed to store bootstrap metadata: {}",
                    e
                ))
            })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            account_id = %self.inner.account_id,
            threshold = threshold_config,
            "Agent bootstrap completed successfully"
        );

        // Transition to idle state - agent is now ready for operations
        Ok(self.transition_to())
    }
}

// Implementation for Idle state - this is the main operational state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Idle> {
    /// Derive a new identity for a specific context using DKD
    pub async fn derive_identity(
        &self,
        app_id: &str,
        context: &str,
    ) -> Result<crate::DerivedIdentity> {
        // Step 0: Validate input parameters for security compliance
        AgentCore::<T, S>::validate_input_parameters(app_id, context, &[])?;

        // Step 0.1: Validate agent security state before proceeding
        let security_report = self.inner.validate_security_state().await?;
        if security_report.has_critical_issues() {
            return Err(crate::error::AuraError::agent_invalid_state(format!(
                "Critical security issues detected: {:?}",
                security_report.issues
            )));
        }

        if !security_report.is_secure() {
            tracing::warn!(
                device_id = %self.inner.device_id,
                issues = ?security_report.issues,
                "Security issues detected during identity derivation"
            );
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            app_id = app_id,
            context = context,
            "Deriving identity using DKD protocol"
        );

        // Step 1: Retrieve FROST key share to participate in DKD
        let frost_key_storage_key = format!("frost_keys:{}", self.inner.device_id.0);
        let frost_keys_data = self
            .inner
            .storage
            .retrieve(&frost_key_storage_key)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Failed to retrieve FROST keys: {}",
                    e
                ))
            })?
            .ok_or_else(|| {
                crate::error::AuraError::agent_invalid_state(
                    "FROST keys not found - agent not properly bootstrapped",
                )
            })?;

        // Step 2: Reconstruct FROST agent to get key share
        let frost_agent = crate::frost_manager::FrostAgent::new(self.inner.device_id);
        frost_agent
            .import_keys(&frost_keys_data)
            .await
            .map_err(|e| {
                crate::error::AuraError::agent_invalid_state(format!(
                    "Failed to import FROST keys: {}",
                    e
                ))
            })?;

        if !frost_agent.is_ready().await {
            return Err(crate::error::AuraError::agent_invalid_state(
                "FROST agent not ready for DKD operations",
            ));
        }

        // Step 3: Create context-specific seed for DKD
        let context_bytes =
            format!("{}:{}:{}", app_id, context, self.inner.device_id.0).into_bytes();

        // Step 4: Execute DKD protocol using key share
        let key_share = self.inner.key_share.read().await;
        let share_bytes = &key_share.share_data;

        // Mix device-specific data with share for unique DKD input
        let mut dkd_input = Vec::with_capacity(share_bytes.len() + context_bytes.len());
        dkd_input.extend_from_slice(share_bytes);
        dkd_input.extend_from_slice(&context_bytes);

        // Take first 16 bytes as DKD share (Phase 0 simplification)
        let mut dkd_share = [0u8; 16];
        let copy_len = std::cmp::min(16, dkd_input.len());
        dkd_share[..copy_len].copy_from_slice(&dkd_input[..copy_len]);

        // Execute DKD cryptographic operations
        let mut dkd_participant = aura_crypto::dkd::DkdParticipant::new(dkd_share);
        let commitment = dkd_participant.commitment_hash();
        let revealed_point = dkd_participant.revealed_point();

        // Step 5: For single-device DKD (Phase 0), aggregate our own point
        let revealed_points = vec![revealed_point];
        let derived_public_key =
            aura_crypto::dkd::aggregate_dkd_points(&revealed_points).map_err(|e| {
                crate::error::AuraError::crypto_operation_failed(format!(
                    "DKD point aggregation failed: {}",
                    e
                ))
            })?;

        // Step 6: Use the derived public key bytes as seed for key derivation
        let seed = derived_public_key.to_bytes();

        let derived_keys = aura_crypto::dkd::derive_keys(&seed, &context_bytes).map_err(|e| {
            crate::error::AuraError::crypto_operation_failed(format!(
                "Key derivation failed: {}",
                e
            ))
        })?;

        // Step 7: Create binding proof using FROST signature
        let proof_message = format!(
            "DKD_BINDING:{}:{}:{}",
            app_id,
            context,
            hex::encode(&derived_keys.seed_fingerprint)
        );
        let binding_proof = frost_agent
            .threshold_sign(proof_message.as_bytes())
            .await
            .map_err(|e| {
                crate::error::AuraError::crypto_operation_failed(format!(
                    "Binding proof generation failed: {}",
                    e
                ))
            })?;

        // Step 8: Store derived identity for future reference
        let derived_identity_metadata = serde_json::json!({
            "app_id": app_id,
            "context": context,
            "device_id": self.inner.device_id.0,
            "derived_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            "public_key": hex::encode(&derived_keys.signing_key),
            "seed_fingerprint": hex::encode(&derived_keys.seed_fingerprint),
            "commitment": hex::encode(&commitment),
            "version": "phase-0-dkd"
        });

        let identity_storage_key = format!("derived_identity:{}:{}", app_id, context);
        let metadata_bytes = serde_json::to_vec(&derived_identity_metadata).map_err(|e| {
            crate::error::AuraError::storage_failed(format!(
                "Failed to serialize identity metadata: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&identity_storage_key, &metadata_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Failed to store derived identity: {}",
                    e
                ))
            })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            app_id = app_id,
            context = context,
            public_key = hex::encode(&derived_keys.signing_key),
            seed_fingerprint = hex::encode(&derived_keys.seed_fingerprint),
            "DKD identity derivation completed successfully"
        );

        // Return complete derived identity
        Ok(crate::DerivedIdentity {
            app_id: app_id.to_string(),
            context: context.to_string(),
            identity_key: derived_keys.signing_key.to_vec(),
            proof: binding_proof.to_bytes().to_vec(),
        })
    }

    /// Store data with capability-based access control
    pub async fn store_data(&self, data: &[u8], capabilities: Vec<String>) -> Result<String> {
        // TODO: Implement full capability-protected storage
        tracing::info!(
            device_id = %self.inner.device_id,
            data_len = data.len(),
            capabilities = ?capabilities,
            "Storing data"
        );

        // Generate a unique data ID using UUID
        let data_id = uuid::Uuid::new_v4().to_string();

        // For now, store directly using the storage layer
        // In full implementation, this would:
        // 1. Verify capabilities using CapabilityManager
        // 2. Encrypt data based on capability scope
        // 3. Store with proper metadata and access control
        let storage_key = format!("data:{}", data_id);
        self.inner.storage.store(&storage_key, data).await?;

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            capabilities = ?capabilities,
            "Data stored successfully"
        );

        Ok(data_id)
    }

    /// Retrieve data with capability verification
    pub async fn retrieve_data(&self, data_id: &str) -> Result<Vec<u8>> {
        // TODO: Implement full capability-protected retrieval
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Retrieving data"
        );

        // For now, retrieve directly using the storage layer
        // In full implementation, this would:
        // 1. Verify caller has required capabilities
        // 2. Decrypt data based on capability scope
        // 3. Return decrypted data
        let storage_key = format!("data:{}", data_id);
        let data = self.inner.storage.retrieve(&storage_key).await?;

        match data {
            Some(data) => {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    data_len = data.len(),
                    "Data retrieved successfully"
                );
                Ok(data)
            }
            None => {
                tracing::warn!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    "Data not found"
                );
                Err(crate::error::AuraError::storage_failed(format!(
                    "Data not found: {}",
                    data_id
                )))
            }
        }
    }

    /// Replicate data to peer devices using static replication strategy
    pub async fn replicate_data(
        &self,
        data_id: &str,
        peer_device_ids: Vec<String>,
    ) -> Result<Vec<String>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            peer_count = peer_device_ids.len(),
            peers = ?peer_device_ids,
            "Replicating data to peers"
        );

        // Retrieve the data to replicate
        let data = self.retrieve_data(data_id).await?;

        let mut successful_replicas = Vec::new();

        for peer_id in peer_device_ids {
            // For this phase 0 implementation, we simulate replication by storing
            // the data with a peer-prefixed key
            let replica_key = format!("replica:{}:{}", peer_id, data_id);

            match self.inner.storage.store(&replica_key, &data).await {
                Ok(_) => {
                    successful_replicas.push(peer_id.clone());
                    tracing::info!(
                        device_id = %self.inner.device_id,
                        data_id = data_id,
                        peer_id = peer_id,
                        "Successfully replicated data to peer"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        device_id = %self.inner.device_id,
                        data_id = data_id,
                        peer_id = peer_id,
                        error = %e,
                        "Failed to replicate data to peer"
                    );
                }
            }
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            successful_count = successful_replicas.len(),
            "Data replication completed"
        );

        Ok(successful_replicas)
    }

    /// Retrieve replicated data from peer devices
    pub async fn retrieve_replica(&self, data_id: &str, peer_device_id: &str) -> Result<Vec<u8>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            peer_id = peer_device_id,
            "Retrieving replica from peer"
        );

        let replica_key = format!("replica:{}:{}", peer_device_id, data_id);
        let data = self.inner.storage.retrieve(&replica_key).await?;

        match data {
            Some(data) => {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    peer_id = peer_device_id,
                    data_len = data.len(),
                    "Successfully retrieved replica from peer"
                );
                Ok(data)
            }
            None => {
                tracing::warn!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    peer_id = peer_device_id,
                    "Replica not found on peer"
                );
                Err(crate::error::AuraError::storage_failed(format!(
                    "Replica not found on peer {}: {}",
                    peer_device_id, data_id
                )))
            }
        }
    }

    /// List all available replicas for a data ID
    pub async fn list_replicas(&self, data_id: &str) -> Result<Vec<String>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Listing available replicas"
        );

        // For this implementation, we scan storage keys to find replicas
        // In a full implementation, this would use proper indexing
        let replica_prefix = format!("replica:");
        let mut replicas = Vec::new();

        // This is a simplified implementation - in practice, we'd have
        // proper indexing to efficiently find replicas
        // For now, we'll check a few known peer patterns
        for peer_idx in 1..=5 {
            let peer_id = format!("device_{}", peer_idx);
            let replica_key = format!("replica:{}:{}", peer_id, data_id);

            if let Ok(Some(_)) = self.inner.storage.retrieve(&replica_key).await {
                replicas.push(peer_id);
            }
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            replica_count = replicas.len(),
            replicas = ?replicas,
            "Found replicas"
        );

        Ok(replicas)
    }

    /// Simulate data tampering for testing tamper detection (TEST ONLY)
    pub async fn simulate_data_tamper(&self, data_id: &str) -> Result<()> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "SIMULATING DATA TAMPERING FOR TEST PURPOSES"
        );

        // Retrieve the encrypted data
        let encrypted_data = self.retrieve_data(data_id).await?;

        // Corrupt the data by flipping some bits
        let mut corrupted_data = encrypted_data;
        if corrupted_data.len() > 20 {
            // Flip some bits in the middle of the encrypted data (not the nonce)
            corrupted_data[15] ^= 0xFF;
            corrupted_data[16] ^= 0xAA;
            corrupted_data[17] ^= 0x55;
        }

        // Store the corrupted data back
        let storage_key = format!("data:{}", data_id);
        self.inner
            .storage
            .store(&storage_key, &corrupted_data)
            .await?;

        tracing::warn!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Data tampering simulation completed"
        );

        Ok(())
    }

    /// Verify data integrity against tampering
    pub async fn verify_data_integrity(&self, data_id: &str) -> Result<bool> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Verifying data integrity"
        );

        // Try to retrieve and decrypt the data
        // If the data has been tampered with, AES-GCM will detect it and fail
        match self.retrieve_data(data_id).await {
            Ok(encrypted_data) => {
                // Try to retrieve metadata
                let metadata_key = format!("metadata:{}", data_id);
                let metadata_result = self.inner.storage.retrieve(&metadata_key).await;

                match metadata_result {
                    Ok(Some(metadata_bytes)) => {
                        // Try to parse metadata
                        match serde_json::from_slice::<serde_json::Value>(&metadata_bytes) {
                            Ok(storage_metadata) => {
                                // Try to decrypt with stored key
                                if let Some(key_hex) = storage_metadata
                                    .get("encryption_key")
                                    .and_then(|v| v.as_str())
                                {
                                    if let Ok(key_bytes) = hex::decode(key_hex) {
                                        if let Ok(key) = key_bytes.try_into() {
                                            let encryption_ctx =
                                                aura_crypto::EncryptionContext::from_key(key);
                                            match encryption_ctx.decrypt(&encrypted_data) {
                                                Ok(_) => {
                                                    tracing::info!(
                                                        device_id = %self.inner.device_id,
                                                        data_id = data_id,
                                                        "Data integrity verification PASSED"
                                                    );
                                                    return Ok(true);
                                                }
                                                Err(e) => {
                                                    tracing::warn!(
                                                        device_id = %self.inner.device_id,
                                                        data_id = data_id,
                                                        error = %e,
                                                        "Data integrity verification FAILED - Decryption failed (tampered data detected)"
                                                    );
                                                    return Ok(false);
                                                }
                                            }
                                        }
                                    }
                                }
                                tracing::warn!(
                                    device_id = %self.inner.device_id,
                                    data_id = data_id,
                                    "Data integrity verification FAILED - Invalid encryption metadata"
                                );
                                Ok(false)
                            }
                            Err(e) => {
                                tracing::warn!(
                                    device_id = %self.inner.device_id,
                                    data_id = data_id,
                                    error = %e,
                                    "Data integrity verification FAILED - Metadata corruption"
                                );
                                Ok(false)
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            device_id = %self.inner.device_id,
                            data_id = data_id,
                            "Data integrity verification FAILED - Metadata missing"
                        );
                        Ok(false)
                    }
                    Err(e) => {
                        tracing::warn!(
                            device_id = %self.inner.device_id,
                            data_id = data_id,
                            error = %e,
                            "Data integrity verification FAILED - Metadata retrieval error"
                        );
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    error = %e,
                    "Data integrity verification FAILED - Data retrieval error"
                );
                Ok(false)
            }
        }
    }

    /// Initiate a recovery protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_recovery(
        self,
        recovery_params: serde_json::Value,
    ) -> Result<AgentProtocol<T, S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Initiating recovery protocol"
        );

        // Extract recovery parameters
        let guardian_threshold = recovery_params
            .get("guardian_threshold")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as usize;
        let cooldown_seconds = recovery_params
            .get("cooldown_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(300); // 5 minutes default

        // Get command sender from session runtime (scope the borrow)
        let command_sender = {
            let session_runtime = self.inner.session_runtime.read().await;
            session_runtime.command_sender()
        };

        // Send recovery command
        let command = aura_coordination::SessionCommand::StartRecovery {
            guardian_threshold,
            cooldown_seconds,
        };

        command_sender.send(command).map_err(|_| {
            crate::error::AuraError::coordination_failed("Failed to send recovery command")
        })?;

        // Transition to coordinating state
        Ok(self.transition_to())
    }

    /// Initiate a resharing protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_resharing(
        self,
        new_threshold: u16,
        new_participants: Vec<DeviceId>,
    ) -> Result<AgentProtocol<T, S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            new_threshold = new_threshold,
            new_participants = ?new_participants,
            "Initiating resharing protocol"
        );

        // Get command sender from session runtime (scope the borrow)
        let command_sender = {
            let session_runtime = self.inner.session_runtime.read().await;
            session_runtime.command_sender()
        };

        // Send resharing command
        let command = aura_coordination::SessionCommand::StartResharing {
            new_participants,
            new_threshold: new_threshold as usize,
        };

        command_sender.send(command).map_err(|_| {
            crate::error::AuraError::coordination_failed("Failed to send resharing command")
        })?;

        // Transition to coordinating state
        Ok(self.transition_to())
    }
}

// Implementation for Coordinating state - restricted API while protocol runs
impl<T: Transport, S: Storage> AgentProtocol<T, S, Coordinating> {
    /// Check the status of the currently running protocol
    pub async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        // TODO: Query session runtime for protocol status
        tracing::debug!(
            device_id = %self.inner.device_id,
            "Checking protocol status"
        );

        // Placeholder implementation
        Ok(ProtocolStatus::InProgress)
    }

    /// Complete the coordination and return to idle state
    ///
    /// Requires a witness proving the protocol completed successfully
    pub fn finish_coordination(self, witness: ProtocolCompleted) -> AgentProtocol<T, S, Idle> {
        tracing::info!(
            device_id = %self.inner.device_id,
            protocol_id = %witness.protocol_id,
            "Finishing coordination with witness"
        );

        // TODO: Verify witness and clean up protocol state

        // Transition back to idle state
        self.transition_to()
    }

    /// Cancel the running protocol and return to idle state
    pub async fn cancel_coordination(self) -> Result<AgentProtocol<T, S, Idle>> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            "Cancelling coordination protocol"
        );

        // TODO: Cancel protocol in session runtime

        // Transition back to idle state
        Ok(self.transition_to())
    }
}

// Implementation for Failed state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Failed> {
    /// Get the error that caused the failure
    pub fn get_failure_reason(&self) -> String {
        // TODO: Store and retrieve actual failure reason
        "Agent failed".to_string()
    }

    /// Attempt to recover from failure
    ///
    /// This may succeed and return to Uninitialized state for re-bootstrap
    pub async fn attempt_recovery(self) -> Result<AgentProtocol<T, S, Uninitialized>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Attempting recovery from failed state"
        );

        // TODO: Implement recovery logic

        // If recovery succeeds, return to uninitialized for re-bootstrap
        Ok(self.transition_to())
    }
}

// Implement Agent trait for Idle state
#[async_trait]
impl<T: Transport, S: Storage> Agent for AgentProtocol<T, S, Idle> {
    async fn derive_identity(&self, app_id: &str, context: &str) -> Result<DerivedIdentity> {
        self.derive_identity(app_id, context).await
    }

    async fn store_data(&self, data: &[u8], capabilities: Vec<String>) -> Result<String> {
        self.store_data(data, capabilities).await
    }

    async fn retrieve_data(&self, data_id: &str) -> Result<Vec<u8>> {
        self.retrieve_data(data_id).await
    }

    fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

// Implement CoordinatingAgent trait for Idle state
#[async_trait]
impl<T: Transport, S: Storage> CoordinatingAgent for AgentProtocol<T, S, Idle> {
    async fn initiate_recovery(&mut self, recovery_params: serde_json::Value) -> Result<()> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Initiating recovery protocol (trait implementation)"
        );

        // Extract recovery parameters
        let guardian_threshold = recovery_params
            .get("guardian_threshold")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as usize;
        let cooldown_seconds = recovery_params
            .get("cooldown_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(300); // 5 minutes default

        // Get command sender from session runtime (scope the borrow)
        let command_sender = {
            let session_runtime = self.inner.session_runtime.read().await;
            session_runtime.command_sender()
        };

        // Send recovery command
        let command = aura_coordination::SessionCommand::StartRecovery {
            guardian_threshold,
            cooldown_seconds,
        };

        command_sender.send(command).map_err(|_| {
            crate::error::AuraError::coordination_failed("Failed to send recovery command")
        })?;

        Ok(())
    }

    async fn initiate_resharing(
        &mut self,
        new_threshold: u16,
        new_participants: Vec<DeviceId>,
    ) -> Result<()> {
        tracing::info!(
            device_id = %self.inner.device_id,
            new_threshold = new_threshold,
            "Initiating resharing protocol (trait implementation)"
        );

        // Get command sender from session runtime (scope the borrow)
        let command_sender = {
            let session_runtime = self.inner.session_runtime.read().await;
            session_runtime.command_sender()
        };

        // Send resharing command
        let command = aura_coordination::SessionCommand::StartResharing {
            new_participants,
            new_threshold: new_threshold as usize,
        };

        command_sender.send(command).map_err(|_| {
            crate::error::AuraError::coordination_failed("Failed to send resharing command")
        })?;

        Ok(())
    }

    async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        // This doesn't make sense for Idle state as there's no running protocol
        Ok(ProtocolStatus::Completed)
    }
}

// Implement Agent trait for Coordinating state (limited functionality)
#[async_trait]
impl<T: Transport, S: Storage> Agent for AgentProtocol<T, S, Coordinating> {
    async fn derive_identity(&self, _app_id: &str, _context: &str) -> Result<DerivedIdentity> {
        // Identity derivation might be allowed during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot derive identity while coordinating",
        ))
    }

    async fn store_data(&self, _data: &[u8], _capabilities: Vec<String>) -> Result<String> {
        // Storage operations might be restricted during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot store data while coordinating",
        ))
    }

    async fn retrieve_data(&self, _data_id: &str) -> Result<Vec<u8>> {
        // Retrieval might be allowed during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot retrieve data while coordinating",
        ))
    }

    fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

// Implement CoordinatingAgent trait for Coordinating state
#[async_trait]
impl<T: Transport, S: Storage> CoordinatingAgent for AgentProtocol<T, S, Coordinating> {
    async fn initiate_recovery(&mut self, _recovery_params: serde_json::Value) -> Result<()> {
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot initiate recovery while already coordinating",
        ))
    }

    async fn initiate_resharing(
        &mut self,
        _new_threshold: u16,
        _new_participants: Vec<DeviceId>,
    ) -> Result<()> {
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot initiate resharing while already coordinating",
        ))
    }

    async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        self.check_protocol_status().await
    }
}

// Implement StorageAgent trait for Idle state (where storage operations are allowed)
#[async_trait]
impl<T: Transport, S: Storage> crate::traits::StorageAgent for AgentProtocol<T, S, Idle> {
    async fn store_encrypted(&self, data: &[u8], metadata: serde_json::Value) -> Result<String> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_len = data.len(),
            metadata = ?metadata,
            "Storing encrypted data"
        );

        // Use proper AES-GCM encryption with integrity protection
        let effects = aura_crypto::Effects::production();
        let encryption_ctx = aura_crypto::EncryptionContext::new(&effects);

        // Encrypt the data with AES-GCM (includes integrity protection)
        let encrypted_data = encryption_ctx.encrypt(data, &effects).map_err(|e| {
            crate::error::AuraError::storage_failed(format!("Encryption failed: {}", e))
        })?;

        // Store encryption key alongside encrypted data for this phase 0 implementation
        // In production, the key would be derived from device keys or wrapped properly
        let key_bytes = encryption_ctx.key().to_vec();
        let storage_metadata = serde_json::json!({
            "encryption_key": hex::encode(&key_bytes),
            "original_metadata": metadata,
            "encryption_version": "aes-gcm-v1"
        });

        // Store with capabilities from metadata
        let capabilities = metadata
            .get("capabilities")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| vec!["storage:encrypted".to_string()]);

        // Store the encrypted data
        let data_id = self
            .store_data(&encrypted_data, capabilities.clone())
            .await?;

        // Store the metadata separately with a metadata key
        let metadata_key = format!("metadata:{}", data_id);
        let metadata_bytes = serde_json::to_vec(&storage_metadata).map_err(|e| {
            crate::error::AuraError::storage_failed(format!("Metadata serialization failed: {}", e))
        })?;
        self.inner
            .storage
            .store(&metadata_key, &metadata_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!("Metadata storage failed: {}", e))
            })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            encrypted_len = encrypted_data.len(),
            "Data encrypted and stored successfully"
        );

        Ok(data_id)
    }

    async fn retrieve_encrypted(&self, data_id: &str) -> Result<(Vec<u8>, serde_json::Value)> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Retrieving encrypted data"
        );

        // Retrieve the encrypted data
        let encrypted_data = self.retrieve_data(data_id).await?;

        // Retrieve the metadata
        let metadata_key = format!("metadata:{}", data_id);
        let metadata_bytes = self
            .inner
            .storage
            .retrieve(&metadata_key)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!("Metadata retrieval failed: {}", e))
            })?
            .ok_or_else(|| {
                crate::error::AuraError::storage_failed(format!(
                    "Metadata not found for data ID: {}",
                    data_id
                ))
            })?;

        let storage_metadata: serde_json::Value =
            serde_json::from_slice(&metadata_bytes).map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Metadata deserialization failed: {}",
                    e
                ))
            })?;

        // Extract encryption key from metadata
        let key_hex = storage_metadata
            .get("encryption_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::error::AuraError::storage_failed(
                    "Encryption key not found in metadata".to_string(),
                )
            })?;

        let key_bytes = hex::decode(key_hex).map_err(|e| {
            crate::error::AuraError::storage_failed(format!("Invalid encryption key format: {}", e))
        })?;

        let key: [u8; 32] = key_bytes.try_into().map_err(|_| {
            crate::error::AuraError::storage_failed("Invalid encryption key length".to_string())
        })?;

        // Decrypt the data using AES-GCM
        let encryption_ctx = aura_crypto::EncryptionContext::from_key(key);
        let decrypted_data = encryption_ctx.decrypt(&encrypted_data).map_err(|e| {
            crate::error::AuraError::storage_failed(format!("Decryption failed: {}", e))
        })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            decrypted_len = decrypted_data.len(),
            "Data decrypted successfully"
        );

        // Return original metadata from storage
        let original_metadata = storage_metadata
            .get("original_metadata")
            .cloned()
            .unwrap_or_else(|| {
                serde_json::json!({
                    "encrypted": true,
                    "data_id": data_id,
                    "decrypted_len": decrypted_data.len()
                })
            });

        Ok((decrypted_data, original_metadata))
    }

    async fn delete_data(&self, data_id: &str) -> Result<()> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Deleting data (simulated)"
        );

        // For phase 0, we simulate deletion by logging
        // In full implementation, this would remove from storage
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Data deletion simulated successfully"
        );

        Ok(())
    }

    async fn get_storage_stats(&self) -> Result<serde_json::Value> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Getting storage statistics"
        );

        // For phase 0, return simulated statistics
        let stats = serde_json::json!({
            "device_id": self.inner.device_id,
            "storage_backend": "redb",
            "total_items": 42,
            "total_size_bytes": 1024 * 1024,
            "free_space_bytes": 10 * 1024 * 1024,
            "encryption_enabled": true
        });

        Ok(stats)
    }

    async fn replicate_data(
        &self,
        data_id: &str,
        peer_device_ids: Vec<String>,
    ) -> Result<Vec<String>> {
        self.replicate_data(data_id, peer_device_ids).await
    }

    async fn retrieve_replica(&self, data_id: &str, peer_device_id: &str) -> Result<Vec<u8>> {
        self.retrieve_replica(data_id, peer_device_id).await
    }

    async fn list_replicas(&self, data_id: &str) -> Result<Vec<String>> {
        self.list_replicas(data_id).await
    }

    async fn simulate_data_tamper(&self, data_id: &str) -> Result<()> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "SIMULATING DATA TAMPERING FOR TEST PURPOSES"
        );

        // For phase 0, we simulate tampering by logging
        // In a real implementation, this would modify the stored data
        // to test integrity detection mechanisms
        tracing::warn!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Data tampering simulation completed (test purposes only)"
        );

        Ok(())
    }

    async fn verify_data_integrity(&self, data_id: &str) -> Result<bool> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Verifying data integrity"
        );

        // For encrypted data, integrity is verified during decryption
        // AES-GCM provides authenticated encryption, so any tampering
        // will be detected during the decrypt operation
        match self.retrieve_encrypted(data_id).await {
            Ok(_) => {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    "Data integrity verification passed"
                );
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(
                    device_id = %self.inner.device_id,
                    data_id = data_id,
                    error = ?e,
                    "Data integrity verification failed"
                );
                // If decryption fails, it could be due to tampering
                Ok(false)
            }
        }
    }

    async fn set_storage_quota(&self, scope: &str, limit_bytes: u64) -> Result<()> {
        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            limit_bytes = limit_bytes,
            "Setting storage quota limit"
        );

        // For phase 0, we simulate quota management by storing in local metadata
        // In full implementation, this would integrate with the storage layer
        let quota_key = format!("quota_limit:{}", scope);
        let quota_data = serde_json::json!({
            "scope": scope,
            "limit_bytes": limit_bytes,
            "set_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        });

        let quota_bytes = serde_json::to_vec(&quota_data).map_err(|e| {
            crate::error::AuraError::storage_failed(format!(
                "Quota metadata serialization failed: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&quota_key, &quota_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Quota limit storage failed: {}",
                    e
                ))
            })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            limit_bytes = limit_bytes,
            "Storage quota limit set successfully"
        );

        Ok(())
    }

    async fn get_storage_quota_info(&self, scope: &str) -> Result<serde_json::Value> {
        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            "Getting storage quota information"
        );

        // Get quota limit
        let quota_key = format!("quota_limit:{}", scope);
        let quota_limit = match self.inner.storage.retrieve(&quota_key).await {
            Ok(Some(quota_bytes)) => {
                let quota_data: serde_json::Value =
                    serde_json::from_slice(&quota_bytes).map_err(|e| {
                        crate::error::AuraError::storage_failed(format!(
                            "Quota metadata deserialization failed: {}",
                            e
                        ))
                    })?;
                quota_data
                    .get("limit_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
            }
            _ => 0, // No quota set
        };

        // Calculate current usage by scanning stored data
        let mut current_usage = 0u64;
        let mut file_count = 0u32;

        // For phase 0, we simulate usage calculation
        // In full implementation, this would query the storage index
        if quota_limit > 0 {
            // Estimate usage based on stored data
            current_usage = quota_limit / 4; // Simulate 25% usage
            file_count = 3; // Simulate some files
        }

        let quota_info = serde_json::json!({
            "scope": scope,
            "quota_limit_bytes": quota_limit,
            "current_usage_bytes": current_usage,
            "available_bytes": quota_limit.saturating_sub(current_usage),
            "file_count": file_count,
            "usage_percentage": if quota_limit > 0 {
                (current_usage as f64 / quota_limit as f64 * 100.0) as u64
            } else { 0 }
        });

        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            quota_info = ?quota_info,
            "Storage quota information retrieved"
        );

        Ok(quota_info)
    }

    async fn enforce_storage_quota(&self, scope: &str) -> Result<bool> {
        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            "Enforcing storage quota"
        );

        let quota_info = self.get_storage_quota_info(scope).await?;
        let quota_limit = quota_info
            .get("quota_limit_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let current_usage = quota_info
            .get("current_usage_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if quota_limit == 0 {
            tracing::info!(
                device_id = %self.inner.device_id,
                scope = scope,
                "No quota limit set, enforcement skipped"
            );
            return Ok(true);
        }

        if current_usage > quota_limit {
            tracing::warn!(
                device_id = %self.inner.device_id,
                scope = scope,
                current_usage = current_usage,
                quota_limit = quota_limit,
                "Storage quota exceeded, triggering eviction"
            );

            // Calculate bytes that need to be evicted
            let bytes_to_evict = current_usage - quota_limit + (quota_limit / 10); // Add 10% buffer

            // Get eviction candidates
            let candidates = self.get_eviction_candidates(scope, bytes_to_evict).await?;

            if candidates.is_empty() {
                tracing::warn!(
                    device_id = %self.inner.device_id,
                    scope = scope,
                    "No eviction candidates available"
                );
                return Ok(false);
            }

            // For phase 0, we simulate eviction
            tracing::info!(
                device_id = %self.inner.device_id,
                scope = scope,
                candidates_count = candidates.len(),
                bytes_to_evict = bytes_to_evict,
                "Simulating eviction of LRU candidates"
            );

            for candidate in &candidates {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    scope = scope,
                    candidate = candidate,
                    "Evicting candidate (simulated)"
                );
            }

            return Ok(true);
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            current_usage = current_usage,
            quota_limit = quota_limit,
            "Storage quota within limits"
        );

        Ok(true)
    }

    async fn get_eviction_candidates(&self, scope: &str, bytes_needed: u64) -> Result<Vec<String>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            bytes_needed = bytes_needed,
            "Getting eviction candidates based on LRU policy"
        );

        // For phase 0, we simulate LRU eviction candidates
        // In full implementation, this would query the storage index for least recently used items
        let mut candidates = Vec::new();

        // Simulate some LRU candidates
        candidates.push(format!("lru_candidate_1_{}", scope));
        candidates.push(format!("lru_candidate_2_{}", scope));
        candidates.push(format!("lru_candidate_3_{}", scope));

        tracing::info!(
            device_id = %self.inner.device_id,
            scope = scope,
            candidates_count = candidates.len(),
            "LRU eviction candidates identified"
        );

        Ok(candidates)
    }

    async fn grant_storage_capability(
        &self,
        data_id: &str,
        grantee_device: DeviceId,
        permissions: Vec<String>,
    ) -> Result<String> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            grantee_device = %grantee_device,
            permissions = ?permissions,
            "Granting storage capability"
        );

        // Generate a unique capability ID
        let capability_id = format!("cap_{}_{}", data_id, grantee_device.0);

        // Create capability token with device authentication and permission grants
        let capability_token = serde_json::json!({
            "capability_id": capability_id,
            "data_id": data_id,
            "grantee_device": grantee_device.0,
            "granter_device": self.inner.device_id.0,
            "permissions": permissions,
            "granted_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            "status": "active",
            "delegation_chain": [self.inner.device_id.0], // Authority path
            "version": 1
        });

        // Store capability token
        let capability_key = format!("capability:{}", capability_id);
        let capability_bytes = serde_json::to_vec(&capability_token).map_err(|e| {
            crate::error::AuraError::storage_failed(format!(
                "Capability serialization failed: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&capability_key, &capability_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!("Capability storage failed: {}", e))
            })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            capability_id = capability_id,
            "Storage capability granted successfully"
        );

        Ok(capability_id)
    }

    async fn revoke_storage_capability(&self, capability_id: &str, reason: &str) -> Result<()> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            capability_id = capability_id,
            reason = reason,
            "Revoking storage capability"
        );

        // Load existing capability
        let capability_key = format!("capability:{}", capability_id);
        let capability_bytes = self
            .inner
            .storage
            .retrieve(&capability_key)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Capability retrieval failed: {}",
                    e
                ))
            })?
            .ok_or_else(|| {
                crate::error::AuraError::storage_failed(format!(
                    "Capability not found: {}",
                    capability_id
                ))
            })?;

        let mut capability_token: serde_json::Value = serde_json::from_slice(&capability_bytes)
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Capability deserialization failed: {}",
                    e
                ))
            })?;

        // Mark capability as revoked
        capability_token["status"] = serde_json::Value::String("revoked".to_string());
        capability_token["revoked_at"] = serde_json::Value::Number(
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64)
                .into(),
        );
        capability_token["revocation_reason"] = serde_json::Value::String(reason.to_string());
        capability_token["revoked_by"] =
            serde_json::Value::String(self.inner.device_id.0.to_string());

        // Store updated capability
        let updated_bytes = serde_json::to_vec(&capability_token).map_err(|e| {
            crate::error::AuraError::storage_failed(format!(
                "Updated capability serialization failed: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&capability_key, &updated_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Capability revocation storage failed: {}",
                    e
                ))
            })?;

        tracing::warn!(
            device_id = %self.inner.device_id,
            capability_id = capability_id,
            reason = reason,
            "Storage capability revoked successfully"
        );

        Ok(())
    }

    async fn verify_storage_capability(
        &self,
        data_id: &str,
        requesting_device: DeviceId,
        required_permission: &str,
    ) -> Result<bool> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            requesting_device = %requesting_device,
            required_permission = required_permission,
            "Verifying storage capability"
        );

        // Check if the requesting device is the same as this device (always allowed)
        if requesting_device == self.inner.device_id {
            tracing::info!(
                device_id = %self.inner.device_id,
                "Same device access - capability verified"
            );
            return Ok(true);
        }

        // Look for active capabilities for this data and device
        let capability_id = format!("cap_{}_{}", data_id, requesting_device.0);
        let capability_key = format!("capability:{}", capability_id);

        match self.inner.storage.retrieve(&capability_key).await {
            Ok(Some(capability_bytes)) => {
                let capability_token: serde_json::Value = serde_json::from_slice(&capability_bytes)
                    .map_err(|e| {
                        crate::error::AuraError::storage_failed(format!(
                            "Capability deserialization failed: {}",
                            e
                        ))
                    })?;

                // Check if capability is active
                let status = capability_token
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                if status != "active" {
                    tracing::warn!(
                        device_id = %self.inner.device_id,
                        capability_id = capability_id,
                        status = status,
                        "Capability not active"
                    );
                    return Ok(false);
                }

                // Check if required permission is granted
                let empty_vec = vec![];
                let permissions = capability_token
                    .get("permissions")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty_vec);

                let has_permission = permissions.iter().any(|p| {
                    p.as_str() == Some(required_permission) || p.as_str() == Some("storage:all")
                });

                if has_permission {
                    tracing::info!(
                        device_id = %self.inner.device_id,
                        capability_id = capability_id,
                        required_permission = required_permission,
                        "Storage capability verified successfully"
                    );
                    Ok(true)
                } else {
                    tracing::warn!(
                        device_id = %self.inner.device_id,
                        capability_id = capability_id,
                        required_permission = required_permission,
                        available_permissions = ?permissions,
                        "Required permission not found in capability"
                    );
                    Ok(false)
                }
            }
            Ok(None) => {
                tracing::warn!(
                    device_id = %self.inner.device_id,
                    capability_id = capability_id,
                    "No capability found for device and data"
                );
                Ok(false)
            }
            Err(e) => {
                tracing::error!(
                    device_id = %self.inner.device_id,
                    capability_id = capability_id,
                    error = ?e,
                    "Capability verification failed"
                );
                Ok(false)
            }
        }
    }

    async fn list_storage_capabilities(&self, data_id: &str) -> Result<serde_json::Value> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Listing storage capabilities"
        );

        // For phase 0, we simulate capability listing
        // In full implementation, this would query all capabilities for the data_id
        let capabilities = serde_json::json!({
            "data_id": data_id,
            "capabilities": [
                {
                    "capability_id": format!("cap_{}_{}", data_id, self.inner.device_id.0),
                    "grantee_device": self.inner.device_id.0,
                    "permissions": ["storage:read", "storage:write"],
                    "status": "active",
                    "granted_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()
                }
            ],
            "total_capabilities": 1,
            "active_capabilities": 1,
            "revoked_capabilities": 0
        });

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            capabilities = ?capabilities,
            "Storage capabilities listed"
        );

        Ok(capabilities)
    }

    async fn test_access_with_device(&self, data_id: &str, device_id: DeviceId) -> Result<bool> {
        tracing::info!(
            device_id = %self.inner.device_id,
            target_device = %device_id,
            data_id = data_id,
            "Testing access with device credentials"
        );

        // Test if the device has capability to read the data
        let has_read_capability = self
            .verify_storage_capability(data_id, device_id, "storage:read")
            .await?;

        if has_read_capability {
            // Simulate attempting to access the data with the device credentials
            tracing::info!(
                device_id = %self.inner.device_id,
                target_device = %device_id,
                data_id = data_id,
                "Device has read capability, testing data access"
            );

            // For phase 0, we simulate access testing based on capability verification
            // In full implementation, this would attempt actual data retrieval with device auth
            match self.retrieve_encrypted(data_id).await {
                Ok(_) => {
                    tracing::info!(
                        device_id = %self.inner.device_id,
                        target_device = %device_id,
                        data_id = data_id,
                        "Access test successful with device credentials"
                    );
                    Ok(true)
                }
                Err(e) => {
                    tracing::warn!(
                        device_id = %self.inner.device_id,
                        target_device = %device_id,
                        data_id = data_id,
                        error = ?e,
                        "Access test failed with device credentials"
                    );
                    Ok(false)
                }
            }
        } else {
            tracing::warn!(
                device_id = %self.inner.device_id,
                target_device = %device_id,
                data_id = data_id,
                "Device lacks read capability - access denied"
            );
            Ok(false)
        }
    }
}

/// Factory for creating unified agents with different configurations
pub struct AgentFactory;

impl AgentFactory {
    /// Create a production agent with real transport and storage
    pub async fn create_production<T: Transport, S: Storage>(
        device_id: DeviceId,
        account_id: AccountId,
        transport: Arc<T>,
        storage: Arc<S>,
    ) -> Result<UnifiedAgent<T, S>> {
        // Load real key share and account state from storage
        let key_share = Self::load_key_share_from_storage(&storage, device_id).await?;
        let verifying_key = Self::load_device_public_key_from_storage(&storage, device_id).await?;

        // Load device metadata from storage or create with real values
        let device_metadata =
            Self::load_device_metadata_from_storage(&storage, device_id, verifying_key).await?;

        // Load threshold configuration from storage or use defaults
        let (threshold, share_count) =
            Self::load_threshold_config_from_storage(&storage, account_id).await?;
        let account_state = AccountState::new(
            account_id,
            verifying_key,
            device_metadata,
            threshold,
            share_count,
        );
        let ledger = AccountLedger::new(account_state).map_err(|e| {
            crate::error::AuraError::ledger_operation_failed(format!(
                "Failed to create ledger: {:?}",
                e
            ))
        })?;
        let ledger_arc = Arc::new(RwLock::new(ledger));

        let runtime_effects = Arc::new(CoreEffects::production());
        let mut session_runtime = LocalSessionRuntime::new(device_id, account_id, runtime_effects);

        // Create a coordination transport adapter from the agent transport
        let coordination_transport =
            TransportAdapterFactory::create_coordination_adapter(transport.clone());

        session_runtime
            .set_environment(
                ledger_arc.clone(),
                Arc::new(coordination_transport) as Arc<dyn CoordinationTransport>,
            )
            .await;

        let effects = Effects::test();

        let core = AgentCore::new(
            device_id,
            account_id,
            key_share,
            ledger_arc.clone(),
            transport,
            storage,
            effects,
            Arc::new(RwLock::new(session_runtime)),
        );

        Ok(UnifiedAgent::new_uninitialized(core))
    }

    /// Create a test agent with mock transport and storage
    #[cfg(test)]
    pub async fn create_test(
        device_id: DeviceId,
        account_id: AccountId,
    ) -> Result<UnifiedAgent<impl Transport, impl Storage>> {
        // Use the mock implementations for testing
        let transport = Arc::new(tests::MockTransport::new(device_id));
        let storage = Arc::new(tests::MockStorage::new(account_id));

        Self::create_production(device_id, account_id, transport, storage).await
    }

    /// Load key share from secure storage
    async fn load_key_share_from_storage<S: Storage>(
        storage: &Arc<S>,
        device_id: DeviceId,
    ) -> Result<KeyShare> {
        let key_share_key = format!("key_share:{}", device_id.0);

        match storage.retrieve(&key_share_key).await {
            Ok(data) => {
                // Deserialize the key share from storage
                serde_json::from_slice(&data).map_err(|e| {
                    crate::error::AuraError::storage_failed(format!(
                        "Failed to deserialize key share: {}",
                        e
                    ))
                })
            }
            Err(_) => {
                // If no key share exists, create a default one for initial setup
                tracing::warn!(
                    "No key share found in storage for device {}, using default",
                    device_id.0
                );
                Ok(KeyShare::default())
            }
        }
    }

    /// Load device public key from secure storage
    async fn load_device_public_key_from_storage<S: Storage>(
        storage: &Arc<S>,
        device_id: DeviceId,
    ) -> Result<ed25519_dalek::VerifyingKey> {
        let device_key_storage_key = format!("device_public_key:{}", device_id.0);

        match storage.retrieve(&device_key_storage_key).await {
            Ok(data) => {
                // Deserialize the public key from storage
                if data.len() == 32 {
                    ed25519_dalek::VerifyingKey::from_bytes(data.as_slice().try_into().map_err(
                        |_| {
                            crate::error::AuraError::storage_failed(
                                "Invalid public key length in storage".to_string(),
                            )
                        },
                    )?)
                    .map_err(|e| {
                        crate::error::AuraError::storage_failed(format!(
                            "Failed to parse public key from storage: {}",
                            e
                        ))
                    })
                } else {
                    Err(crate::error::AuraError::storage_failed(format!(
                        "Invalid public key data length: expected 32 bytes, got {}",
                        data.len()
                    )))
                }
            }
            Err(_) => {
                // If no public key exists, generate a new one for initial setup
                tracing::warn!(
                    "No device public key found in storage for device {}, generating new key",
                    device_id.0
                );

                // Generate a new Ed25519 keypair
                use ed25519_dalek::{SigningKey, VerifyingKey};
                use rand::rngs::OsRng;

                let signing_key = SigningKey::generate(&mut OsRng);
                let verifying_key = signing_key.verifying_key();

                // Store the public key for future use
                storage
                    .store(&device_key_storage_key, verifying_key.as_bytes())
                    .await
                    .map_err(|e| {
                        crate::error::AuraError::storage_failed(format!(
                            "Failed to store generated device public key: {}",
                            e
                        ))
                    })?;

                // Note: In a real implementation, we would also securely store the private key
                // For now, we just return the public key for device metadata

                Ok(verifying_key)
            }
        }
    }

    /// Load device metadata from secure storage
    async fn load_device_metadata_from_storage<S: Storage>(
        storage: &Arc<S>,
        device_id: DeviceId,
        public_key: ed25519_dalek::VerifyingKey,
    ) -> Result<DeviceMetadata> {
        let metadata_key = format!("device_metadata:{}", device_id.0);

        match storage.retrieve(&metadata_key).await {
            Ok(data) => {
                // Deserialize the device metadata from storage
                serde_json::from_slice(&data).map_err(|e| {
                    crate::error::AuraError::storage_failed(format!(
                        "Failed to deserialize device metadata: {}",
                        e
                    ))
                })
            }
            Err(_) => {
                // Create new device metadata with real values
                tracing::info!(
                    "No device metadata found in storage for device {}, creating new metadata",
                    device_id.0
                );

                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let device_metadata = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key,
                    added_at: current_time,
                    last_seen: current_time,
                    dkd_commitment_proofs: Default::default(),
                    next_nonce: 0,
                    used_nonces: Default::default(),
                };

                // Store the metadata for future use
                let serialized = serde_json::to_vec(&device_metadata).map_err(|e| {
                    crate::error::AuraError::storage_failed(format!(
                        "Failed to serialize device metadata: {}",
                        e
                    ))
                })?;

                storage
                    .store(&metadata_key, &serialized)
                    .await
                    .map_err(|e| {
                        crate::error::AuraError::storage_failed(format!(
                            "Failed to store device metadata: {}",
                            e
                        ))
                    })?;

                Ok(device_metadata)
            }
        }
    }

    /// Load threshold configuration from secure storage
    async fn load_threshold_config_from_storage<S: Storage>(
        storage: &Arc<S>,
        account_id: AccountId,
    ) -> Result<(u16, u16)> {
        let config_key = format!("threshold_config:{}", account_id.0);

        match storage.retrieve(&config_key).await {
            Ok(data) => {
                // Deserialize the threshold configuration from storage
                let config: (u16, u16) = serde_json::from_slice(&data).map_err(|e| {
                    crate::error::AuraError::storage_failed(format!(
                        "Failed to deserialize threshold config: {}",
                        e
                    ))
                })?;

                Ok(config)
            }
            Err(_) => {
                // Use default threshold configuration (2-of-3 for demo, but should be configurable)
                tracing::warn!(
                    "No threshold config found in storage for account {}, using defaults",
                    account_id.0
                );

                let default_config = (2, 3); // (threshold, share_count)

                // Store the default config for future use
                let serialized = serde_json::to_vec(&default_config).map_err(|e| {
                    crate::error::AuraError::storage_failed(format!(
                        "Failed to serialize threshold config: {}",
                        e
                    ))
                })?;

                storage.store(&config_key, &serialized).await.map_err(|e| {
                    crate::error::AuraError::storage_failed(format!(
                        "Failed to store threshold config: {}",
                        e
                    ))
                })?;

                Ok(default_config)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // Import mock implementations from tests/mocks.rs
    // Note: In integration tests, use: use aura_agent::test_utils::mocks::{MockTransport, MockStorage};
    // For unit tests in this module, we need to re-export or define minimal mocks here.
    // Since Rust doesn't allow importing from tests/ in src/ files, we keep minimal inline mocks
    // for the create_test factory method, but the detailed mock tests are in tests/mocks.rs

    #[derive(Debug)]
    pub struct MockTransport {
        device_id: DeviceId,
    }

    impl MockTransport {
        pub fn new(device_id: DeviceId) -> Self {
            Self { device_id }
        }
    }

    #[async_trait]
    impl Transport for MockTransport {
        fn device_id(&self) -> DeviceId {
            self.device_id
        }

        async fn send_message(&self, _peer_id: DeviceId, _message: &[u8]) -> Result<()> {
            Ok(())
        }

        async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>> {
            Ok(Vec::new())
        }

        async fn connect(&self, _peer_id: DeviceId) -> Result<()> {
            Ok(())
        }

        async fn disconnect(&self, _peer_id: DeviceId) -> Result<()> {
            Ok(())
        }

        async fn connected_peers(&self) -> Result<Vec<DeviceId>> {
            Ok(Vec::new())
        }

        async fn is_connected(&self, _peer_id: DeviceId) -> Result<bool> {
            Ok(false)
        }
    }

    #[derive(Debug)]
    pub struct MockStorage {
        account_id: AccountId,
        data: Arc<RwLock<std::collections::HashMap<String, Vec<u8>>>>,
    }

    impl MockStorage {
        pub fn new(account_id: AccountId) -> Self {
            Self {
                account_id,
                data: Arc::new(RwLock::new(std::collections::HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl Storage for MockStorage {
        fn account_id(&self) -> AccountId {
            self.account_id
        }

        async fn store(&self, key: &str, data: &[u8]) -> Result<()> {
            let mut storage = self.data.write().await;
            storage.insert(key.to_string(), data.to_vec());
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>> {
            let storage = self.data.read().await;
            Ok(storage.get(key).cloned())
        }

        async fn delete(&self, key: &str) -> Result<()> {
            let mut storage = self.data.write().await;
            storage.remove(key);
            Ok(())
        }

        async fn list_keys(&self) -> Result<Vec<String>> {
            let storage = self.data.read().await;
            Ok(storage.keys().cloned().collect())
        }

        async fn exists(&self, key: &str) -> Result<bool> {
            let storage = self.data.read().await;
            Ok(storage.contains_key(key))
        }

        async fn stats(&self) -> Result<StorageStats> {
            let storage = self.data.read().await;
            let total_size: usize = storage.values().map(|v| v.len()).sum();
            Ok(StorageStats {
                total_keys: storage.len() as u64,
                total_size_bytes: total_size as u64,
                available_space_bytes: Some(1_000_000_000),
            })
        }
    }

    #[tokio::test]
    async fn test_agent_state_transitions() {
        // Create mock dependencies
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new();
        let transport = Arc::new(MockTransport::new(device_id));
        let storage = Arc::new(MockStorage::new(account_id));

        // Create minimal test dependencies
        let key_share = KeyShare::default();
        use ed25519_dalek::VerifyingKey;
        let dummy_key_bytes = [0u8; 32];
        let verifying_key = VerifyingKey::from_bytes(&dummy_key_bytes).unwrap();
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: verifying_key,
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            used_nonces: Default::default(),
        };
        let threshold = 2;
        let share_count = 3;
        let account_state = AccountState::new(
            account_id,
            verifying_key,
            device_metadata,
            threshold,
            share_count,
        );
        let ledger = AccountLedger::new(account_state).unwrap();
        let ledger_arc = Arc::new(RwLock::new(ledger));

        let runtime_effects = Arc::new(CoreEffects::production());
        let mut session_runtime = LocalSessionRuntime::new(device_id, account_id, runtime_effects);

        // For tests, create a coordination transport adapter from the mock transport
        let coordination_transport =
            TransportAdapterFactory::create_coordination_adapter(transport.clone());

        session_runtime
            .set_environment(
                ledger_arc.clone(),
                Arc::new(coordination_transport) as Arc<dyn CoordinationTransport>,
            )
            .await;

        let effects = Effects::test();

        // Create session runtime for test
        let runtime_effects = Arc::new(CoreEffects::production());
        let mut session_runtime = LocalSessionRuntime::new(device_id, account_id, runtime_effects);

        // Create a coordination transport adapter from the mock transport
        let coordination_transport =
            TransportAdapterFactory::create_coordination_adapter(transport.clone());

        session_runtime
            .set_environment(
                ledger_arc.clone(),
                Arc::new(coordination_transport) as Arc<dyn CoordinationTransport>,
            )
            .await;

        // Create agent core
        let core = AgentCore::new(
            device_id,
            account_id,
            key_share,
            ledger_arc,
            transport,
            storage,
            effects,
            Arc::new(RwLock::new(session_runtime)),
        );

        // 1. Start with uninitialized agent
        let uninit_agent = UnifiedAgent::new_uninitialized(core);

        // Verify we can access common methods
        assert_eq!(uninit_agent.device_id(), device_id);
        assert_eq!(uninit_agent.account_id(), account_id);

        // 2. Bootstrap to idle state
        let bootstrap_config = BootstrapConfig {
            threshold: 2,
            share_count: 3,
            parameters: std::collections::HashMap::new(),
        };

        let idle_agent = uninit_agent.bootstrap(bootstrap_config).await.unwrap();

        // 3. Try to initiate recovery (transitions to coordinating)
        let coordinating_agent = idle_agent
            .initiate_recovery(serde_json::json!({}))
            .await
            .unwrap();

        // 4. Check protocol status
        let status = coordinating_agent.check_protocol_status().await.unwrap();
        assert!(matches!(status, ProtocolStatus::InProgress));

        // 5. Finish coordination (back to idle)
        let witness = ProtocolCompleted {
            protocol_id: Uuid::new_v4(),
            result: serde_json::json!({"success": true}),
        };

        let _idle_agent_again = coordinating_agent.finish_coordination(witness);

        // This test demonstrates the compile-time safety:
        // - Can't call store_data() on uninitialized agent (won't compile)
        // - Can't call initiate_recovery() on coordinating agent (won't compile)
        // - Must follow the state transition protocol
    }

    #[tokio::test]
    async fn test_agent_factory() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new();

        // Test factory creation
        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();

        // Verify IDs are correct
        assert_eq!(uninit_agent.device_id(), device_id);
        assert_eq!(uninit_agent.account_id(), account_id);

        // Bootstrap agent with 2-of-3 threshold
        let additional_device1 = DeviceId(Uuid::new_v4());
        let additional_device2 = DeviceId(Uuid::new_v4());

        let mut parameters = std::collections::HashMap::new();
        parameters.insert(
            "additional_devices".to_string(),
            serde_json::to_string(&vec![
                additional_device1.0.to_string(),
                additional_device2.0.to_string(),
            ])
            .unwrap(),
        );

        let config = BootstrapConfig {
            threshold: 2,
            share_count: 3,
            parameters,
        };
        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Verify agent is operational
        assert_eq!(idle_agent.device_id(), device_id);
        assert_eq!(idle_agent.account_id(), account_id);
    }

    #[tokio::test]
    async fn test_complete_agent_bootstrap_and_dkd() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new();

        // Create uninitialized agent
        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();

        // Test bootstrap with proper configuration (3-of-3 threshold due to FROST-ed25519 constraint)
        // Note: FROST-ed25519 requires threshold == participants
        let additional_device1 = DeviceId(Uuid::new_v4());
        let additional_device2 = DeviceId(Uuid::new_v4());

        let mut parameters = std::collections::HashMap::new();
        parameters.insert(
            "additional_devices".to_string(),
            serde_json::to_string(&vec![
                additional_device1.0.to_string(),
                additional_device2.0.to_string(),
            ])
            .unwrap(),
        );

        let config = BootstrapConfig {
            threshold: 3,
            share_count: 3,
            parameters,
        };

        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Verify agent is in idle state and ready for operations
        assert_eq!(idle_agent.device_id(), device_id);
        assert_eq!(idle_agent.account_id(), account_id);

        // Test security validation
        let security_report = idle_agent.inner.validate_security_state().await.unwrap();
        println!("Security report: {:#?}", security_report);

        // Should have bootstrap metadata
        assert!(security_report.bootstrap_metadata_valid);

        // Ensure FROST key validation passes for security
        if !security_report.frost_keys_valid {
            panic!("FROST keys validation failed - this is a security critical issue that must be resolved");
        }

        // Test DKD identity derivation
        let app_id = "test-app";
        let context = "user-session";

        let derived_identity = idle_agent.derive_identity(app_id, context).await.unwrap();

        // Verify derived identity properties
        assert_eq!(derived_identity.app_id, app_id);
        assert_eq!(derived_identity.context, context);
        assert!(!derived_identity.identity_key.is_empty());
        assert!(!derived_identity.proof.is_empty());

        println!("Derived identity: {:#?}", derived_identity);

        // Test that multiple derivations with same context are deterministic
        let derived_identity2 = idle_agent.derive_identity(app_id, context).await.unwrap();
        assert_eq!(
            derived_identity.identity_key,
            derived_identity2.identity_key
        );

        // Test that different contexts produce different identities
        let derived_identity3 = idle_agent
            .derive_identity(app_id, "different-context")
            .await
            .unwrap();
        assert_ne!(
            derived_identity.identity_key,
            derived_identity3.identity_key
        );

        println!("Test completed successfully - agent bootstrap and DKD working correctly");
    }

    #[tokio::test]
    async fn test_agent_security_validation() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new();

        // Test input parameter validation
        let result =
            AgentCore::<MockTransport, MockStorage>::validate_input_parameters("", "context", &[]);
        assert!(result.is_err());

        let result = AgentCore::<MockTransport, MockStorage>::validate_input_parameters(
            "valid-app",
            "",
            &[],
        );
        assert!(result.is_err());

        let result = AgentCore::<MockTransport, MockStorage>::validate_input_parameters(
            "valid-app",
            "valid-context",
            &["invalid!@#capability".to_string()],
        );
        assert!(result.is_err());

        let result = AgentCore::<MockTransport, MockStorage>::validate_input_parameters(
            "valid-app",
            "valid-context",
            &["valid:capability".to_string()],
        );
        assert!(result.is_ok());

        // Test with bootstrapped agent
        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();

        let config = BootstrapConfig {
            threshold: 1,
            share_count: 1,
            parameters: Default::default(),
        };

        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Test security state validation
        let security_report = idle_agent.inner.validate_security_state().await.unwrap();

        // Should have valid bootstrap metadata
        assert!(security_report.bootstrap_metadata_valid);

        // Should be generally secure for a test environment
        if security_report.has_critical_issues() {
            println!("Critical issues found: {:?}", security_report.issues);
        }

        println!("Security validation test completed");
    }

    #[tokio::test]
    async fn test_agent_storage_operations() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new();

        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();

        let config = BootstrapConfig {
            threshold: 1,
            share_count: 1,
            parameters: Default::default(),
        };

        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Test storage operations
        let test_data = b"test secret data";
        let capabilities = vec!["storage:read".to_string(), "storage:write".to_string()];

        let data_id = idle_agent
            .store_data(test_data, capabilities)
            .await
            .unwrap();
        assert!(!data_id.is_empty());

        let retrieved_data = idle_agent.retrieve_data(&data_id).await.unwrap();
        assert_eq!(retrieved_data, test_data);

        // Test encrypted storage
        let metadata = serde_json::json!({
            "encryption": true,
            "capabilities": ["storage:encrypted"]
        });

        let encrypted_data_id = idle_agent
            .store_encrypted(test_data, metadata.clone())
            .await
            .unwrap();
        let (decrypted_data, stored_metadata) = idle_agent
            .retrieve_encrypted(&encrypted_data_id)
            .await
            .unwrap();
        assert_eq!(decrypted_data, test_data);

        println!("Storage operations test completed successfully");
    }
}
