//! Layer 3: Secure Storage Effect Handlers - Production Only
//!
//! Stateless single-party implementation of SecureStorageEffects from aura-core (Layer 1).
//! This handler implements pure secure storage effect operations, delegating to platform APIs.
//!
//! **Layer Constraint**: No mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production stateless handlers.

#[cfg(target_arch = "wasm32")]
use crate::storage::FilesystemStorageHandler;
use async_trait::async_trait;
use aura_core::effects::{
    SecureGeneratedKey, SecureStorageCapability, SecureStorageEffects, SecureStorageError,
    SecureStorageLocation,
};
#[cfg(target_arch = "wasm32")]
use aura_core::effects::{StorageCoreEffects, StorageExtendedEffects};
use cfg_if::cfg_if;
#[cfg(not(target_arch = "wasm32"))]
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd"
))]
const PLATFORM_KEYRING_SERVICE: &str = "hxrts.aura.secure-storage";

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use js_sys::Date;
    } else {
        use std::time::{SystemTime, UNIX_EPOCH};
    }
}

#[cfg(not(target_arch = "wasm32"))]
const FALLBACK_RECORD_MAGIC: &[u8] = b"AURA-FS-FALLBACK-SECURE-V1";
#[cfg(not(target_arch = "wasm32"))]
const FALLBACK_NONCE_LEN: usize = 12;
#[cfg(not(target_arch = "wasm32"))]
const SECURE_ACCESS_TOKEN_VERSION: u8 = 1;
#[cfg(not(target_arch = "wasm32"))]
const SECURE_ACCESS_TOKEN_NONCE_LEN: usize = 12;
#[cfg(not(target_arch = "wasm32"))]
const SECURE_ACCESS_TOKEN_AAD_DOMAIN: &str = "aura:secure-storage-access-token:v1";

#[cfg(not(target_arch = "wasm32"))]
fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), SecureStorageError> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        file.set_permissions(permissions)
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    }
    file.write_all(bytes)
        .map_err(|e| SecureStorageError::storage(e.to_string()))
}

#[allow(clippy::disallowed_methods)] // Effect implementation reads wall clock directly.
fn current_time_ms() -> Result<u64, SecureStorageError> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok(Date::now() as u64)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SecureStorageError::storage(e.to_string()))?
            .as_millis() as u64)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SecureAccessTokenClaims {
    version: u8,
    location: SecureStorageLocation,
    capabilities: Vec<SecureStorageCapability>,
    expires_at_ms: u64,
    audience: String,
    nonce: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SecureAccessTokenEnvelope {
    version: u8,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
fn generate_secret_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).expect("secure storage requires OS randomness");
    key
}

#[cfg(not(target_arch = "wasm32"))]
fn secure_access_token_aad(audience: &str, location: &SecureStorageLocation) -> Vec<u8> {
    format!(
        "{}:{}:{}",
        SECURE_ACCESS_TOKEN_AAD_DOMAIN,
        audience,
        location.full_path()
    )
    .into_bytes()
}

#[cfg(not(target_arch = "wasm32"))]
fn create_authenticated_access_token(
    token_key: &[u8; 32],
    audience: &str,
    location: &SecureStorageLocation,
    capabilities: &[SecureStorageCapability],
    expires_at_ms: u64,
) -> Result<Vec<u8>, SecureStorageError> {
    let mut nonce = [0u8; SECURE_ACCESS_TOKEN_NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| SecureStorageError::storage(e.to_string()))?;
    let claims = SecureAccessTokenClaims {
        version: SECURE_ACCESS_TOKEN_VERSION,
        location: location.clone(),
        capabilities: capabilities.to_vec(),
        expires_at_ms,
        audience: audience.to_string(),
        nonce: nonce.to_vec(),
    };
    let claims = serde_json::to_vec(&claims)
        .map_err(|e| SecureStorageError::serialization(e.to_string()))?;
    let cipher = ChaCha20Poly1305::new(token_key.into());
    let aad = secure_access_token_aad(audience, location);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &claims,
                aad: &aad,
            },
        )
        .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    let envelope = SecureAccessTokenEnvelope {
        version: SECURE_ACCESS_TOKEN_VERSION,
        nonce: nonce.to_vec(),
        ciphertext,
    };
    serde_json::to_vec(&envelope).map_err(|e| SecureStorageError::serialization(e.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
fn verify_authenticated_access_token(
    token_key: &[u8; 32],
    audience: &str,
    token: &[u8],
    requested_location: &SecureStorageLocation,
    used_tokens: &Mutex<HashSet<[u8; 32]>>,
) -> Result<Vec<SecureStorageCapability>, SecureStorageError> {
    let token_id = aura_core::hash::hash(token);
    let envelope: SecureAccessTokenEnvelope = serde_json::from_slice(token)
        .map_err(|e| SecureStorageError::serialization(e.to_string()))?;
    if envelope.version != SECURE_ACCESS_TOKEN_VERSION
        || envelope.nonce.len() != SECURE_ACCESS_TOKEN_NONCE_LEN
    {
        return Err(SecureStorageError::invalid("invalid secure access token"));
    }
    let cipher = ChaCha20Poly1305::new(token_key.into());
    let aad = secure_access_token_aad(audience, requested_location);
    let claims = cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            Payload {
                msg: &envelope.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| SecureStorageError::permission_denied("invalid secure access token"))?;
    let claims: SecureAccessTokenClaims = serde_json::from_slice(&claims)
        .map_err(|e| SecureStorageError::serialization(e.to_string()))?;
    if claims.version != SECURE_ACCESS_TOKEN_VERSION
        || claims.location != *requested_location
        || claims.audience != audience
        || !claims.capabilities.contains(&SecureStorageCapability::Read)
    {
        return Err(SecureStorageError::permission_denied(
            "secure access token is not bound to the requested access",
        ));
    }
    if current_time_ms()? > claims.expires_at_ms {
        return Err(SecureStorageError::permission_denied(
            "secure access token expired",
        ));
    }
    let mut used = used_tokens
        .lock()
        .map_err(|_| SecureStorageError::storage("secure token replay cache is poisoned"))?;
    if !used.insert(token_id) {
        return Err(SecureStorageError::permission_denied(
            "secure access token has already been used",
        ));
    }
    Ok(claims.capabilities)
}

/// Production secure storage selector.
///
/// Production mode uses the platform credential store on supported native
/// targets. Unsupported production targets fail closed instead of silently using
/// filesystem fallback storage. Tests and simulations may explicitly construct
/// the named filesystem fallback variant.
#[derive(Debug)]
pub enum ProductionSecureStorageHandler {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd"
    ))]
    /// Platform credential-store backed secure storage.
    Platform(PlatformSecureStorageHandler),
    /// Explicit non-production filesystem fallback.
    FilesystemFallback(FilesystemFallbackSecureStorageHandler),
    /// Fail-closed production handler for targets without platform support.
    UnavailablePlatform {
        /// Target label used in fail-closed error messages.
        target: &'static str,
    },
}

impl ProductionSecureStorageHandler {
    /// Create production secure storage. Unsupported targets fail closed.
    pub fn for_production(base_path: PathBuf) -> Self {
        let _ = base_path;
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd"
        ))]
        {
            Self::Platform(PlatformSecureStorageHandler::new())
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd"
        )))]
        {
            Self::UnavailablePlatform {
                target: "unsupported",
            }
        }
    }

    /// Create explicitly non-production secure storage for tests/simulations.
    pub fn filesystem_fallback_for_non_production(base_path: PathBuf) -> Self {
        Self::FilesystemFallback(FilesystemFallbackSecureStorageHandler::with_base_path(
            base_path,
        ))
    }

    fn unavailable_error(target: &'static str) -> SecureStorageError {
        SecureStorageError::storage(format!(
            "platform secure storage is unavailable for production target {target}"
        ))
    }
}

#[async_trait]
impl SecureStorageEffects for ProductionSecureStorageHandler {
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_store(location, data, caps).await,
            Self::FilesystemFallback(handler) => handler.secure_store(location, data, caps).await,
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_retrieve(location, caps).await,
            Self::FilesystemFallback(handler) => handler.secure_retrieve(location, caps).await,
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_delete(location, caps).await,
            Self::FilesystemFallback(handler) => handler.secure_delete(location, caps).await,
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_exists(location).await,
            Self::FilesystemFallback(handler) => handler.secure_exists(location).await,
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_list_keys(namespace, caps).await,
            Self::FilesystemFallback(handler) => handler.secure_list_keys(namespace, caps).await,
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        key_type: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<SecureGeneratedKey, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_generate_key(location, key_type, caps).await,
            Self::FilesystemFallback(handler) => {
                handler.secure_generate_key(location, key_type, caps).await
            }
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => {
                handler
                    .secure_create_time_bound_token(location, caps, expires_at)
                    .await
            }
            Self::FilesystemFallback(handler) => {
                handler
                    .secure_create_time_bound_token(location, caps, expires_at)
                    .await
            }
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn secure_access_with_token(
        &self,
        token: &[u8],
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_access_with_token(token, location).await,
            Self::FilesystemFallback(handler) => {
                handler.secure_access_with_token(token, location).await
            }
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.get_device_attestation().await,
            Self::FilesystemFallback(handler) => handler.get_device_attestation().await,
            Self::UnavailablePlatform { target } => Err(Self::unavailable_error(target)),
        }
    }

    async fn is_secure_storage_available(&self) -> bool {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.is_secure_storage_available().await,
            Self::FilesystemFallback(handler) => handler.is_secure_storage_available().await,
            Self::UnavailablePlatform { .. } => false,
        }
    }

    fn get_secure_storage_capabilities(&self) -> Vec<String> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.get_secure_storage_capabilities(),
            Self::FilesystemFallback(handler) => handler.get_secure_storage_capabilities(),
            Self::UnavailablePlatform { target } => {
                vec![
                    "platform-secure-storage-unavailable".to_string(),
                    format!("target:{target}"),
                ]
            }
        }
    }
}

/// Platform credential-store backed secure storage.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd"
))]
#[derive(Debug)]
pub struct PlatformSecureStorageHandler {
    service: String,
    platform_config: String,
    token_key: [u8; 32],
    used_tokens: Mutex<HashSet<[u8; 32]>>,
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd"
))]
impl PlatformSecureStorageHandler {
    /// Create a platform credential-store backed secure storage handler.
    pub fn new() -> Self {
        Self {
            service: PLATFORM_KEYRING_SERVICE.to_string(),
            platform_config: "platform-keyring".to_string(),
            token_key: generate_secret_key(),
            used_tokens: Mutex::new(HashSet::new()),
        }
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
                "missing capability: {required:?}"
            )))
        }
    }

    fn entry_for_location(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<keyring::Entry, SecureStorageError> {
        FilesystemFallbackSecureStorageHandler::validate_location(location)?;
        self.entry_for_user(&Self::user_for_location(location))
    }

    fn entry_for_namespace_index(
        &self,
        namespace: &str,
    ) -> Result<keyring::Entry, SecureStorageError> {
        FilesystemFallbackSecureStorageHandler::validate_component("namespace", namespace)?;
        self.entry_for_user(&format!(
            "index:{}",
            FilesystemFallbackSecureStorageHandler::encode_component(namespace)
        ))
    }

    fn entry_for_user(&self, user: &str) -> Result<keyring::Entry, SecureStorageError> {
        keyring::Entry::new(&self.service, user).map_err(Self::map_keyring_error)
    }

    fn user_for_location(location: &SecureStorageLocation) -> String {
        let mut user = format!(
            "record:{}:{}",
            FilesystemFallbackSecureStorageHandler::encode_component(&location.namespace),
            FilesystemFallbackSecureStorageHandler::encode_component(&location.key)
        );
        if let Some(sub_key) = &location.sub_key {
            user.push(':');
            user.push_str(&FilesystemFallbackSecureStorageHandler::encode_component(
                sub_key,
            ));
        }
        user
    }

    fn load_namespace_index(&self, namespace: &str) -> Result<Vec<String>, SecureStorageError> {
        let entry = self.entry_for_namespace_index(namespace)?;
        match entry.get_secret() {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| SecureStorageError::storage(e.to_string())),
            Err(keyring::Error::NoEntry) => Ok(Vec::new()),
            Err(err) => Err(Self::map_keyring_error(err)),
        }
    }

    fn store_namespace_index(
        &self,
        namespace: &str,
        keys: &[String],
    ) -> Result<(), SecureStorageError> {
        let entry = self.entry_for_namespace_index(namespace)?;
        let bytes = serde_json::to_vec(keys)
            .map_err(|e| SecureStorageError::serialization(e.to_string()))?;
        entry.set_secret(&bytes).map_err(Self::map_keyring_error)
    }

    fn add_index_key(&self, location: &SecureStorageLocation) -> Result<(), SecureStorageError> {
        let mut keys = self.load_namespace_index(&location.namespace)?;
        if !keys.contains(&location.key) {
            keys.push(location.key.clone());
            keys.sort();
            self.store_namespace_index(&location.namespace, &keys)?;
        }
        Ok(())
    }

    fn remove_index_key(&self, location: &SecureStorageLocation) -> Result<(), SecureStorageError> {
        let mut keys = self.load_namespace_index(&location.namespace)?;
        let old_len = keys.len();
        keys.retain(|key| key != &location.key);
        if keys.len() != old_len {
            self.store_namespace_index(&location.namespace, &keys)?;
        }
        Ok(())
    }

    fn map_keyring_error(error: keyring::Error) -> SecureStorageError {
        match error {
            keyring::Error::NoEntry => SecureStorageError::storage("secure key not found"),
            keyring::Error::Invalid(field, reason) => {
                SecureStorageError::invalid(format!("{field}: {reason}"))
            }
            other => SecureStorageError::storage(other.to_string()),
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd"
))]
impl Default for PlatformSecureStorageHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd"
))]
#[async_trait]
impl SecureStorageEffects for PlatformSecureStorageHandler {
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let entry = self.entry_for_location(location)?;
        entry.set_secret(data).map_err(Self::map_keyring_error)?;
        self.add_index_key(location)
    }

    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Read)?;
        self.entry_for_location(location)?
            .get_secret()
            .map_err(Self::map_keyring_error)
    }

    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Delete)?;
        match self.entry_for_location(location)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {
                self.remove_index_key(location)?;
                Ok(())
            }
            Err(err) => Err(Self::map_keyring_error(err)),
        }
    }

    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError> {
        match self.entry_for_location(location)?.get_secret() {
            Ok(_) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(err) => Err(Self::map_keyring_error(err)),
        }
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::List)?;
        self.load_namespace_index(namespace)
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        context: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<SecureGeneratedKey, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let mut key = [0u8; 32];
        getrandom::getrandom(&mut key).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        let mut material = key.to_vec();
        material.extend_from_slice(context.as_bytes());
        self.secure_store(location, &material, caps).await?;
        Ok(SecureGeneratedKey::OpaqueHandle(location.full_path()))
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Read)?;
        create_authenticated_access_token(
            &self.token_key,
            &self.platform_config,
            location,
            caps,
            expires_at.ts_ms,
        )
    }

    async fn secure_access_with_token(
        &self,
        token: &[u8],
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError> {
        let capabilities = verify_authenticated_access_token(
            &self.token_key,
            &self.platform_config,
            token,
            location,
            &self.used_tokens,
        )?;
        self.secure_retrieve(location, &capabilities).await
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
        #[derive(serde::Serialize)]
        struct Attestation<'a> {
            platform: &'a str,
            issued_at_ms: u64,
            capabilities: Vec<String>,
        }

        let attestation = Attestation {
            platform: &self.platform_config,
            issued_at_ms: current_time_ms()?,
            capabilities: self.get_secure_storage_capabilities(),
        };

        serde_json::to_vec(&attestation)
            .map_err(|e| SecureStorageError::serialization(e.to_string()))
    }

    async fn is_secure_storage_available(&self) -> bool {
        self.entry_for_user("availability-probe").is_ok()
    }

    fn get_secure_storage_capabilities(&self) -> Vec<String> {
        vec![
            "platform-keyring".to_string(),
            "opaque-secret-bytes".to_string(),
            "time-bound-token".to_string(),
        ]
    }
}

/// Explicit filesystem fallback for secure storage.
///
/// This handler is not a platform secure enclave, keystore, TPM, or hardware
/// backed implementation. It is a clearly named fallback used until a target
/// platform wires a stronger secure-storage backend.
#[derive(Debug)]
pub struct FilesystemFallbackSecureStorageHandler {
    platform_config: String,
    base_path: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    wrapping_key: [u8; 32],
    #[cfg(not(target_arch = "wasm32"))]
    token_key: [u8; 32],
    #[cfg(not(target_arch = "wasm32"))]
    used_tokens: Mutex<HashSet<[u8; 32]>>,
}

impl FilesystemFallbackSecureStorageHandler {
    /// Create a filesystem fallback secure storage handler with a custom base path.
    ///
    /// The secure storage files will be placed in `base_path/secure_store/`.
    pub fn with_base_path(base_path: PathBuf) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let wrapping_key = generate_secret_key();
        #[cfg(not(target_arch = "wasm32"))]
        let token_key = generate_secret_key();
        Self {
            platform_config: "filesystem-fallback".to_string(),
            base_path: base_path.join("secure_store"),
            #[cfg(not(target_arch = "wasm32"))]
            wrapping_key,
            #[cfg(not(target_arch = "wasm32"))]
            token_key,
            #[cfg(not(target_arch = "wasm32"))]
            used_tokens: Mutex::new(HashSet::new()),
        }
    }

    /// Create a handler for testing with an ephemeral temp directory.
    #[cfg(test)]
    pub fn for_testing() -> Self {
        let suffix = fastrand::u64(..);
        let temp_dir = std::env::temp_dir().join(format!("aura-secure-test-{suffix}"));
        Self::with_base_path(temp_dir)
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
                "missing capability: {required:?}"
            )))
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn path_for(&self, location: &SecureStorageLocation) -> Result<PathBuf, SecureStorageError> {
        Self::validate_location(location)?;
        let mut path = self
            .base_path
            .join(Self::encode_component(&location.namespace))
            .join(Self::encode_component(&location.key));
        if let Some(sub) = &location.sub_key {
            path = path.join(Self::encode_component(sub));
        }
        Ok(path)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn encrypt_fallback_record(
        &self,
        location: &SecureStorageLocation,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SecureStorageError> {
        let cipher = ChaCha20Poly1305::new((&self.wrapping_key).into());
        let mut nonce = [0u8; FALLBACK_NONCE_LEN];
        getrandom::getrandom(&mut nonce).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: location.full_path().as_bytes(),
                },
            )
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;

        let mut record =
            Vec::with_capacity(FALLBACK_RECORD_MAGIC.len() + FALLBACK_NONCE_LEN + ciphertext.len());
        record.extend_from_slice(FALLBACK_RECORD_MAGIC);
        record.extend_from_slice(&nonce);
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn decrypt_fallback_record(
        &self,
        location: &SecureStorageLocation,
        record: &[u8],
    ) -> Result<Vec<u8>, SecureStorageError> {
        if !record.starts_with(FALLBACK_RECORD_MAGIC) {
            return Err(SecureStorageError::storage(
                "filesystem fallback secure record is not encrypted",
            ));
        }
        let nonce_start = FALLBACK_RECORD_MAGIC.len();
        let ciphertext_start = nonce_start + FALLBACK_NONCE_LEN;
        if record.len() < ciphertext_start {
            return Err(SecureStorageError::storage(
                "filesystem fallback secure record is truncated",
            ));
        }

        let cipher = ChaCha20Poly1305::new((&self.wrapping_key).into());
        cipher
            .decrypt(
                Nonce::from_slice(&record[nonce_start..ciphertext_start]),
                Payload {
                    msg: &record[ciphertext_start..],
                    aad: location.full_path().as_bytes(),
                },
            )
            .map_err(|e| SecureStorageError::storage(e.to_string()))
    }

    fn current_time_ms(&self) -> Result<u64, SecureStorageError> {
        current_time_ms()
    }

    #[cfg(target_arch = "wasm32")]
    fn wasm_storage(&self) -> FilesystemStorageHandler {
        FilesystemStorageHandler::new(self.base_path.clone())
    }

    fn validate_location(location: &SecureStorageLocation) -> Result<(), SecureStorageError> {
        Self::validate_component("namespace", &location.namespace)?;
        Self::validate_component("key", &location.key)?;
        if let Some(sub_key) = &location.sub_key {
            Self::validate_component("sub_key", sub_key)?;
        }
        Ok(())
    }

    fn validate_component(label: &str, value: &str) -> Result<(), SecureStorageError> {
        if value.is_empty() {
            return Err(SecureStorageError::invalid(format!(
                "secure storage {label} cannot be empty"
            )));
        }
        if value == "." || value == ".." {
            return Err(SecureStorageError::invalid(format!(
                "secure storage {label} cannot be a directory traversal segment"
            )));
        }
        if value.contains('/') || value.contains('\\') {
            return Err(SecureStorageError::invalid(format!(
                "secure storage {label} cannot contain path separators"
            )));
        }
        if value.contains('\0') {
            return Err(SecureStorageError::invalid(format!(
                "secure storage {label} cannot contain NUL bytes"
            )));
        }
        if Self::is_windows_drive_prefix(value) {
            return Err(SecureStorageError::invalid(format!(
                "secure storage {label} cannot be a Windows drive prefix"
            )));
        }
        Ok(())
    }

    fn is_windows_drive_prefix(value: &str) -> bool {
        let bytes = value.as_bytes();
        bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn encode_component(component: &str) -> String {
        let mut encoded = String::with_capacity(component.len());
        for byte in component.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    encoded.push(byte as char)
                }
                _ => {
                    encoded.push('%');
                    encoded.push(Self::hex_digit(byte >> 4));
                    encoded.push(Self::hex_digit(byte & 0x0f));
                }
            }
        }
        encoded
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn decode_component(component: &str) -> Result<String, SecureStorageError> {
        let bytes = component.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            }
            if index + 2 >= bytes.len() {
                return Err(SecureStorageError::invalid(
                    "stored secure storage component has invalid escape",
                ));
            }
            let high = Self::hex_value(bytes[index + 1]).ok_or_else(|| {
                SecureStorageError::invalid("stored secure storage component has invalid escape")
            })?;
            let low = Self::hex_value(bytes[index + 2]).ok_or_else(|| {
                SecureStorageError::invalid("stored secure storage component has invalid escape")
            })?;
            decoded.push((high << 4) | low);
            index += 3;
        }
        String::from_utf8(decoded).map_err(|_| {
            SecureStorageError::invalid("stored secure storage component is not valid UTF-8")
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn hex_digit(value: u8) -> char {
        match value {
            0..=9 => (b'0' + value) as char,
            10..=15 => (b'A' + (value - 10)) as char,
            _ => '?',
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn hex_value(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }
}

#[async_trait]
impl SecureStorageEffects for FilesystemFallbackSecureStorageHandler {
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        key: &[u8],
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        #[cfg(target_arch = "wasm32")]
        {
            Self::validate_location(location)?;
            return self
                .wasm_storage()
                .store(&location.full_path(), key.to_vec())
                .await
                .map_err(|e| SecureStorageError::storage(e.to_string()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.path_for(location)?;
            let record = self.encrypt_fallback_record(location, key)?;
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| SecureStorageError::storage(e.to_string()))?;
            }
            write_private_file(&path, &record)?;
            Ok(())
        }
    }

    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Read)?;
        #[cfg(target_arch = "wasm32")]
        {
            Self::validate_location(location)?;
            return self
                .wasm_storage()
                .retrieve(&location.full_path())
                .await
                .map_err(|e| SecureStorageError::storage(e.to_string()))?
                .ok_or_else(|| SecureStorageError::storage("secure key not found"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.path_for(location)?;
            let record = fs::read(&path).map_err(|e| SecureStorageError::storage(e.to_string()))?;
            self.decrypt_fallback_record(location, &record)
        }
    }

    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Delete)?;
        #[cfg(target_arch = "wasm32")]
        {
            Self::validate_location(location)?;
            let _ = self
                .wasm_storage()
                .remove(&location.full_path())
                .await
                .map_err(|e| SecureStorageError::storage(e.to_string()))?;
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.path_for(location)?;
            if path.exists() {
                fs::remove_file(&path).map_err(|e| SecureStorageError::storage(e.to_string()))?;
            }
            Ok(())
        }
    }

    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError> {
        #[cfg(target_arch = "wasm32")]
        {
            Self::validate_location(location)?;
            return self
                .wasm_storage()
                .exists(&location.full_path())
                .await
                .map_err(|e| SecureStorageError::storage(e.to_string()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.path_for(location)?;
            Ok(path.exists())
        }
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::List)?;
        Self::validate_component("namespace", namespace)?;
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .wasm_storage()
                .list_keys(Some(&format!("{namespace}/")))
                .await
                .map_err(|e| SecureStorageError::storage(e.to_string()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let ns_path = self.base_path.join(Self::encode_component(namespace));
            if !ns_path.exists() {
                return Ok(Vec::new());
            }
            let mut keys = Vec::new();
            for entry in
                fs::read_dir(&ns_path).map_err(|e| SecureStorageError::storage(e.to_string()))?
            {
                let entry = entry.map_err(|e| SecureStorageError::storage(e.to_string()))?;
                if let Some(name) = entry.file_name().to_str() {
                    keys.push(Self::decode_component(name)?);
                }
            }
            Ok(keys)
        }
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        context: &str,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<SecureGeneratedKey, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let mut key = [0u8; 32];
        getrandom::getrandom(&mut key).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        let mut material = key.to_vec();
        material.extend_from_slice(context.as_bytes());
        self.secure_store(location, &material, caps).await?;
        Ok(SecureGeneratedKey::OpaqueHandle(location.full_path()))
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        caps: &[aura_core::effects::SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Read)?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (location, expires_at);
            Err(SecureStorageError::storage(
                "authenticated secure access tokens are unavailable for wasm filesystem fallback",
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            create_authenticated_access_token(
                &self.token_key,
                &self.platform_config,
                location,
                caps,
                expires_at.ts_ms,
            )
        }
    }

    async fn secure_access_with_token(
        &self,
        token: &[u8],
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (token, location);
            Err(SecureStorageError::storage(
                "authenticated secure access tokens are unavailable for wasm filesystem fallback",
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let capabilities = verify_authenticated_access_token(
                &self.token_key,
                &self.platform_config,
                token,
                location,
                &self.used_tokens,
            )?;
            self.secure_retrieve(location, &capabilities).await
        }
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
        #[derive(serde::Serialize)]
        struct Attestation<'a> {
            platform: &'a str,
            issued_at_ms: u64,
            capabilities: Vec<String>,
        }

        let issued_at_ms = self.current_time_ms()?;

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
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_filesystem_fallback_secure_storage_store_and_retrieve() {
        let temp = match tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("create tempdir: {err}"),
        };
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            let raw = fs::read(handler.path_for(&location).unwrap()).unwrap();
            assert!(
                !raw.windows(b"data".len()).any(|window| window == b"data"),
                "filesystem fallback secure record stored plaintext"
            );
            assert!(raw.starts_with(FALLBACK_RECORD_MAGIC));
            assert!(
                !raw.windows(handler.wrapping_key.len())
                    .any(|window| window == handler.wrapping_key),
                "filesystem fallback secure record stored wrapping key bytes"
            );
            assert!(
                !temp
                    .path()
                    .join("secure_store")
                    .join(".filesystem-fallback-wrap-key")
                    .exists(),
                "filesystem fallback must not persist wrapping key material"
            );
        }
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

    #[tokio::test]
    async fn filesystem_fallback_secure_storage_rejects_path_components() {
        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let capabilities = vec![SecureStorageCapability::Write];

        for location in [
            SecureStorageLocation::new("", "key"),
            SecureStorageLocation::new("../namespace", "key"),
            SecureStorageLocation::new("/absolute", "key"),
            SecureStorageLocation::new("C:", "key"),
            SecureStorageLocation::new("namespace", "../key"),
            SecureStorageLocation::new("namespace", "key/child"),
            SecureStorageLocation::new("namespace", "key\\child"),
            SecureStorageLocation::with_sub_key("namespace", "key", ".."),
            SecureStorageLocation::with_sub_key("namespace", "key", "sub/child"),
            SecureStorageLocation::with_sub_key("namespace", "key", "sub\0child"),
        ] {
            let error = handler
                .secure_store(&location, b"blocked", &capabilities)
                .await
                .unwrap_err();
            assert!(
                matches!(error, SecureStorageError::Invalid { .. }),
                "expected invalid location for {location:?}, got {error:?}"
            );
        }

        assert!(!temp.path().join("secure_store").join("key").exists());
    }

    #[tokio::test]
    async fn filesystem_fallback_secure_storage_encodes_and_lists_safe_components() {
        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let capabilities = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
            SecureStorageCapability::List,
        ];
        let location = SecureStorageLocation::new("ns:one", "key:one");

        handler
            .secure_store(&location, b"secret", &capabilities)
            .await
            .unwrap();
        assert_eq!(
            handler
                .secure_retrieve(&location, &capabilities)
                .await
                .unwrap(),
            b"secret"
        );
        assert_eq!(
            handler
                .secure_list_keys("ns:one", &capabilities)
                .await
                .unwrap(),
            vec!["key:one".to_string()]
        );
    }

    #[tokio::test]
    #[cfg(not(target_arch = "wasm32"))]
    async fn secure_access_tokens_are_authenticated_bound_and_one_time() {
        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let capabilities = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
            SecureStorageCapability::Delete,
        ];
        let location = SecureStorageLocation::new("tokens", "primary");
        let other_location = SecureStorageLocation::new("tokens", "other");

        handler
            .secure_store(&location, b"secret-data", &capabilities)
            .await
            .unwrap();
        handler
            .secure_store(&other_location, b"other-data", &capabilities)
            .await
            .unwrap();

        let token = handler
            .secure_create_time_bound_token(
                &location,
                &[SecureStorageCapability::Read],
                &aura_core::time::PhysicalTime {
                    ts_ms: current_time_ms().unwrap() + 60_000,
                    uncertainty: None,
                },
            )
            .await
            .unwrap();

        assert!(handler
            .secure_access_with_token(&token, &other_location)
            .await
            .is_err());
        assert_eq!(
            handler
                .secure_access_with_token(&token, &location)
                .await
                .unwrap(),
            b"secret-data"
        );
        assert!(handler
            .secure_access_with_token(&token, &location)
            .await
            .is_err());
    }

    #[tokio::test]
    #[cfg(not(target_arch = "wasm32"))]
    async fn secure_access_tokens_reject_forgery_expiry_and_wrong_capability() {
        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let location = SecureStorageLocation::new("tokens", "primary");
        handler
            .secure_store(
                &location,
                b"secret-data",
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .unwrap();

        assert!(handler
            .secure_access_with_token(
                b"tokens/primary:999999999999:filesystem-fallback",
                &location
            )
            .await
            .is_err());

        let expired = handler
            .secure_create_time_bound_token(
                &location,
                &[SecureStorageCapability::Read],
                &aura_core::time::PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                },
            )
            .await
            .unwrap();
        assert!(handler
            .secure_access_with_token(&expired, &location)
            .await
            .is_err());

        let wrong_capability = create_authenticated_access_token(
            &handler.token_key,
            &handler.platform_config,
            &location,
            &[SecureStorageCapability::Write],
            current_time_ms().unwrap() + 60_000,
        )
        .unwrap();
        assert!(handler
            .secure_access_with_token(&wrong_capability, &location)
            .await
            .is_err());
    }
}
