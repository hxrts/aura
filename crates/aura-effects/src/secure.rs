//! Layer 3: Secure Storage Effect Handlers - Production Only
//!
//! Stateless single-party implementation of SecureStorageEffects from aura-core (Layer 1).
//! This handler implements pure secure storage effect operations, delegating to platform APIs.
//!
//! **Layer Constraint**: No mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production stateless handlers.

use async_trait::async_trait;
use aura_core::crypto::{ed25519_verifying_key, Ed25519SigningKey};
use aura_core::effects::{
    SecureGeneratedKey, SecureStorageCapability, SecureStorageEffects, SecureStorageError,
    SecureStorageLocation,
};
#[cfg(not(target_arch = "wasm32"))]
use aura_core::AuraError;
use cfg_if::cfg_if;
#[cfg(not(target_arch = "wasm32"))]
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
#[cfg(target_arch = "wasm32")]
use indexed_db_futures::{
    database::Database, prelude::*, query_source::QuerySource, transaction::TransactionMode,
};
#[cfg(target_arch = "wasm32")]
use js_sys::{Array, Object, Reflect, Uint8Array};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
use std::io::{Read, Write};
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::Mutex;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::{AesGcmParams, CryptoKey, SubtleCrypto};

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
const FALLBACK_WRAPPING_KEY_FILENAME: &str = ".filesystem-fallback-wrap-key";
#[cfg(not(target_arch = "wasm32"))]
const SECURE_ACCESS_TOKEN_VERSION: u8 = 1;
#[cfg(not(target_arch = "wasm32"))]
const SECURE_ACCESS_TOKEN_NONCE_LEN: usize = 12;
#[cfg(not(target_arch = "wasm32"))]
const SECURE_ACCESS_TOKEN_AAD_DOMAIN: &str = "aura:secure-storage-access-token:v1";
#[cfg(target_arch = "wasm32")]
const WASM_SECURE_DB_VERSION: u8 = 1;
#[cfg(target_arch = "wasm32")]
const WASM_SECURE_RECORD_STORE: &str = "secure_records";
#[cfg(target_arch = "wasm32")]
const WASM_SECURE_WRAPPING_KEY_STORE: &str = "wrapping_keys";
#[cfg(target_arch = "wasm32")]
const WASM_SECURE_WRAPPING_KEY_ID: &str = "aura-secure-storage-wrapping-key-v1";
#[cfg(target_arch = "wasm32")]
const WASM_SECURE_RECORD_MAGIC: &[u8] = b"AURA-WASM-SECURE-V1";
#[cfg(target_arch = "wasm32")]
const WASM_SECURE_NONCE_LEN: usize = 12;

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
fn validate_private_directory_metadata(
    path: &std::path::Path,
    metadata: &fs::Metadata,
) -> Result<(), SecureStorageError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SecureStorageError::storage(format!(
            "secure-storage directory is not a private directory: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 {
            return Err(SecureStorageError::storage(format!(
                "secure-storage directory permissions are not private: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
fn ensure_private_directory(path: &std::path::Path) -> Result<(), SecureStorageError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        validate_private_directory_metadata(path, &metadata)?;
    }
    fs::create_dir_all(path).map_err(|e| SecureStorageError::storage(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|e| SecureStorageError::storage(e.to_string()))?;
    validate_private_directory_metadata(path, &metadata)
}

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
fn validate_private_file_metadata(
    path: &std::path::Path,
    metadata: &fs::Metadata,
) -> Result<(), SecureStorageError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SecureStorageError::storage(format!(
            "secure-storage file is not a private regular file: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 {
            return Err(SecureStorageError::storage(format!(
                "secure-storage file permissions are not private: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn open_private_file_no_follow(
    path: &std::path::Path,
    create_new: bool,
) -> Result<fs::File, SecureStorageError> {
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc_o_no_follow());
    }
    options
        .open(path)
        .map_err(|e| SecureStorageError::storage(e.to_string()))
}

#[cfg(all(unix, not(target_arch = "wasm32"), test))]
fn libc_o_no_follow() -> i32 {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        0x20000
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd"
    ))]
    {
        0x0100
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd"
    )))]
    {
        0
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn create_private_file_no_follow(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), SecureStorageError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        validate_private_file_metadata(path, &metadata)?;
        return Err(SecureStorageError::storage(format!(
            "secure-storage file already exists: {}",
            path.display()
        )));
    }
    let mut file = open_private_file_no_follow(path, true)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    }
    file.write_all(bytes)
        .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    file.sync_all()
        .map_err(|e| SecureStorageError::storage(e.to_string()))
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn write_private_file_atomic_no_follow(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), SecureStorageError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        validate_private_file_metadata(path, &metadata)?;
    }
    let parent = path
        .parent()
        .ok_or_else(|| SecureStorageError::storage("secure-storage file has no parent"))?;
    ensure_private_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| SecureStorageError::storage("secure-storage file name is invalid"))?;
    let nonce = generate_secret_bytes(8)?;
    let tmp_path = parent.join(format!(".{file_name}.tmp-{}", hex::encode(nonce)));
    create_private_file_no_follow(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        SecureStorageError::storage(e.to_string())
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|e| SecureStorageError::storage(e.to_string()))?;
    validate_private_file_metadata(path, &metadata)
}

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
fn read_existing_private_file(
    path: &std::path::Path,
) -> Result<Option<Vec<u8>>, SecureStorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SecureStorageError::storage(error.to_string())),
    };
    validate_private_file_metadata(path, &metadata)?;

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc_o_no_follow());
    }
    let mut file = options
        .open(path)
        .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| SecureStorageError::storage(e.to_string()))?;
    Ok(Some(bytes))
}

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
fn publish_private_file_immutable(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
    publish_private_file_immutable_at(path, bytes, |_| Ok(()))
}

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
#[derive(Clone, Copy, Debug)]
enum ImmutablePublicationCheckpoint {
    Staged,
    Published,
}

#[cfg(all(not(target_arch = "wasm32"), any(not(unix), test)))]
fn publish_private_file_immutable_at(
    path: &std::path::Path,
    bytes: &[u8],
    mut checkpoint: impl FnMut(ImmutablePublicationCheckpoint) -> Result<(), std::io::Error>,
) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
    use aura_core::effects::secure::ImmutableSecureStoreOutcome;

    let io = |source: std::io::Error| aura_core::AuraError::Storage {
        message: "immutable secure record publication failed".into(),
        source: Some(std::sync::Arc::new(source)),
    };
    let parent = path
        .parent()
        .ok_or_else(|| SecureStorageError::storage("record has no parent"))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| SecureStorageError::storage("invalid record name"))?;
    let temporary = parent.join(format!(
        ".{name}.immutable-{}",
        hex::encode(generate_secret_bytes(16)?)
    ));
    // Existing helper writes all encrypted bytes and syncs the private inode.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc_o_no_follow());
    }
    let mut staged = options.open(&temporary).map_err(io)?;
    staged.write_all(bytes).map_err(io)?;
    staged.sync_all().map_err(io)?;
    drop(staged);

    checkpoint(ImmutablePublicationCheckpoint::Staged).map_err(io)?;
    let published = match fs::hard_link(&temporary, path) {
        Ok(()) => Ok(ImmutableSecureStoreOutcome::Created),
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(io)?;
            validate_private_file_metadata(path, &metadata)?;
            Ok(ImmutableSecureStoreOutcome::AlreadyExists)
        }
        Err(source) => Err(io(source)),
    };
    if matches!(published, Ok(ImmutableSecureStoreOutcome::Created)) {
        checkpoint(ImmutablePublicationCheckpoint::Published).map_err(io)?;
    }
    // A crash can leave an orphan encrypted temporary inode, never a partial
    // published record. Recovery may clean this namespace under profile lease.
    fs::remove_file(&temporary).map_err(io)?;
    let outcome = published?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(io)?;
    Ok(outcome)
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
    if let Err(error) = getrandom::getrandom(&mut key) {
        panic!("secure storage requires OS randomness: {error}");
    }
    key
}

fn generate_secret_bytes(len: usize) -> Result<Vec<u8>, SecureStorageError> {
    let mut bytes = vec![0u8; len];
    getrandom::getrandom(&mut bytes).map_err(|e| SecureStorageError::storage(e.to_string()))?;
    Ok(bytes)
}

fn generate_secure_key_material(
    key_type: &str,
) -> Result<(Vec<u8>, Option<Vec<u8>>), SecureStorageError> {
    match key_type {
        "ed25519" => {
            let signing_key_bytes = generate_secret_bytes(32)?;
            let signing_key = Ed25519SigningKey::try_from(signing_key_bytes.as_slice())
                .map_err(|e| SecureStorageError::invalid(e.to_string()))?;
            let verifying_key = ed25519_verifying_key(&signing_key)
                .map_err(|e| SecureStorageError::invalid(e.to_string()))?
                .to_bytes()
                .to_vec();
            Ok((signing_key_bytes, Some(verifying_key)))
        }
        "frost-share" | "symmetric" | "aes256" | "xchacha20poly1305" => {
            Ok((generate_secret_bytes(32)?, None))
        }
        other => Err(SecureStorageError::invalid(format!(
            "unsupported secure key type: {other}"
        ))),
    }
}

fn generated_key_result(
    location: &SecureStorageLocation,
    public_material: Option<Vec<u8>>,
) -> SecureGeneratedKey {
    match public_material {
        Some(public_material) => SecureGeneratedKey::PublicMaterial(public_material),
        None => SecureGeneratedKey::OpaqueHandle(location.full_path()),
    }
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
async fn verify_authenticated_access_token(
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
    let mut used = used_tokens.lock().await;
    if !used.insert(token_id) {
        return Err(SecureStorageError::permission_denied(
            "secure access token has already been used",
        ));
    }
    Ok(claims.capabilities)
}

/// Actual provider/physical profile pairing minted only by the checked factory.
/// Neither arbitrary guards nor a real lease for another profile can construct it.
/// ```compile_fail
/// use aura_effects::secure::{ProfileOwnedSecureStorage,ProductionSecureStorageHandler};
/// fn counterfeit(owner:std::sync::Arc<aura_effects::profile_storage::OwnedProfileLease>) {
///     let _ = ProfileOwnedSecureStorage {
///         backend:Box::new(ProductionSecureStorageHandler::for_production("foreign".into())),
///         _owner:owner,
///     };
/// }
/// ```
#[derive(Debug)]
pub struct ProfileOwnedSecureStorage {
    backend: Box<ProductionSecureStorageHandler>,
    _owner: std::sync::Arc<crate::profile_storage::OwnedProfileLease>,
}
impl ProfileOwnedSecureStorage {
    /// Observed provider category; does not expose an unowned provider clone.
    pub fn uses_filesystem_fallback(&self) -> bool {
        matches!(
            self.backend.as_ref(),
            ProductionSecureStorageHandler::FilesystemFallback(_)
        )
    }
}

/// Production secure storage selector.
///
/// Production mode uses the platform credential store on supported native
/// targets and the WebCrypto/IndexedDB secure-storage adapter on wasm. Other
/// unsupported production targets fail closed instead of silently storing
/// plaintext secret material. Tests and simulations may explicitly construct the
/// named filesystem fallback variant.
#[derive(Debug)]
pub enum ProductionSecureStorageHandler {
    /// Infrastructure writer lifetime retains the actual selected profile owner.
    ProfileOwned(ProfileOwnedSecureStorage),

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
    /// Explicit fallback adapter. On wasm this is WebCrypto plus IndexedDB; on
    /// native targets this is filesystem-backed and non-production only.
    FilesystemFallback(FilesystemFallbackSecureStorageHandler),
    /// Fail-closed production handler for targets without platform support.
    UnavailablePlatform {
        /// Target label used in fail-closed error messages.
        target: &'static str,
    },
}

impl ProductionSecureStorageHandler {
    /// Arbitrary core effect guards cannot authorize this actual writer.
    /// ```compile_fail
    /// use aura_core::effects::profile_storage::ProfileStorageLease;
    /// use aura_effects::ProductionSecureStorageHandler;
    /// #[derive(Debug)] struct Noop;
    /// impl ProfileStorageLease for Noop {fn profile_identity(&self)->&str {"fake"}}
    /// let backend=ProductionSecureStorageHandler::for_production("fake".into());
    /// backend.retain_profile_owner(std::sync::Arc::new(Noop));
    /// ```
    pub fn retain_profile_owner(
        mut self,
        owner: std::sync::Arc<crate::profile_storage::OwnedProfileLease>,
    ) -> Result<Self, aura_core::effects::profile_storage::ProfileStorageError> {
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd"
        ))]
        if let Self::Platform(handler) = &mut self {
            // Preserve the original service/key addresses, while separately
            // excluding every cooperating profile using that shared OS namespace.
            handler.namespace_owner = Some(
                crate::platform_namespace::PlatformNamespaceLease::for_selected_profile(
                    &owner,
                    &handler.service,
                )?,
            );
        }
        if let Self::FilesystemFallback(handler) = &self {
            #[cfg(unix)]
            {
                let expected = owner
                    .directory
                    .child(std::path::Path::new("secure_store"), false)
                    .map_err(|source| {
                        aura_core::effects::profile_storage::ProfileStorageError::Io {
                            source: std::sync::Arc::new(source),
                        }
                    })?;
                if !handler
                    .owned_directory()
                    .map_err(|error| {
                        aura_core::effects::profile_storage::ProfileStorageError::Io {
                            source: std::sync::Arc::new(std::io::Error::other(error)),
                        }
                    })?
                    .same_directory(&expected)
                    .map_err(|source| {
                        aura_core::effects::profile_storage::ProfileStorageError::Io {
                            source: std::sync::Arc::new(source),
                        }
                    })?
                {
                    return Err(
                        aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                            "secure provider directory differs from selected owner".into(),
                        ),
                    );
                }
            }

            #[cfg(target_arch = "wasm32")]
            handler.require_no_legacy_browser_secure_records()?;

            let profile = handler.base_path.parent().ok_or_else(|| {
                aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                    "secure backend has no profile".into(),
                )
            })?;
            if !owner.matches_profile(profile)? {
                return Err(
                    aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                        "secure writer profile differs from owner".into(),
                    ),
                );
            }
        }
        if matches!(self, Self::ProfileOwned(_)) {
            return Err(
                aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                    "secure writer already has a bound profile owner".into(),
                ),
            );
        }
        Ok(Self::ProfileOwned(ProfileOwnedSecureStorage {
            backend: Box::new(self),
            _owner: owner,
        }))
    }

    /// Create production secure storage. Unsupported targets fail closed.
    pub fn for_production(base_path: PathBuf) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            return Self::FilesystemFallback(
                FilesystemFallbackSecureStorageHandler::with_base_path(base_path),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = base_path;
        #[cfg(all(
            not(target_arch = "wasm32"),
            any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            )
        ))]
        {
            Self::Platform(PlatformSecureStorageHandler::new())
        }
        #[cfg(all(
            not(target_arch = "wasm32"),
            not(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))
        ))]
        {
            Self::UnavailablePlatform {
                target: "unsupported",
            }
        }
    }

    /// Create the exact filesystem provider under its already-acquired owner.
    /// Directory creation, wrapping-key admission and all later IO use the
    /// owner's retained descriptor; no selected-profile path is reopened.
    #[cfg(unix)]
    pub fn filesystem_fallback_with_profile_owner(
        owner: std::sync::Arc<crate::profile_storage::OwnedProfileLease>,
    ) -> Result<Self, SecureStorageError> {
        use aura_core::effects::profile_storage::ProfileStorageLease;
        let physical = PathBuf::from(owner.profile_identity());
        let directory = owner
            .directory
            .child(std::path::Path::new("secure_store"), true)
            .map_err(|e| {
                FilesystemFallbackSecureStorageHandler::descriptor_error(
                    "open owned secure directory",
                    e,
                )
            })?;
        let (wrapping_key, filesystem_error) =
            FilesystemFallbackSecureStorageHandler::load_or_create_descriptor_wrapping_key(
                &directory,
            );
        if let Some(error) = filesystem_error {
            return Err(error);
        }
        let handler = FilesystemFallbackSecureStorageHandler {
            platform_config: "filesystem-fallback".into(),
            base_path: physical.join("secure_store"),
            wrapping_key,
            token_key: generate_secret_key(),
            filesystem_error: None,
            directory: Some(directory),
            used_tokens: Mutex::new(HashSet::new()),
        };
        Ok(Self::ProfileOwned(ProfileOwnedSecureStorage {
            backend: Box::new(Self::FilesystemFallback(handler)),
            _owner: owner,
        }))
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SecureStorageEffects for ProductionSecureStorageHandler {
    async fn secure_create_mutable(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_create_mutable(location, data, caps).await,
            Self::FilesystemFallback(handler) => {
                handler.secure_create_mutable(location, data, caps).await
            }
            Self::ProfileOwned(owned) => {
                owned
                    .backend
                    .secure_create_mutable(location, data, caps)
                    .await
            }
            _ => Err(aura_core::AuraError::Storage {
                message: "platform secure storage has no atomic immutable publication contract"
                    .into(),
                source: Some(std::sync::Arc::new(
                    aura_core::effects::secure::MutableSecureCreateUnsupported,
                )),
            }),
        }
    }

    async fn secure_store_immutable(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        match self {
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd"
            ))]
            Self::Platform(handler) => handler.secure_store_immutable(location, data, caps).await,
            Self::FilesystemFallback(handler) => {
                handler.secure_store_immutable(location, data, caps).await
            }
            Self::ProfileOwned(owned) => {
                owned
                    .backend
                    .secure_store_immutable(location, data, caps)
                    .await
            }
            _ => Err(aura_core::AuraError::Storage {
                message: "platform secure storage has no atomic immutable publication contract"
                    .into(),
                source: Some(std::sync::Arc::new(
                    aura_core::effects::secure::ImmutableSecureStoreUnsupported,
                )),
            }),
        }
    }

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
            Self::ProfileOwned(owned) => owned.backend.secure_store(location, data, caps).await,
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
            Self::ProfileOwned(owned) => owned.backend.secure_retrieve(location, caps).await,
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
            Self::ProfileOwned(owned) => owned.backend.secure_delete(location, caps).await,
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
            Self::ProfileOwned(owned) => owned.backend.secure_exists(location).await,
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
            Self::ProfileOwned(owned) => owned.backend.secure_list_keys(namespace, caps).await,
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
            Self::ProfileOwned(owned) => {
                owned
                    .backend
                    .secure_generate_key(location, key_type, caps)
                    .await
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
            Self::ProfileOwned(owned) => {
                owned
                    .backend
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
            Self::ProfileOwned(owned) => {
                owned
                    .backend
                    .secure_access_with_token(token, location)
                    .await
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
            Self::ProfileOwned(owned) => owned.backend.get_device_attestation().await,
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
            Self::ProfileOwned(owned) => owned.backend.is_secure_storage_available().await,
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
            Self::ProfileOwned(owned) => owned.backend.get_secure_storage_capabilities(),
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
    namespace_owner: Option<Arc<crate::platform_namespace::PlatformNamespaceLease>>,
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
            namespace_owner: None,
            service: PLATFORM_KEYRING_SERVICE.to_string(),
            platform_config: "platform-keyring".to_string(),
            token_key: generate_secret_key(),
            used_tokens: Mutex::new(HashSet::new()),
        }
    }

    fn namespace_owner(
        &self,
    ) -> Result<&crate::platform_namespace::PlatformNamespaceLease, SecureStorageError> {
        self.namespace_owner
            .as_deref()
            .ok_or_else(|| AuraError::Storage {
                message: "platform keyring operation requires owned service namespace".into(),
                source: Some(Arc::new(
                    aura_core::effects::profile_storage::ProfileStorageError::Unsupported,
                )),
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
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| AuraError::Serialization {
                message: "decode original keyring namespace index".into(),
                source: Some(Arc::new(e)),
            }),
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
        let bytes = serde_json::to_vec(keys).map_err(|e| AuraError::Serialization {
            message: "encode keyring namespace index".into(),
            source: Some(Arc::new(e)),
        })?;
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
        AuraError::Storage {
            message: "platform keyring operation failed".into(),
            source: Some(Arc::new(error)),
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
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SecureStorageEffects for PlatformSecureStorageHandler {
    async fn secure_create_mutable(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let _namespace = self.namespace_owner()?.mutation_guard().await;
        let entry = self.entry_for_location(location)?;
        match entry.get_secret() {
            Ok(_) => {
                self.add_index_key(location)?;
                Ok(aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists)
            }
            Err(keyring::Error::NoEntry) => {
                entry.set_secret(data).map_err(Self::map_keyring_error)?;
                self.add_index_key(location)?;
                Ok(aura_core::effects::secure::ImmutableSecureStoreOutcome::Created)
            }
            Err(error) => Err(Self::map_keyring_error(error)),
        }
    }

    async fn secure_store_immutable(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let _namespace = self.namespace_owner()?.mutation_guard().await;
        let entry = self.entry_for_location(location)?;
        match entry.get_secret() {
            Ok(_) => {
                self.add_index_key(location)?;
                Ok(aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists)
            }
            Err(keyring::Error::NoEntry) => {
                entry.set_secret(data).map_err(Self::map_keyring_error)?;
                self.add_index_key(location)?;
                Ok(aura_core::effects::secure::ImmutableSecureStoreOutcome::Created)
            }
            Err(error) => Err(Self::map_keyring_error(error)),
        }
    }
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        let _namespace = self.namespace_owner()?.mutation_guard().await;
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
        let _namespace = self.namespace_owner()?.mutation_guard().await;
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
        let _namespace = self.namespace_owner()?.mutation_guard().await;
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
        let _namespace = self.namespace_owner()?.mutation_guard().await;
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
        let _namespace = self.namespace_owner()?.mutation_guard().await;
        self.require_capability(caps, SecureStorageCapability::List)?;
        self.load_namespace_index(namespace)
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        key_type: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<SecureGeneratedKey, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let (secret_material, public_material) = generate_secure_key_material(key_type)?;
        self.secure_store(location, &secret_material, caps).await?;
        Ok(generated_key_result(location, public_material))
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
        )
        .await?;
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
    filesystem_error: Option<SecureStorageError>,
    #[cfg(unix)]
    directory: Option<crate::profile_directory::ProfileDirectory>,
    #[cfg(not(target_arch = "wasm32"))]
    used_tokens: Mutex<HashSet<[u8; 32]>>,
}

#[cfg(unix)]
#[derive(Debug)]
struct InvalidWrappingKeyLength {
    actual: usize,
}
#[cfg(unix)]
impl std::fmt::Display for InvalidWrappingKeyLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "expected 32 wrapping-key bytes, received {}",
            self.actual
        )
    }
}
#[cfg(unix)]
impl std::error::Error for InvalidWrappingKeyLength {}

impl FilesystemFallbackSecureStorageHandler {
    /// Create a filesystem fallback secure storage handler with a custom base path.
    ///
    /// The secure storage files will be placed in `base_path/secure_store/`.
    pub fn with_base_path(base_path: PathBuf) -> Self {
        let secure_store_path = base_path.join("secure_store");
        // Resolve and reject directory aliases before reading or creating any
        // wrapping key. A failed provider stays failed; no fresh key repairs it.
        #[cfg(unix)]
        let (secure_store_path, directory_error) =
            match fs::create_dir_all(&base_path).and_then(|_| {
                crate::profile_storage::create_contained_directory(
                    &base_path,
                    std::path::Path::new("secure_store"),
                )
            }) {
                Ok(physical) => (physical, None),
                Err(source) => (
                    secure_store_path,
                    Some(SecureStorageError::Storage {
                        message: "resolve contained secure storage directory".into(),
                        source: Some(std::sync::Arc::new(source)),
                    }),
                ),
            };
        #[cfg(unix)]
        let (directory, directory_error) = match directory_error {
            Some(error) => (None, Some(error)),
            None => match crate::profile_directory::ProfileDirectory::open(&secure_store_path) {
                Ok(directory) => (Some(directory), None),
                Err(source) => (
                    None,
                    Some(Self::descriptor_error("open secure directory", source)),
                ),
            },
        };
        #[cfg(not(target_arch = "wasm32"))]
        let (wrapping_key, filesystem_error) = {
            #[cfg(unix)]
            if let Some(error) = directory_error {
                ([0; 32], Some(error))
            } else {
                match &directory {
                    Some(directory) => Self::load_or_create_descriptor_wrapping_key(directory),
                    None => (
                        [0; 32],
                        Some(SecureStorageError::internal(
                            "missing owned secure directory",
                        )),
                    ),
                }
            }
            #[cfg(not(unix))]
            Self::load_or_create_wrapping_key(&secure_store_path)
        };

        #[cfg(not(target_arch = "wasm32"))]
        let token_key = generate_secret_key();
        Self {
            platform_config: "filesystem-fallback".to_string(),
            base_path: secure_store_path,
            #[cfg(not(target_arch = "wasm32"))]
            wrapping_key,
            #[cfg(not(target_arch = "wasm32"))]
            token_key,
            #[cfg(not(target_arch = "wasm32"))]
            filesystem_error,
            #[cfg(unix)]
            directory,
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

    #[cfg(unix)]
    fn descriptor_error(operation: &str, source: std::io::Error) -> SecureStorageError {
        SecureStorageError::Storage {
            message: operation.into(),
            source: Some(std::sync::Arc::new(source)),
        }
    }
    #[cfg(unix)]
    fn owned_directory(
        &self,
    ) -> Result<&crate::profile_directory::ProfileDirectory, SecureStorageError> {
        self.require_filesystem_available()?;
        self.directory
            .as_ref()
            .ok_or_else(|| SecureStorageError::internal("secure directory absent"))
    }
    #[cfg(unix)]
    fn descriptor_path(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<PathBuf, SecureStorageError> {
        self.path_for(location)?
            .strip_prefix(&self.base_path)
            .map(std::path::Path::to_path_buf)
            .map_err(|_| SecureStorageError::invalid("secure path escaped provider"))
    }
    #[cfg(unix)]
    fn load_or_create_descriptor_wrapping_key(
        directory: &crate::profile_directory::ProfileDirectory,
    ) -> ([u8; 32], Option<SecureStorageError>) {
        let load = || -> Result<[u8; 32], SecureStorageError> {
            directory
                .require_private()
                .map_err(|e| Self::descriptor_error("validate secure directory", e))?;
            let path = std::path::Path::new(FALLBACK_WRAPPING_KEY_FILENAME);
            let decode = |bytes: Vec<u8>| -> Result<[u8; 32], SecureStorageError> {
                bytes
                    .try_into()
                    .map_err(|bytes: Vec<u8>| SecureStorageError::Storage {
                        message: "secure wrapping key has invalid length".into(),
                        source: Some(std::sync::Arc::new(InvalidWrappingKeyLength {
                            actual: bytes.len(),
                        })),
                    })
            };
            if let Some(bytes) = directory
                .read(path, true)
                .map_err(|e| Self::descriptor_error("read wrapping key", e))?
            {
                return decode(bytes);
            }
            let key = generate_secret_key();
            let prepared = directory
                .prepare_private(path, &key)
                .map_err(|e| Self::descriptor_error("prepare wrapping key", e))?;
            let created = prepared
                .publish(true)
                .map_err(|e| Self::descriptor_error("publish wrapping key", e))?;
            prepared
                .acknowledge()
                .map_err(|e| Self::descriptor_error("acknowledge wrapping key", e))?;
            if created {
                Ok(key)
            } else {
                decode(
                    directory
                        .read(path, true)
                        .map_err(|e| Self::descriptor_error("read original wrapping key", e))?
                        .ok_or_else(|| {
                            SecureStorageError::storage("original wrapping key disappeared")
                        })?,
                )
            }
        };
        match load() {
            Ok(key) => (key, None),
            Err(error) => ([0; 32], Some(error)),
        }
    }
    #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
    fn wrapping_key_path(secure_store_path: &std::path::Path) -> PathBuf {
        secure_store_path.join(FALLBACK_WRAPPING_KEY_FILENAME)
    }

    #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
    fn load_or_create_wrapping_key(
        secure_store_path: &std::path::Path,
    ) -> ([u8; 32], Option<SecureStorageError>) {
        let key_path = Self::wrapping_key_path(secure_store_path);
        match read_existing_private_file(&key_path) {
            Ok(Some(bytes)) => {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    return (key, None);
                }
                let error = format!(
                    "filesystem fallback secure-storage wrapping key had invalid length: {}",
                    bytes.len()
                );
                tracing::warn!(
                    path = %key_path.display(),
                    len = bytes.len(),
                    "Filesystem fallback secure-storage wrapping key had invalid length"
                );
                return ([0; 32], Some(SecureStorageError::storage(error)));
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    path = %key_path.display(),
                    err = %error,
                    "Filesystem fallback secure-storage wrapping key failed validation"
                );
                return ([0; 32], Some(error));
            }
        }

        let wrapping_key = generate_secret_key();
        if let Err(error) = ensure_private_directory(secure_store_path) {
            tracing::warn!(
                path = %secure_store_path.display(),
                err = %error,
                "Failed to create filesystem fallback secure-storage directory"
            );
            return (wrapping_key, Some(error));
        }
        if let Err(error) = create_private_file_no_follow(&key_path, &wrapping_key) {
            tracing::warn!(
                path = %key_path.display(),
                err = %error,
                "Failed to persist filesystem fallback wrapping key"
            );
            return (wrapping_key, Some(error));
        }
        (wrapping_key, None)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn require_filesystem_available(&self) -> Result<(), SecureStorageError> {
        match &self.filesystem_error {
            Some(error) => Err(SecureStorageError::Storage {
                message: "filesystem fallback secure-storage unavailable".into(),
                source: Some(std::sync::Arc::new(error.clone())),
            }),
            None => Ok(()),
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
    fn require_no_legacy_browser_secure_records(
        &self,
    ) -> Result<(), aura_core::effects::profile_storage::ProfileStorageError> {
        use aura_core::effects::profile_storage::ProfileStorageError;
        let window = web_sys::window().ok_or(ProfileStorageError::Unsupported)?;
        let storage = window
            .local_storage()
            .map_err(|e| {
                crate::profile_storage::browser_profile_error("legacy secure store lookup", e)
            })?
            .ok_or(ProfileStorageError::Unsupported)?;
        // Historical wasm_storage() used exactly this hash of base/secure_store.
        // Its plaintext records are never selected, migrated or deleted silently.
        let digest = aura_core::hash::hash(self.base_path.to_string_lossy().as_bytes());
        let namespace = format!("aura_storage_{}", hex::encode(&digest[..8]));
        let prefix = format!("{namespace}::");
        let count = storage.length().map_err(|e| {
            crate::profile_storage::browser_profile_error("legacy secure store length", e)
        })?;
        if count > 32_768 {
            return Err(ProfileStorageError::Invalid(
                "browser storage inventory exceeds ownership admission bound".into(),
            ));
        }
        for index in 0..count {
            if let Some(key) = storage.key(index).map_err(|e| {
                crate::profile_storage::browser_profile_error("legacy secure store key", e)
            })? {
                if key.starts_with(&prefix) {
                    return Err(ProfileStorageError::LegacyBrowserSecureStorage { namespace });
                }
            }
        }
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    fn wasm_secure_db_name(&self) -> String {
        let path = self.base_path.to_string_lossy();
        let digest = aura_core::hash::hash(path.as_bytes());
        format!("hxrts_aura_secure_storage_{}", hex::encode(&digest[..8]))
    }

    #[cfg(target_arch = "wasm32")]
    fn map_js_error(context: &str, error: impl std::fmt::Debug) -> SecureStorageError {
        SecureStorageError::storage(format!("{context}: {error:?}"))
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_open_secure_db(&self) -> Result<Database, SecureStorageError> {
        Database::open(self.wasm_secure_db_name())
            .with_version(WASM_SECURE_DB_VERSION)
            .with_on_upgrade_needed(|_, db| {
                if !db
                    .object_store_names()
                    .any(|name| name == WASM_SECURE_RECORD_STORE)
                {
                    db.create_object_store(WASM_SECURE_RECORD_STORE).build()?;
                }
                if !db
                    .object_store_names()
                    .any(|name| name == WASM_SECURE_WRAPPING_KEY_STORE)
                {
                    db.create_object_store(WASM_SECURE_WRAPPING_KEY_STORE)
                        .build()?;
                }
                Ok(())
            })
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure storage open failed", e))
    }

    #[cfg(target_arch = "wasm32")]
    fn wasm_subtle_crypto() -> Result<SubtleCrypto, SecureStorageError> {
        let window = web_sys::window().ok_or_else(|| {
            SecureStorageError::storage("WebCrypto secure storage unavailable: window missing")
        })?;
        let crypto = window.crypto().map_err(|e| {
            Self::map_js_error(
                "WebCrypto secure storage unavailable: crypto lookup failed",
                e,
            )
        })?;
        Ok(crypto.subtle())
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_generate_wrapping_key() -> Result<CryptoKey, SecureStorageError> {
        let algorithm = Object::new();
        Reflect::set(
            &algorithm,
            &JsValue::from_str("name"),
            &JsValue::from_str("AES-GCM"),
        )
        .map_err(|e| Self::map_js_error("WebCrypto AES-GCM algorithm setup failed", e))?;
        Reflect::set(
            &algorithm,
            &JsValue::from_str("length"),
            &JsValue::from_f64(256.0),
        )
        .map_err(|e| Self::map_js_error("WebCrypto AES-GCM key length setup failed", e))?;

        let usages = Array::new();
        usages.push(&JsValue::from_str("encrypt"));
        usages.push(&JsValue::from_str("decrypt"));

        let promise = Self::wasm_subtle_crypto()?
            .generate_key_with_object(&algorithm, false, &usages.into())
            .map_err(|e| {
                Self::map_js_error("WebCrypto non-extractable key generation failed", e)
            })?;
        let key = JsFuture::from(promise).await.map_err(|e| {
            Self::map_js_error("WebCrypto non-extractable key generation rejected", e)
        })?;
        key.dyn_into::<CryptoKey>().map_err(|e| {
            Self::map_js_error("WebCrypto key generation returned unexpected value", e)
        })
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_wrapping_key(&self) -> Result<CryptoKey, SecureStorageError> {
        let db = self.wasm_open_secure_db().await?;
        let transaction = db
            .transaction(WASM_SECURE_WRAPPING_KEY_STORE)
            .build()
            .map_err(|e| Self::map_js_error("IndexedDB wrapping-key transaction failed", e))?;
        let store = transaction
            .object_store(WASM_SECURE_WRAPPING_KEY_STORE)
            .map_err(|e| Self::map_js_error("IndexedDB wrapping-key store lookup failed", e))?;
        let stored: Option<JsValue> = store
            .get(WASM_SECURE_WRAPPING_KEY_ID)
            .primitive()
            .map_err(|e| Self::map_js_error("IndexedDB wrapping-key read request failed", e))?
            .await
            .map_err(|e| Self::map_js_error("IndexedDB wrapping-key read failed", e))?;
        if let Some(stored) = stored {
            return stored.dyn_into::<CryptoKey>().map_err(|e| {
                Self::map_js_error("IndexedDB wrapping-key record is not a CryptoKey", e)
            });
        }

        Self::require_strict_idb_durability()?;
        let candidate = Self::wasm_generate_wrapping_key().await?;
        use indexed_db_futures::transaction::{TransactionDurability, TransactionOptions};
        let mut options = TransactionOptions::default();
        options.set_durability(TransactionDurability::Strict);
        let transaction = db
            .transaction(WASM_SECURE_WRAPPING_KEY_STORE)
            .with_mode(TransactionMode::Readwrite)
            .with_options(options)
            .build()
            .map_err(|e| Self::map_atomic_idb_error("wrapping-key ownership transaction", e))?;
        let store = transaction
            .object_store(WASM_SECURE_WRAPPING_KEY_STORE)
            .map_err(|e| Self::map_atomic_idb_error("wrapping-key store", e))?;
        // First creation can race within one runtime too. Recheck under the
        // exact serial readwrite transaction and use the winning actual key.
        let existing: Option<JsValue> = store
            .get(WASM_SECURE_WRAPPING_KEY_ID)
            .primitive()
            .map_err(|e| Self::map_atomic_idb_error("wrapping-key read request", e))?
            .await
            .map_err(|e| Self::map_atomic_idb_error("wrapping-key read", e))?;
        let key = if let Some(existing) = existing {
            existing
                .dyn_into::<CryptoKey>()
                .map_err(|e| Self::map_js_error("stored wrapping-key is not CryptoKey", e))?
        } else {
            store
                .add(JsValue::from(candidate.clone()))
                .with_key(WASM_SECURE_WRAPPING_KEY_ID)
                .primitive()
                .map_err(|e| Self::map_atomic_idb_error("wrapping-key create request", e))?
                .await
                .map_err(|e| Self::map_atomic_idb_error("wrapping-key create", e))?;
            candidate
        };
        transaction
            .commit()
            .await
            .map_err(|e| Self::map_atomic_idb_error("wrapping-key durable commit", e))?;
        Ok(key)
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_encrypt_record(
        &self,
        location: &SecureStorageLocation,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SecureStorageError> {
        let wrapping_key = self.wasm_wrapping_key().await?;
        let mut nonce = [0u8; WASM_SECURE_NONCE_LEN];
        getrandom::getrandom(&mut nonce).map_err(|e| SecureStorageError::storage(e.to_string()))?;
        let params = AesGcmParams::new_with_u8_slice("AES-GCM", &mut nonce);
        let mut aad = location.full_path().into_bytes();
        params.set_additional_data_u8_slice(&mut aad);
        params.set_tag_length(128);

        let promise = Self::wasm_subtle_crypto()?
            .encrypt_with_object_and_u8_array(&params, &wrapping_key, plaintext)
            .map_err(|e| Self::map_js_error("WebCrypto secure-record encryption failed", e))?;
        let encrypted = JsFuture::from(promise)
            .await
            .map_err(|e| Self::map_js_error("WebCrypto secure-record encryption rejected", e))?;
        let ciphertext = Uint8Array::new(&encrypted).to_vec();

        let mut record = Vec::with_capacity(
            WASM_SECURE_RECORD_MAGIC.len() + WASM_SECURE_NONCE_LEN + ciphertext.len(),
        );
        record.extend_from_slice(WASM_SECURE_RECORD_MAGIC);
        record.extend_from_slice(&nonce);
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_decrypt_record(
        &self,
        location: &SecureStorageLocation,
        record: &[u8],
    ) -> Result<Vec<u8>, SecureStorageError> {
        if !record.starts_with(WASM_SECURE_RECORD_MAGIC) {
            return Err(SecureStorageError::storage(
                "wasm secure-storage record is not encrypted",
            ));
        }
        let nonce_start = WASM_SECURE_RECORD_MAGIC.len();
        let ciphertext_start = nonce_start + WASM_SECURE_NONCE_LEN;
        if record.len() < ciphertext_start {
            return Err(SecureStorageError::storage(
                "wasm secure-storage record is truncated",
            ));
        }

        let wrapping_key = self.wasm_wrapping_key().await?;
        let mut nonce = record[nonce_start..ciphertext_start].to_vec();
        let params = AesGcmParams::new_with_u8_slice("AES-GCM", &mut nonce);
        let mut aad = location.full_path().into_bytes();
        params.set_additional_data_u8_slice(&mut aad);
        params.set_tag_length(128);

        let promise = Self::wasm_subtle_crypto()?
            .decrypt_with_object_and_u8_array(&params, &wrapping_key, &record[ciphertext_start..])
            .map_err(|e| Self::map_js_error("WebCrypto secure-record decryption failed", e))?;
        let plaintext = JsFuture::from(promise)
            .await
            .map_err(|e| Self::map_js_error("WebCrypto secure-record decryption rejected", e))?;
        Ok(Uint8Array::new(&plaintext).to_vec())
    }

    #[cfg(target_arch = "wasm32")]
    fn require_strict_idb_durability() -> Result<(), SecureStorageError> {
        let foreign = |operation, e| aura_core::AuraError::Storage {
            message: "IndexedDB durability capability failed".into(),
            source: Some(std::sync::Arc::new(
                crate::profile_storage::browser_profile_error(operation, e),
            )),
        };
        let global = js_sys::global();

        let constructor = js_sys::Reflect::get(&global, &"IDBTransaction".into())
            .map_err(|e| foreign("IndexedDB transaction capability lookup", e))?;
        if constructor.is_null() || constructor.is_undefined() {
            return Err(aura_core::AuraError::Storage {
                message: "browser IndexedDB is unavailable".into(),
                source: Some(std::sync::Arc::new(
                    aura_core::effects::secure::ImmutableSecureStoreUnsupported,
                )),
            });
        }
        let prototype = js_sys::Reflect::get(&constructor, &"prototype".into())
            .map_err(|e| foreign("IndexedDB transaction prototype lookup", e))?;
        if !js_sys::Reflect::has(&prototype, &"durability".into())
            .map_err(|e| foreign("IndexedDB durability lookup", e))?
        {
            return Err(aura_core::AuraError::Storage {
                message: "browser lacks strict IndexedDB durability".into(),
                source: Some(std::sync::Arc::new(
                    aura_core::effects::secure::ImmutableSecureStoreUnsupported,
                )),
            });
        }
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    fn map_atomic_idb_error(
        operation: &'static str,
        error: indexed_db_futures::error::Error,
    ) -> SecureStorageError {
        // Foreign object causes cannot implement Send+Sync Rust Error. Keep the
        // typed library error category and diagnostic at this explicit boundary.
        #[derive(Debug, thiserror::Error)]
        #[error("{operation}: {category}: {diagnostic}")]
        struct BrowserImmutableStorageFailure {
            operation: &'static str,
            category: &'static str,
            diagnostic: String,
        }
        let category = match &error {
            indexed_db_futures::error::Error::DomException(_) => "dom_exception",
            indexed_db_futures::error::Error::Serialisation(_) => "serialization",
            indexed_db_futures::error::Error::MissingData(_) => "missing_data",
            indexed_db_futures::error::Error::Unknown(_) => "foreign_error",
        };
        let cause = BrowserImmutableStorageFailure {
            operation,
            category,
            diagnostic: format!("{error:?}"),
        };
        aura_core::AuraError::Storage {
            message: "immutable IndexedDB operation failed".into(),
            source: Some(std::sync::Arc::new(cause)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_publish_record_immutable(
        &self,
        location: &SecureStorageLocation,
        record: &[u8],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        use aura_core::effects::secure::ImmutableSecureStoreOutcome;
        use indexed_db_futures::transaction::{TransactionDurability, TransactionOptions};
        let db = self.wasm_open_secure_db().await?;
        let mut options = TransactionOptions::default();
        options.set_durability(TransactionDurability::Strict);
        let transaction = db
            .transaction(WASM_SECURE_RECORD_STORE)
            .with_mode(TransactionMode::Readwrite)
            .with_options(options)
            .build()
            .map_err(|e| Self::map_atomic_idb_error("transaction", e))?;
        let store = transaction
            .object_store(WASM_SECURE_RECORD_STORE)
            .map_err(|e| Self::map_atomic_idb_error("record store", e))?;
        // The absence check and publication are inside one serial readwrite
        // transaction on the exact store; no separate get/store transaction.
        let existing: Option<JsValue> = store
            .get(location.full_path())
            .primitive()
            .map_err(|e| Self::map_atomic_idb_error("existing request", e))?
            .await
            .map_err(|e| Self::map_atomic_idb_error("existing response", e))?;
        let outcome = if existing.is_some() {
            ImmutableSecureStoreOutcome::AlreadyExists
        } else {
            store
                .add(JsValue::from(Uint8Array::from(record)))
                .with_key(location.full_path())
                .primitive()
                .map_err(|e| Self::map_atomic_idb_error("publish request", e))?
                .await
                .map_err(|e| Self::map_atomic_idb_error("publish response", e))?;
            ImmutableSecureStoreOutcome::Created
        };
        transaction
            .commit()
            .await
            .map_err(|e| Self::map_atomic_idb_error("durable commit", e))?;
        Ok(outcome)
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_put_record(
        &self,
        location: &SecureStorageLocation,
        record: &[u8],
    ) -> Result<(), SecureStorageError> {
        let db = self.wasm_open_secure_db().await?;
        let transaction = db
            .transaction(WASM_SECURE_RECORD_STORE)
            .with_mode(TransactionMode::Readwrite)
            .build()
            .map_err(|e| {
                Self::map_js_error("IndexedDB secure-record write transaction failed", e)
            })?;
        let store = transaction
            .object_store(WASM_SECURE_RECORD_STORE)
            .map_err(|e| Self::map_js_error("IndexedDB secure-record store lookup failed", e))?;
        let value = Uint8Array::from(record);
        store
            .put(JsValue::from(value))
            .with_key(location.full_path())
            .primitive()
            .map_err(|e| Self::map_js_error("IndexedDB secure-record write request failed", e))?
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure-record write failed", e))?;
        transaction
            .commit()
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure-record commit failed", e))
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_get_record(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let db = self.wasm_open_secure_db().await?;
        let transaction = db
            .transaction(WASM_SECURE_RECORD_STORE)
            .build()
            .map_err(|e| {
                Self::map_js_error("IndexedDB secure-record read transaction failed", e)
            })?;
        let store = transaction
            .object_store(WASM_SECURE_RECORD_STORE)
            .map_err(|e| Self::map_js_error("IndexedDB secure-record store lookup failed", e))?;
        let stored: Option<JsValue> = store
            .get(location.full_path())
            .primitive()
            .map_err(|e| Self::map_js_error("IndexedDB secure-record read request failed", e))?
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure-record read failed", e))?;
        Ok(stored.map(|value| Uint8Array::new(&value).to_vec()))
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_delete_record(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<(), SecureStorageError> {
        let db = self.wasm_open_secure_db().await?;
        let transaction = db
            .transaction(WASM_SECURE_RECORD_STORE)
            .with_mode(TransactionMode::Readwrite)
            .build()
            .map_err(|e| {
                Self::map_js_error("IndexedDB secure-record delete transaction failed", e)
            })?;
        let store = transaction
            .object_store(WASM_SECURE_RECORD_STORE)
            .map_err(|e| Self::map_js_error("IndexedDB secure-record store lookup failed", e))?;
        store
            .delete(location.full_path())
            .primitive()
            .map_err(|e| Self::map_js_error("IndexedDB secure-record delete request failed", e))?
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure-record delete failed", e))?;
        transaction
            .commit()
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure-record delete commit failed", e))
    }

    #[cfg(target_arch = "wasm32")]
    async fn wasm_list_record_keys(
        &self,
        namespace: &str,
    ) -> Result<Vec<String>, SecureStorageError> {
        let db = self.wasm_open_secure_db().await?;
        let transaction = db
            .transaction(WASM_SECURE_RECORD_STORE)
            .build()
            .map_err(|e| {
                Self::map_js_error("IndexedDB secure-record list transaction failed", e)
            })?;
        let store = transaction
            .object_store(WASM_SECURE_RECORD_STORE)
            .map_err(|e| Self::map_js_error("IndexedDB secure-record store lookup failed", e))?;
        let keys = store
            .get_all_keys::<String>()
            .primitive()
            .map_err(|e| Self::map_js_error("IndexedDB secure-record list request failed", e))?
            .await
            .map_err(|e| Self::map_js_error("IndexedDB secure-record list failed", e))?;
        let prefix = format!("{namespace}/");
        keys.into_iter()
            .filter_map(|key| match key {
                Ok(key) => key.strip_prefix(&prefix).map(ToOwned::to_owned).map(Ok),
                Err(error) => Some(Err(Self::map_js_error(
                    "IndexedDB secure-record key decode failed",
                    error,
                ))),
            })
            .collect()
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
                    encoded.push(byte as char);
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SecureStorageEffects for FilesystemFallbackSecureStorageHandler {
    async fn secure_create_mutable(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        #[cfg(unix)]
        {
            self.require_filesystem_available()?;
            let directory = self.owned_directory()?;
            let path = self.descriptor_path(location)?;
            let record = self.encrypt_fallback_record(location, data)?;
            let prepared = directory
                .prepare_private(&path, &record)
                .map_err(|e| Self::descriptor_error("prepare immutable secure value", e))?;
            let created = prepared
                .publish(true)
                .map_err(|e| Self::descriptor_error("publish immutable secure value", e))?;
            prepared
                .acknowledge()
                .map_err(|e| Self::descriptor_error("acknowledge immutable secure value", e))?;
            if !created {
                directory
                    .read(&path, true)
                    .map_err(|e| {
                        Self::descriptor_error("validate original immutable secure value", e)
                    })?
                    .ok_or_else(|| {
                        SecureStorageError::storage("original immutable secure value disappeared")
                    })?;
            }
            Ok(if created {
                aura_core::effects::secure::ImmutableSecureStoreOutcome::Created
            } else {
                aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
            })
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
            let path = self.path_for(location)?;
            let record = self.encrypt_fallback_record(location, data)?;
            if let Some(parent) = path.parent() {
                ensure_private_directory(parent)?;
            }
            let outcome = publish_private_file_immutable(&path, &record)?;
            let mut ancestor = path.parent();
            while let Some(directory) = ancestor {
                let source = fs::File::open(directory).and_then(|f| f.sync_all());
                source.map_err(|source| aura_core::AuraError::Storage {
                    message: "sync immutable secure namespace failed".into(),
                    source: Some(std::sync::Arc::new(source)),
                })?;
                if Some(directory) == self.base_path.parent() {
                    break;
                }
                ancestor = directory.parent();
            }
            Ok(outcome)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::validate_location(location)?;
            Self::require_strict_idb_durability()?;
            let record = self.wasm_encrypt_record(location, data).await?;
            self.wasm_publish_record_immutable(location, &record).await
        }
    }

    async fn secure_store_immutable(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<aura_core::effects::secure::ImmutableSecureStoreOutcome, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        #[cfg(unix)]
        {
            self.require_filesystem_available()?;
            let directory = self.owned_directory()?;
            let path = self.descriptor_path(location)?;
            let record = self.encrypt_fallback_record(location, data)?;
            let prepared = directory
                .prepare_private(&path, &record)
                .map_err(|e| Self::descriptor_error("prepare immutable secure value", e))?;
            let created = prepared
                .publish(true)
                .map_err(|e| Self::descriptor_error("publish immutable secure value", e))?;
            prepared
                .acknowledge()
                .map_err(|e| Self::descriptor_error("acknowledge immutable secure value", e))?;
            if !created {
                directory
                    .read(&path, true)
                    .map_err(|e| {
                        Self::descriptor_error("validate original immutable secure value", e)
                    })?
                    .ok_or_else(|| {
                        SecureStorageError::storage("original immutable secure value disappeared")
                    })?;
            }
            Ok(if created {
                aura_core::effects::secure::ImmutableSecureStoreOutcome::Created
            } else {
                aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
            })
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
            let path = self.path_for(location)?;
            let record = self.encrypt_fallback_record(location, data)?;
            if let Some(parent) = path.parent() {
                ensure_private_directory(parent)?;
            }
            let outcome = publish_private_file_immutable(&path, &record)?;
            let mut ancestor = path.parent();
            while let Some(directory) = ancestor {
                let source = fs::File::open(directory).and_then(|f| f.sync_all());
                source.map_err(|source| aura_core::AuraError::Storage {
                    message: "sync immutable secure namespace failed".into(),
                    source: Some(std::sync::Arc::new(source)),
                })?;
                if Some(directory) == self.base_path.parent() {
                    break;
                }
                ancestor = directory.parent();
            }
            Ok(outcome)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::validate_location(location)?;
            Self::require_strict_idb_durability()?;
            let record = self.wasm_encrypt_record(location, data).await?;
            self.wasm_publish_record_immutable(location, &record).await
        }
    }

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
            let record = self.wasm_encrypt_record(location, key).await?;
            return self.wasm_put_record(location, &record).await;
        }
        #[cfg(unix)]
        {
            let path = self.descriptor_path(location)?;
            let record = self.encrypt_fallback_record(location, key)?;
            let prepared = self
                .owned_directory()?
                .prepare_private(&path, &record)
                .map_err(|e| Self::descriptor_error("prepare secure replacement", e))?;
            prepared
                .publish(false)
                .map_err(|e| Self::descriptor_error("publish secure replacement", e))?;
            prepared
                .acknowledge()
                .map_err(|e| Self::descriptor_error("acknowledge secure replacement", e))?;
            Ok(())
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
            let path = self.path_for(location)?;
            let record = self.encrypt_fallback_record(location, key)?;
            if let Some(dir) = path.parent() {
                ensure_private_directory(dir)?;
            }
            write_private_file_atomic_no_follow(&path, &record)?;
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
            let record = self
                .wasm_get_record(location)
                .await?
                .ok_or_else(|| SecureStorageError::storage("secure key not found"))?;
            return self.wasm_decrypt_record(location, &record).await;
        }
        #[cfg(unix)]
        {
            let record = self
                .owned_directory()?
                .read(&self.descriptor_path(location)?, true)
                .map_err(|e| Self::descriptor_error("read secure value", e))?
                .ok_or_else(|| SecureStorageError::storage("secure key not found"))?;
            self.decrypt_fallback_record(location, &record)
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
            let path = self.path_for(location)?;
            let record = read_existing_private_file(&path)?
                .ok_or_else(|| SecureStorageError::storage("secure key not found"))?;
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
            return self.wasm_delete_record(location).await;
        }
        #[cfg(unix)]
        {
            self.owned_directory()?
                .remove(&self.descriptor_path(location)?)
                .map_err(|e| Self::descriptor_error("remove secure value", e))?;
            Ok(())
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
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
                .wasm_get_record(location)
                .await
                .map(|record| record.is_some());
        }
        #[cfg(unix)]
        {
            self.owned_directory()?
                .read(&self.descriptor_path(location)?, true)
                .map(|value| value.is_some())
                .map_err(|e| Self::descriptor_error("inspect secure value", e))
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
            let path = self.path_for(location)?;
            Ok(fs::symlink_metadata(&path)
                .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                .unwrap_or(false))
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
            return self.wasm_list_record_keys(namespace).await;
        }
        #[cfg(unix)]
        {
            let directory = match self.owned_directory()?.child(
                std::path::Path::new(&Self::encode_component(namespace)),
                false,
            ) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(Self::descriptor_error("open secure namespace", e)),
            };
            let mut keys = Vec::new();
            for name in directory
                .names()
                .map_err(|e| Self::descriptor_error("enumerate secure namespace", e))?
            {
                let name = name
                    .to_str()
                    .ok_or_else(|| SecureStorageError::invalid("secure key name is not UTF-8"))?;
                if !name.starts_with(".aura-stage-") {
                    keys.push(Self::decode_component(name)?);
                }
            }
            Ok(keys)
        }
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        {
            self.require_filesystem_available()?;
            let ns_path = self.base_path.join(Self::encode_component(namespace));
            let metadata = match fs::symlink_metadata(&ns_path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Vec::new());
                }
                Err(error) => return Err(SecureStorageError::storage(error.to_string())),
            };
            validate_private_directory_metadata(&ns_path, &metadata)?;
            if !metadata.is_dir() {
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
        key_type: &str,
        caps: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<SecureGeneratedKey, SecureStorageError> {
        self.require_capability(caps, SecureStorageCapability::Write)?;
        let (secret_material, public_material) = generate_secure_key_material(key_type)?;
        self.secure_store(location, &secret_material, caps).await?;
        Ok(generated_key_result(location, public_material))
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
                "authenticated secure access tokens are unavailable for wasm WebCrypto IndexedDB secure storage",
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
                "authenticated secure access tokens are unavailable for wasm WebCrypto IndexedDB secure storage",
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
            )
            .await?;
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
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    async fn atomic_mutable_creation_retains_winner_and_allows_owned_update_after_reopen() {
        use aura_core::effects::secure::ImmutableSecureStoreOutcome::{AlreadyExists, Created};
        let temp = tempdir().expect("isolated physical provider");
        let handler = FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().into());
        let location = SecureStorageLocation::new("mutable_create_fixture", "checkpoint");
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        let (first, second) = futures::join!(
            handler.secure_create_mutable(&location, b"first", &caps),
            handler.secure_create_mutable(&location, b"second", &caps),
        );
        let winner = match (
            first.expect("first publisher"),
            second.expect("second publisher"),
        ) {
            (Created, AlreadyExists) => b"first".as_slice(),
            (AlreadyExists, Created) => b"second".as_slice(),
            other => panic!("exactly one atomic initial publication: {other:?}"),
        };
        assert_eq!(
            handler
                .secure_retrieve(&location, &caps)
                .await
                .expect("retained winner"),
            winner
        );
        assert_eq!(
            handler
                .secure_create_mutable(&location, b"replacement", &caps)
                .await
                .expect("repeat"),
            AlreadyExists
        );
        drop(handler);
        let reopened = FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().into());
        assert_eq!(
            reopened
                .secure_retrieve(&location, &caps)
                .await
                .expect("original across reopen"),
            winner
        );
        reopened
            .secure_store(&location, b"owned checkpoint", &caps)
            .await
            .expect("mutable policy permits update");
        assert_eq!(
            reopened
                .secure_retrieve(&location, &caps)
                .await
                .expect("checkpoint"),
            b"owned checkpoint"
        );
    }

    #[test]
    fn immutable_publication_faults_leave_absent_or_complete_encrypted_records() {
        for fail_after_publish in [false, true] {
            let directory = tempdir().unwrap();
            let handler =
                FilesystemFallbackSecureStorageHandler::with_base_path(directory.path().into());
            let location = SecureStorageLocation::new("fault_fixture", "actual_admission");
            let record = handler
                .encrypt_fallback_record(&location, b"original admission")
                .unwrap();
            let path = handler.path_for(&location).unwrap();
            ensure_private_directory(path.parent().unwrap()).unwrap();
            let result = publish_private_file_immutable_at(&path, &record, |stage| {
                if matches!(
                    (fail_after_publish, stage),
                    (false, ImmutablePublicationCheckpoint::Staged)
                        | (true, ImmutablePublicationCheckpoint::Published)
                ) {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "injected publication crash boundary",
                    ))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            let recovered = read_existing_private_file(&path).unwrap();
            if fail_after_publish {
                let bytes = recovered.expect("publication is complete before failure");
                assert_eq!(
                    handler.decrypt_fallback_record(&location, &bytes).unwrap(),
                    b"original admission"
                );
                assert_eq!(
                    publish_private_file_immutable(&path, &record).unwrap(),
                    aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
                );
            } else {
                assert!(
                    recovered.is_none(),
                    "unpublished staging file cannot become trusted admission"
                );
                assert_eq!(
                    publish_private_file_immutable(&path, &record).unwrap(),
                    aura_core::effects::secure::ImmutableSecureStoreOutcome::Created
                );
            }
        }
    }

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
                temp.path()
                    .join("secure_store")
                    .join(FALLBACK_WRAPPING_KEY_FILENAME)
                    .exists(),
                "filesystem fallback must persist wrapping key material for reopen"
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
    async fn filesystem_fallback_secure_storage_reopens_with_same_wrapping_key() {
        let temp = tempdir().expect("tempdir");
        let location = SecureStorageLocation::new("persist", "primary");
        let capabilities = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        let writer =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        writer
            .secure_store(&location, b"persisted-secret", &capabilities)
            .await
            .expect("store secret");

        let reader =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let loaded = reader
            .secure_retrieve(&location, &capabilities)
            .await
            .expect("retrieve persisted secret after reopen");

        assert_eq!(loaded, b"persisted-secret");
    }

    #[tokio::test]
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    async fn filesystem_fallback_secure_storage_rejects_symlinked_wrapping_key() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempdir().expect("tempdir");
        let secure_store = temp.path().join("secure_store");
        fs::create_dir_all(&secure_store).expect("secure-store dir");
        fs::set_permissions(&secure_store, fs::Permissions::from_mode(0o700))
            .expect("private secure-store dir");
        let target = temp.path().join("target-key");
        fs::write(&target, [9u8; 32]).expect("target key");
        let key_path = secure_store.join(FALLBACK_WRAPPING_KEY_FILENAME);
        symlink(&target, &key_path).expect("wrapping-key symlink");

        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let error = handler
            .secure_store(
                &SecureStorageLocation::new("wrap_symlink", "record"),
                b"secret",
                &[SecureStorageCapability::Write],
            )
            .await
            .expect_err("symlinked wrapping key should disable filesystem fallback");

        assert!(
            error
                .to_string()
                .contains("filesystem fallback secure-storage unavailable"),
            "unexpected error: {error:?}"
        );
        assert_eq!(fs::read(&target).expect("target preserved"), [9u8; 32]);
    }

    #[tokio::test]
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    async fn filesystem_fallback_secure_storage_rejects_symlinked_record_path() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let location = SecureStorageLocation::new("records", "primary");
        let record_path = handler.path_for(&location).expect("record path");
        let record_dir = record_path.parent().expect("record parent");
        fs::create_dir_all(record_dir).expect("record dir");
        fs::set_permissions(record_dir, fs::Permissions::from_mode(0o700))
            .expect("private record dir");
        let target = temp.path().join("record-target");
        fs::write(&target, b"do-not-truncate").expect("target file");
        symlink(&target, &record_path).expect("record symlink");

        let error = handler
            .secure_store(&location, b"new-secret", &[SecureStorageCapability::Write])
            .await
            .expect_err("symlinked record path should be rejected");

        assert!(
            error.to_string().contains("not a private regular file"),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"do-not-truncate"
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

    #[tokio::test]
    async fn secure_generate_key_returns_only_public_material_for_ed25519() {
        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let location = SecureStorageLocation::new("keys", "ed25519");
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        let generated = handler
            .secure_generate_key(&location, "ed25519", &caps)
            .await
            .expect("generate ed25519 key");
        let stored = handler
            .secure_retrieve(&location, &caps)
            .await
            .expect("retrieve generated secret");

        let public_material = match generated {
            SecureGeneratedKey::PublicMaterial(public_material) => public_material,
            other => panic!("expected public material, got {other:?}"),
        };
        assert_eq!(public_material.len(), 32);
        assert_eq!(stored.len(), 32);
        assert_ne!(stored, public_material);
        assert!(
            !stored
                .windows("ed25519".len())
                .any(|window| window == b"ed25519"),
            "stored secret material must not include key type metadata"
        );
    }

    #[tokio::test]
    async fn secure_generate_key_returns_only_handles_for_secret_key_types() {
        let temp = tempdir().expect("tempdir");
        let handler =
            FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().to_path_buf());
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        for key_type in ["frost-share", "symmetric"] {
            let location = SecureStorageLocation::new("keys", key_type);
            let generated = handler
                .secure_generate_key(&location, key_type, &caps)
                .await
                .unwrap_or_else(|err| panic!("generate {key_type} key: {err}"));
            let stored = handler
                .secure_retrieve(&location, &caps)
                .await
                .unwrap_or_else(|err| panic!("retrieve {key_type} secret: {err}"));

            assert_eq!(
                generated,
                SecureGeneratedKey::OpaqueHandle(location.full_path())
            );
            assert_eq!(stored.len(), 32);
            assert!(
                !stored
                    .windows(key_type.len())
                    .any(|window| window == key_type.as_bytes()),
                "stored {key_type} secret material must not include type metadata"
            );
        }
    }

    #[test]
    fn wasm_secure_storage_does_not_delegate_to_plain_storage() {
        let source = include_str!("secure.rs");
        assert!(
            !source.contains(concat!(".wasm", "_storage()")),
            "wasm secure storage must not route secret material through plaintext StorageEffects"
        );
        assert!(
            source.contains("wasm_encrypt_record"),
            "wasm secure storage must encrypt records before persistence"
        );
        assert!(
            source.contains("WASM_SECURE_RECORD_STORE"),
            "wasm secure storage must persist encrypted records in its IndexedDB store"
        );
    }
}

#[cfg(all(test, unix))]
mod descriptor_wrapping_key_tests {
    use super::*;
    #[tokio::test]
    async fn malformed_original_wrapping_key_is_typed_and_never_replaced(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let directory = crate::profile_directory::ProfileDirectory::open(temp.path())?
            .child(std::path::Path::new("secure_store"), true)?;
        let path = std::path::Path::new(FALLBACK_WRAPPING_KEY_FILENAME);
        let prepared = directory.prepare_private(path, b"malformed")?;
        assert!(prepared.publish(true)?);
        prepared.acknowledge()?;
        let handler = FilesystemFallbackSecureStorageHandler::with_base_path(temp.path().into());
        let Some(SecureStorageError::Storage {
            source: Some(cause),
            ..
        }) = &handler.filesystem_error
        else {
            return Err("missing typed wrapping-key failure".into());
        };
        let cause = cause
            .downcast_ref::<InvalidWrappingKeyLength>()
            .ok_or("missing original length cause")?;
        assert_eq!(cause.actual, b"malformed".len());
        assert_eq!(handler.wrapping_key, [0; 32]);
        assert!(handler
            .secure_store(
                &SecureStorageLocation::new("new", "secret"),
                b"not-written",
                &[SecureStorageCapability::Write]
            )
            .await
            .is_err());
        assert_eq!(directory.read(path, true)?, Some(b"malformed".to_vec()));
        assert!(!temp.path().join("secure_store/new").exists());
        Ok(())
    }
}
