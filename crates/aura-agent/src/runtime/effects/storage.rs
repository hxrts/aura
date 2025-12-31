use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::storage::{StorageError, StorageStats};
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageError, SecureStorageLocation,
    StorageCoreEffects, StorageExtendedEffects,
};
use std::collections::HashMap;

// Implementation of StorageEffects
#[async_trait]
impl StorageCoreEffects for AuraEffectSystem {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.storage_handler.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.storage_handler.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.storage_handler.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.storage_handler.list_keys(prefix).await
    }
}

#[async_trait]
impl StorageExtendedEffects for AuraEffectSystem {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.storage_handler.exists(key).await
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        self.storage_handler.store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        self.storage_handler.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.storage_handler.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.storage_handler.stats().await
    }
}

// Implementation of SecureStorageEffects
#[async_trait]
impl SecureStorageEffects for AuraEffectSystem {
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        key: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.crypto.secure_storage()
            .secure_store(location, key, caps)
            .await
    }

    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.crypto.secure_storage()
            .secure_retrieve(location, caps)
            .await
    }

    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.crypto.secure_storage()
            .secure_delete(location, caps)
            .await
    }

    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError> {
        self.crypto.secure_storage().secure_exists(location).await
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError> {
        self.crypto.secure_storage()
            .secure_list_keys(namespace, caps)
            .await
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        context: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.crypto.secure_storage()
            .secure_generate_key(location, context, caps)
            .await
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.crypto.secure_storage()
            .secure_create_time_bound_token(location, caps, expires_at)
            .await
    }

    async fn secure_access_with_token(
        &self,
        token: &[u8],
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.crypto.secure_storage()
            .secure_access_with_token(token, location)
            .await
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
        self.crypto.secure_storage().get_device_attestation().await
    }

    async fn is_secure_storage_available(&self) -> bool {
        self.crypto.secure_storage()
            .is_secure_storage_available()
            .await
    }

    fn get_secure_storage_capabilities(&self) -> Vec<String> {
        self.crypto.secure_storage()
            .get_secure_storage_capabilities()
    }
}
