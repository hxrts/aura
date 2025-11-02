//! Core agent logic and AgentCore implementation
//!
//! This module provides the AgentCore struct and its methods for:
//! - Device and account identification
//! - Key share and ledger management
//! - Protocol handler with middleware stack
//! - Security validation and key integrity checks
//! - Capability-based access control

use crate::agent::capabilities::KeyShare;
use crate::utils::ResultExt;
use crate::{Result, Storage};
use aura_crypto::Effects;
use aura_journal::{capability::unified_manager::UnifiedCapabilityManager, AccountLedger};
use aura_protocol::middleware::handler::SessionInfo;
use aura_protocol::prelude::*;
use aura_types::{AccountId, DeviceId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for the complex protocol handler type used by AgentCore
type ProtocolHandlerType = Arc<
    RwLock<
        Box<
            dyn AuraProtocolHandler<
                    DeviceId = aura_types::DeviceId,
                    SessionId = uuid::Uuid,
                    Message = Vec<u8>,
                > + Send,
        >,
    >,
>;

pub struct AgentCore<S: Storage> {
    /// Unique identifier for this device
    pub device_id: DeviceId,
    /// Account identifier this agent belongs to
    pub account_id: AccountId,
    /// Threshold key share for cryptographic operations
    pub key_share: Arc<RwLock<KeyShare>>,
    /// CRDT-based account ledger for state management
    pub ledger: Arc<RwLock<AccountLedger>>,
    /// Storage layer for persistence
    pub storage: Arc<S>,
    /// Protocol handler with middleware stack
    pub protocol_handler: ProtocolHandlerType,
    /// Injectable effects for deterministic testing
    pub effects: Effects,
    /// Capability manager for authorization
    pub capability_manager: Arc<RwLock<UnifiedCapabilityManager>>,
}

impl<S: Storage> AgentCore<S> {
    /// Create a new agent core with the provided dependencies
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        key_share: KeyShare,
        ledger: Arc<RwLock<AccountLedger>>,
        storage: Arc<S>,
        effects: Effects,
        protocol_handler: Box<
            dyn AuraProtocolHandler<
                    DeviceId = aura_types::DeviceId,
                    SessionId = uuid::Uuid,
                    Message = Vec<u8>,
                > + Send,
        >,
    ) -> Self {
        Self {
            device_id,
            account_id,
            key_share: Arc::new(RwLock::new(key_share)),
            ledger,
            storage,
            effects,
            protocol_handler: Arc::new(RwLock::new(protocol_handler)),
            capability_manager: Arc::new(RwLock::new(UnifiedCapabilityManager::default())),
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

    /// Start a protocol session with the given parameters
    pub async fn start_protocol_session(
        &self,
        protocol_type: &str,
        participants: Vec<aura_types::DeviceId>,
        metadata: HashMap<String, String>,
    ) -> Result<uuid::Uuid> {
        let mut handler = self.protocol_handler.write().await;
        let session_id = handler
            .start_session(participants, protocol_type.to_string(), metadata)
            .await
            .map_err(|e| {
                aura_types::AuraError::coordination_failed(format!(
                    "Failed to start protocol session: {}",
                    e
                ))
            })?;
        Ok(session_id)
    }

    /// Get information about active protocol sessions
    pub async fn get_active_sessions(&self) -> Result<Vec<SessionInfo>> {
        let mut handler = self.protocol_handler.write().await;
        let sessions = handler.list_sessions().await.map_err(|e| {
            aura_types::AuraError::coordination_failed(format!("Failed to list sessions: {}", e))
        })?;
        Ok(sessions)
    }

    /// Send a message to a specific session
    pub async fn send_session_message(
        &self,
        session_id: uuid::Uuid,
        message: Vec<u8>,
    ) -> Result<()> {
        let mut handler = self.protocol_handler.write().await;
        handler
            .send_message(aura_types::DeviceId(session_id), message)
            .await
            .map_err(|e| {
                aura_types::AuraError::coordination_failed(format!(
                    "Failed to send session message: {}",
                    e
                ))
            })?;
        Ok(())
    }

    /// Receive messages from a specific session
    pub async fn receive_session_messages(&self, session_id: uuid::Uuid) -> Result<Vec<Vec<u8>>> {
        let mut handler = self.protocol_handler.write().await;

        // Get session info to find participants
        let session_info = handler.get_session_info(session_id).await.map_err(|e| {
            aura_types::AuraError::coordination_failed(format!("Failed to get session info: {}", e))
        })?;

        // Collect messages from all participants
        let mut messages = Vec::new();
        for participant in session_info.participants {
            if let Ok(message) = handler.receive_message(participant).await {
                // Convert message to bytes (this may need proper serialization)
                messages.push(format!("{:?}", message).into_bytes());
            }
        }

        Ok(messages)
    }

    /// Terminate a protocol session
    pub async fn terminate_session(&self, session_id: uuid::Uuid) -> Result<()> {
        let mut handler = self.protocol_handler.write().await;
        handler.end_session(session_id).await.map_err(|e| {
            aura_types::AuraError::coordination_failed(format!(
                "Failed to terminate session: {}",
                e
            ))
        })?;
        Ok(())
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
        let frost_key_storage_key = crate::utils::keys::frost_keys(self.device_id);
        match self.storage.retrieve(&frost_key_storage_key).await {
            Ok(Some(frost_data)) => {
                if frost_data.is_empty() {
                    report.add_issue(SecurityIssue::KeyIntegrityViolation(
                        "Empty FROST key data".to_string(),
                    ));
                } else {
                    // Validate FROST keys can be deserialized
                    match serde_json::from_slice::<aura_crypto::frost::FrostKeyShare>(&frost_data) {
                        Ok(_) => {
                            report.frost_keys_valid = true;
                        }
                        Err(e) => {
                            tracing::error!("FROST key deserialization failed: {}", e);
                            report.add_issue(SecurityIssue::CriticalSecurityViolation(format!(
                                "FROST key validation failed: {}",
                                e
                            )));
                        }
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
        let bootstrap_key = crate::utils::keys::bootstrap_metadata(self.device_id);
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

        report.validation_completed_at = aura_types::time_utils::current_unix_timestamp_millis();

        Ok(report)
    }

    /// Store JSON metadata to storage
    ///
    /// This helper eliminates the repeated pattern of serializing and storing JSON.
    pub async fn store_json_metadata(&self, key: &str, value: impl serde::Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(&value).serialize_context("JSON metadata")?;
        self.storage.store(key, &bytes).await
    }

    /// Retrieve JSON metadata from storage
    ///
    /// This helper eliminates the repeated pattern of retrieving and deserializing JSON.
    pub async fn retrieve_json_metadata<R: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<R>> {
        match self.storage.retrieve(key).await? {
            Some(bytes) => {
                let value = serde_json::from_slice(&bytes).deserialize_context("JSON metadata")?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Get encryption context for this agent
    ///
    /// This helper provides a consistent way to get encryption context.
    pub fn encryption_context(&self) -> aura_crypto::EncryptionContext {
        aura_crypto::EncryptionContext::new(&self.effects)
    }

    /// Validate input parameters for security compliance
    pub fn validate_input_parameters(
        app_id: &str,
        context: &str,
        capabilities: &[String],
    ) -> Result<()> {
        crate::utils::validation::validate_input_parameters(app_id, context, capabilities)
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
    pub validation_completed_at: u64,
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
    CriticalSecurityViolation(String),
}

impl SecurityIssue {
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            SecurityIssue::KeyIntegrityViolation(_)
                | SecurityIssue::LedgerIntegrityViolation(_)
                | SecurityIssue::CriticalSecurityViolation(_)
        )
    }
}
