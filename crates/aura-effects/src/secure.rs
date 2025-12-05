//! Layer 3: Secure Storage Effect Handlers - Production Only
//!
//! Stateless single-party implementation of SecureStorageEffects from aura-core (Layer 1).
//! This handler implements pure secure storage effect operations, delegating to platform APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageError, SecureStorageLocation,
};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Real secure storage handler for production use
#[derive(Debug)]
pub struct RealSecureStorageHandler {
    platform_config: String,
    base_path: PathBuf,
}

impl RealSecureStorageHandler {
    /// Create a new real secure storage handler
    pub fn new() -> Result<Self, SecureStorageError> {
        Ok(Self {
            platform_config: "filesystem-fallback".to_string(),
            base_path: PathBuf::from("./secure_store"),
        })
    }

    fn require_capability(
        &self,
        caps: &[SecureStorageCapability],
        required: SecureStorageCapability,
    ) -> Result<(), SecureStorageError> {
        if caps.contains(&required) {
            Ok(())
        } else {
            Err(SecureStorageError::permission_denied(format!(
                "missing capability: {:?}",
                required
            )))
        }
    }

    fn path_for(&self, location: &SecureStorageLocation) -> PathBuf {
        let mut path = self.base_path.join(&location.namespace).join(&location.key);
        if let Some(sub) = &location.sub_key {
            path = path.join(sub);
        }
        path
    }
}

impl Default for RealSecureStorageHandler {
    fn default() -> Self {
        Self::new().unwrap_or(Self {
            platform_config: "filesystem-fallback".to_string(),
            base_path: PathBuf::from("./secure_store"),
        })
    }
}

#[async_trait]
impl SecureStorageEffects for RealSecureStorageHandler {
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        key: &[u8],
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let path = self.path_for(location);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;
        file.write_all(key)
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;
        Ok(())
    }

    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Read)?;
        let path = self.path_for(location);
        fs::read(&path).map_err(|e| SecureStorageError::storage(e.to_string()))
    }

    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Delete)?;
        let path = self.path_for(location);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        }
        Ok(())
    }

    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError> {
        let path = self.path_for(location);
        Ok(path.exists())
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::List)?;
        let ns_path = self.base_path.join(namespace);
        if !ns_path.exists() {
            return Ok(Vec::new());
        }
        let mut keys = Vec::new();
        for entry in
            fs::read_dir(&ns_path).map_err(|e| SecureStorageError::storage(e.to_string()))?
        {
            let entry = entry.map_err(|e| SecureStorageError::storage(e.to_string()))?;
            if let Some(name) = entry.file_name().to_str() {
                keys.push(name.to_string());
            }
        }
        Ok(keys)
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        context: &str,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let mut key = [0u8; 32];
        getrandom::getrandom(&mut key).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        let mut material = key.to_vec();
        material.extend_from_slice(context.as_bytes());
        self.secure_store(location, &material, caps).await?;
        Ok(Some(material))
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        caps: &[aura_core::effects::SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Read)?;
        let token = format!(
            "{}:{}:{}",
            location.full_path(),
            expires_at.ts_ms,
            self.platform_config
        );
        Ok(token.into_bytes())
    }

    async fn secure_access_with_token(
        &self,
        token: &[u8],
        _location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError> {
        let token_str = std::str::from_utf8(token)
            .map_err(|e| SecureStorageError::serialization(e.to_string()))?;
        let parts: Vec<&str> = token_str.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(SecureStorageError::invalid("invalid secure access token"));
        }
        let expires_at_ms: u64 = parts[1].parse().map_err(|e: std::num::ParseIntError| {
            SecureStorageError::serialization(e.to_string())
        })?;

        #[allow(clippy::disallowed_methods)] // Production security handler needs real system time
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SecureStorageError::storage(e.to_string()))?
            .as_millis() as u64;
        if now_ms > expires_at_ms {
            return Err(SecureStorageError::permission_denied(
                "secure access token expired",
            ));
        }

        Ok(parts[0].as_bytes().to_vec())
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
        #[derive(serde::Serialize)]
        struct Attestation<'a> {
            platform: &'a str,
            issued_at_ms: u64,
            capabilities: Vec<String>,
        }

        #[allow(clippy::disallowed_methods)]
        let issued_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SecureStorageError::storage(e.to_string()))?
            .as_millis() as u64;

        let attestation = Attestation {
            platform: &self.platform_config,
            issued_at_ms,
            capabilities: self.get_secure_storage_capabilities(),
        };

        serde_json::to_vec(&attestation)
            .map_err(|e| SecureStorageError::serialization(e.to_string()))
    }

    async fn is_secure_storage_available(&self) -> bool {
        true
    }

    fn get_secure_storage_capabilities(&self) -> Vec<String> {
        vec![
            "filesystem-fallback".to_string(),
            "time-bound-token".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_real_secure_storage_store_and_retrieve() {
        let handler = RealSecureStorageHandler::default();
        let location = SecureStorageLocation::new("test_namespace", "test_key");
        let capabilities = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
            SecureStorageCapability::Delete,
            SecureStorageCapability::List,
        ];

        handler
            .secure_store(&location, b"data", &capabilities)
            .await
            .unwrap();
        let data = handler
            .secure_retrieve(&location, &capabilities)
            .await
            .unwrap();
        assert_eq!(data, b"data");
        assert!(handler.secure_exists(&location).await.unwrap());
        handler
            .secure_delete(&location, &capabilities)
            .await
            .unwrap();
    }
}
