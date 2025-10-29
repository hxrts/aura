//! Trait implementations for different session states
//!
//! This module provides implementations of Agent, CoordinatingAgent, and StorageAgent
//! traits for different session states (Idle, Coordinating, Failed).

use super::states::{AgentProtocol, Coordinating, Idle, ProtocolStatus};
use crate::agent::capabilities::convert_string_capabilities_to_permissions;
use crate::traits::{Agent, CoordinatingAgent, StorageAgent};
use crate::{DerivedIdentity, Result, Storage, Transport};
use async_trait::async_trait;
use aura_protocol::SessionStatusInfo;
use aura_types::{AccountId, DeviceId};

// ============================================================================
// Agent trait implementations
// ============================================================================

/// Implement Agent trait for Idle state - full functionality available
#[async_trait]
impl<T: Transport, S: Storage> Agent for AgentProtocol<T, S, Idle> {
    async fn derive_identity(&self, app_id: &str, context: &str) -> Result<DerivedIdentity> {
        self.derive_identity_impl(app_id, context).await
    }

    async fn store_data(&self, data: &[u8], capabilities: Vec<String>) -> Result<String> {
        // Convert string capabilities to Permission objects
        let permissions = convert_string_capabilities_to_permissions(capabilities);
        self.store_data_impl(data, permissions).await
    }

    async fn retrieve_data(&self, data_id: &str) -> Result<Vec<u8>> {
        self.retrieve_data_impl(data_id).await
    }

    fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

/// Implement Agent trait for Coordinating state - restricted functionality
#[async_trait]
impl<T: Transport, S: Storage> Agent for AgentProtocol<T, S, Coordinating> {
    async fn derive_identity(&self, _app_id: &str, _context: &str) -> Result<DerivedIdentity> {
        // Identity derivation is restricted during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot derive identity while coordinating",
        ))
    }

    async fn store_data(&self, _data: &[u8], _capabilities: Vec<String>) -> Result<String> {
        // Storage operations are restricted during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot store data while coordinating",
        ))
    }

    async fn retrieve_data(&self, _data_id: &str) -> Result<Vec<u8>> {
        // Retrieval is restricted during coordination
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

// ============================================================================
// CoordinatingAgent trait implementations
// ============================================================================

/// Implement CoordinatingAgent trait for Idle state
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

        // Send recovery command using the new API
        let command = aura_protocol::SessionCommand::StartRecovery {
            guardian_threshold,
            cooldown_seconds,
        };

        self.inner.send_session_command(command).await?;

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

        // Send resharing command using the new API
        let command = aura_protocol::SessionCommand::StartResharing {
            new_participants,
            new_threshold: new_threshold as usize,
        };

        self.inner.send_session_command(command).await?;

        Ok(())
    }

    async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        // Idle state has no running protocol
        Ok(ProtocolStatus::Completed {
            protocol_name: "idle".to_string(),
        })
    }

    async fn get_detailed_session_status(&self) -> Result<Vec<SessionStatusInfo>> {
        // For idle state, return empty session list
        Ok(Vec::new())
    }

    async fn has_failed_sessions(&self) -> Result<bool> {
        // Idle state has no active sessions, so no failed sessions
        Ok(false)
    }

    async fn get_session_timeout_info(&self) -> Result<Option<std::time::Duration>> {
        // Idle state has no active sessions, so no timeouts
        Ok(None)
    }
}

/// Implement CoordinatingAgent trait for Coordinating state
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

    async fn get_detailed_session_status(&self) -> Result<Vec<SessionStatusInfo>> {
        self.get_detailed_session_status().await
    }

    async fn has_failed_sessions(&self) -> Result<bool> {
        self.has_failed_sessions().await
    }

    async fn get_session_timeout_info(&self) -> Result<Option<std::time::Duration>> {
        self.get_session_timeout_info().await
    }
}

// ============================================================================
// StorageAgent trait implementations
// ============================================================================

/// Implement StorageAgent trait for Idle state (where storage operations are allowed)
#[async_trait]
impl<T: Transport, S: Storage> StorageAgent for AgentProtocol<T, S, Idle> {
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
            crate::error::AuraError::crypto_operation_failed(format!("Encryption failed: {}", e))
        })?;

        // Generate unique data ID
        let data_id = uuid::Uuid::new_v4().to_string();

        // Store encrypted data
        let storage_key = format!("data:{}", data_id);
        self.inner
            .storage
            .store(&storage_key, &encrypted_data)
            .await?;

        // Store encryption metadata (including key for decryption)
        let storage_metadata = serde_json::json!({
            "data_id": data_id,
            "encryption_key": hex::encode(encryption_ctx.key()),
            "encrypted_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            "metadata": metadata,
            "version": "aes-gcm-256"
        });

        let metadata_key = format!("metadata:{}", data_id);
        let metadata_bytes = serde_json::to_vec(&storage_metadata).map_err(|e| {
            crate::error::AuraError::serialization_failed(format!(
                "Failed to serialize metadata: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&metadata_key, &metadata_bytes)
            .await?;

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            encrypted_size = encrypted_data.len(),
            "Data stored with encryption"
        );

        Ok(data_id)
    }

    async fn retrieve_encrypted(&self, data_id: &str) -> Result<(Vec<u8>, serde_json::Value)> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Retrieving encrypted data"
        );

        // Retrieve encrypted data
        let storage_key = format!("data:{}", data_id);
        let encrypted_data = self
            .inner
            .storage
            .retrieve(&storage_key)
            .await?
            .ok_or_else(|| {
                crate::error::AuraError::storage_failed(format!(
                    "Encrypted data not found: {}",
                    data_id
                ))
            })?;

        // Retrieve metadata to get encryption key
        let metadata_key = format!("metadata:{}", data_id);
        let metadata_bytes = self
            .inner
            .storage
            .retrieve(&metadata_key)
            .await?
            .ok_or_else(|| {
                crate::error::AuraError::storage_failed(format!(
                    "Encryption metadata not found: {}",
                    data_id
                ))
            })?;

        let storage_metadata: serde_json::Value =
            serde_json::from_slice(&metadata_bytes).map_err(|e| {
                crate::error::AuraError::deserialization_failed(format!(
                    "Failed to deserialize metadata: {}",
                    e
                ))
            })?;

        // Extract encryption key
        let key_hex = storage_metadata
            .get("encryption_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                crate::error::AuraError::storage_failed(
                    "Encryption key not found in metadata".to_string(),
                )
            })?;

        let key_bytes = hex::decode(key_hex).map_err(|e| {
            crate::error::AuraError::storage_failed(format!(
                "Failed to decode encryption key: {}",
                e
            ))
        })?;

        let key: [u8; 32] = key_bytes.try_into().map_err(|_| {
            crate::error::AuraError::storage_failed("Invalid encryption key length".to_string())
        })?;

        // Decrypt the data with AES-GCM (includes integrity verification)
        let encryption_ctx = aura_crypto::EncryptionContext::from_key(key);
        let decrypted_data = encryption_ctx.decrypt(&encrypted_data).map_err(|e| {
            crate::error::AuraError::crypto_operation_failed(format!(
                "Decryption failed (data may be tampered): {}",
                e
            ))
        })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            decrypted_size = decrypted_data.len(),
            "Data retrieved and decrypted successfully"
        );

        Ok((decrypted_data, storage_metadata))
    }

    async fn delete_data(&self, data_id: &str) -> Result<()> {
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Deleting encrypted data"
        );

        // Delete encrypted data
        let storage_key = format!("data:{}", data_id);
        self.inner.storage.delete(&storage_key).await?;

        // Delete metadata
        let metadata_key = format!("metadata:{}", data_id);
        self.inner.storage.delete(&metadata_key).await?;

        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Encrypted data deleted successfully"
        );

        Ok(())
    }

    async fn get_storage_stats(&self) -> Result<serde_json::Value> {
        let stats = self.inner.storage.stats().await?;
        Ok(serde_json::json!({
            "total_size_bytes": stats.total_size_bytes,
            "total_keys": stats.total_keys,
            "available_space_bytes": stats.available_space_bytes,
        }))
    }

    // Placeholder implementations for other StorageAgent methods
    async fn replicate_data(
        &self,
        _data_id: &str,
        _peer_device_ids: Vec<String>,
    ) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn retrieve_replica(&self, data_id: &str, _peer_device_id: &str) -> Result<Vec<u8>> {
        self.retrieve_data(data_id).await
    }

    async fn list_replicas(&self, _data_id: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn simulate_data_tamper(&self, _data_id: &str) -> Result<()> {
        Ok(())
    }

    async fn verify_data_integrity(&self, data_id: &str) -> Result<bool> {
        Ok(self.inner.storage.exists(data_id).await?)
    }

    async fn set_storage_quota(&self, _scope: &str, _limit_bytes: u64) -> Result<()> {
        Ok(())
    }

    async fn get_storage_quota_info(&self, _scope: &str) -> Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    async fn enforce_storage_quota(&self, _scope: &str) -> Result<bool> {
        Ok(true)
    }

    async fn get_eviction_candidates(
        &self,
        _scope: &str,
        _bytes_needed: u64,
    ) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn grant_storage_capability(
        &self,
        _data_id: &str,
        _grantee_device: DeviceId,
        _permissions: Vec<String>,
    ) -> Result<String> {
        Ok(uuid::Uuid::new_v4().to_string())
    }

    async fn revoke_storage_capability(&self, _capability_id: &str, _reason: &str) -> Result<()> {
        Ok(())
    }

    async fn verify_storage_capability(
        &self,
        _data_id: &str,
        _requesting_device: DeviceId,
        _required_permission: &str,
    ) -> Result<bool> {
        Ok(true)
    }

    async fn list_storage_capabilities(&self, _data_id: &str) -> Result<serde_json::Value> {
        Ok(serde_json::json!([]))
    }

    async fn test_access_with_device(&self, data_id: &str, _device_id: DeviceId) -> Result<bool> {
        Ok(self.inner.storage.exists(data_id).await?)
    }
}
