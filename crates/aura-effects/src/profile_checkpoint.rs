//! Simulation-only, effect-owned checkpoint of a quiescent filesystem profile.
//! The archive never exposes record bytes; platform keyring profiles are refused.

use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::profile_storage::FilesystemProfileStorageHandler;

const MAX_FILES: usize = 256;
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_AUXILIARY_BYTES: usize = 256 * 1024;
const MARKER: &str = ".aura-checkpoint-restore-in-progress";

/// Fail-closed reasons for a quiescent filesystem checkpoint operation.
#[derive(Debug, thiserror::Error)]
pub enum ProfileCheckpointError {
    /// Another live owner holds the original profile lease.
    #[error("profile checkpoint owner is busy")]
    Busy,
    /// The selected provider or profile shape cannot be checkpointed.
    #[error("profile checkpoint source or selected provider is unsupported")]
    Unsupported,
    /// The requested profile, source revision or participant differs.
    #[error("profile checkpoint source binding differs")]
    ForeignSource,
    /// The ciphertext or key provenance failed authentication.
    #[error("profile checkpoint authentication failed")]
    Authentication,
    /// Native cryptographic entropy was unavailable.
    #[error("profile checkpoint entropy failed: {0}")]
    Entropy(#[source] rand::Error),
    /// The record inventory exceeds limits or has foreign paths.
    #[error("profile checkpoint inventory is invalid or over budget")]
    Inventory,
    /// An added branch file is outside the selected protected provider.
    #[error("profile checkpoint branch file is outside selected provider")]
    ForeignBranchPath,
    /// An added secure-provider file has no canonical provider location.
    #[error("profile checkpoint branch secure path is noncanonical")]
    NoncanonicalSecureBranchPath,
    /// An added selected-provider file lacks the canonical protected shape.
    #[error("profile checkpoint branch secure record is noncanonical")]
    NoncanonicalSecureBranchRecord,
    /// A selected allocation-lifetime owner could not authenticate a branch reset.
    #[error("profile checkpoint branch allocation lifetime is not quiescent or authenticated")]
    UnverifiedAllocationBranch,
    /// An ordinary branch file does not authenticate through selected storage.
    #[error("profile checkpoint branch ordinary record failed authentication")]
    UnauthenticatedOrdinaryBranchRecord,
    /// Native descriptor-relative profile IO failed.
    #[error("profile checkpoint IO failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Serialize, Deserialize)]
struct Record {
    path: PathBuf,
    bytes: Vec<u8>,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
struct Archive {
    schema: u8,
    records: Vec<Record>,
}

#[derive(Serialize, Deserialize)]
struct ArchiveV2 {
    schema: u8,
    records: Vec<Record>,
    auxiliary: Vec<u8>,
}

impl Drop for ArchiveV2 {
    fn drop(&mut self) {
        self.auxiliary.zeroize();
    }
}

/// Explicit nonsecret context for the simulator-only auxiliary payload. It
/// binds the payload kind/version, original owner, epoch, fixture configuration
/// and selected physical-clock observation; it grants no capture authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuxiliaryCheckpointContext {
    /// Domain-separated kind of the opaque auxiliary payload.
    pub kind: [u8; 16],
    /// Version of that payload codec.
    pub version: u16,
    /// Original stopped owner identity.
    pub owner: [u8; 32],
    /// Original owner epoch; a different epoch cannot restore this artifact.
    pub epoch: u64,
    /// Configuration digest of the compatible simulator fixture.
    pub fixture: [u8; 32],
    /// Original selected physical-clock observation in milliseconds.
    pub selected_clock_ms: u64,
}

/// A protected ciphertext artifact bound to the original profile path,
/// source revision, participant identity and filesystem secure provider.
/// Its key is derived inside the effect adapter and never serialized.
pub struct QuiescentFilesystemProfileCheckpoint {
    artifact_path: PathBuf,
    source: String,
    revision: [u8; 32],
    participant: [u8; 32],
    auxiliary_context: Option<AuxiliaryCheckpointContext>,
}

/// One-use evidence issued by the actual selected, profile-owned filesystem
/// secure-storage handler before its runtime acknowledges shutdown. It holds
/// no live lease or secret and cannot authorize a different profile.
pub struct SelectedFilesystemCheckpointSource {
    source: String,
}

impl SelectedFilesystemCheckpointSource {
    pub(crate) fn from_owned_selection(source: String) -> Self {
        Self { source }
    }
}

#[derive(Serialize, Deserialize)]
struct ProtectedArtifact {
    schema: u8,
    source: String,
    revision: [u8; 32],
    participant: [u8; 32],
    salt: [u8; 32],
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct ProtectedArtifactV2 {
    schema: u8,
    source: String,
    revision: [u8; 32],
    participant: [u8; 32],
    context: AuxiliaryCheckpointContext,
    salt: [u8; 32],
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

impl QuiescentFilesystemProfileCheckpoint {
    fn binding(source: &str, revision: &[u8; 32], participant: &[u8; 32]) -> Vec<u8> {
        let mut binding = b"aura-filesystem-profile-checkpoint-v1\0".to_vec();
        binding.extend_from_slice(&(source.len() as u64).to_le_bytes());
        binding.extend_from_slice(source.as_bytes());
        binding.extend_from_slice(revision);
        binding.extend_from_slice(participant);
        binding
    }

    fn auxiliary_binding(
        source: &str,
        revision: &[u8; 32],
        participant: &[u8; 32],
        context: &AuxiliaryCheckpointContext,
    ) -> Vec<u8> {
        let mut binding = b"aura-filesystem-profile-checkpoint-v2\0".to_vec();
        binding.extend_from_slice(&(source.len() as u64).to_le_bytes());
        binding.extend_from_slice(source.as_bytes());
        binding.extend_from_slice(revision);
        binding.extend_from_slice(participant);
        binding.extend_from_slice(&context.kind);
        binding.extend_from_slice(&context.version.to_le_bytes());
        binding.extend_from_slice(&context.owner);
        binding.extend_from_slice(&context.epoch.to_le_bytes());
        binding.extend_from_slice(&context.fixture);
        binding.extend_from_slice(&context.selected_clock_ms.to_le_bytes());
        binding
    }

    fn owner(
        path: &Path,
    ) -> Result<crate::profile_storage::OwnedProfileLease, ProfileCheckpointError> {
        use aura_core::effects::profile_storage::ProfileStorageError;
        FilesystemProfileStorageHandler::new(path.to_path_buf())
            .acquire_owned_native()
            .map_err(|error| match error {
                ProfileStorageError::Busy => ProfileCheckpointError::Busy,
                _ => ProfileCheckpointError::Unsupported,
            })
    }

    fn require_filesystem_provider(
        directory: &crate::profile_directory::ProfileDirectory,
    ) -> Result<Zeroizing<[u8; 32]>, ProfileCheckpointError> {
        let secure = directory
            .child(
                Path::new(crate::profile_storage::SECURE_PROVIDER_DIRECTORY),
                false,
            )
            .map_err(|_| ProfileCheckpointError::Unsupported)?;
        secure
            .require_private()
            .map_err(|_| ProfileCheckpointError::Unsupported)?;
        let wrapping_key = Zeroizing::new(
            secure
                .read(Path::new(".filesystem-fallback-wrap-key"), true)?
                .ok_or(ProfileCheckpointError::Unsupported)?,
        );
        let key: [u8; 32] = wrapping_key
            .as_slice()
            .try_into()
            .map_err(|_| ProfileCheckpointError::Unsupported)?;
        Ok(Zeroizing::new(key))
    }

    fn derived_key(
        wrapping_key: &[u8; 32],
        salt: &[u8; 32],
        binding: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, ProfileCheckpointError> {
        let material = Zeroizing::new(
            aura_core::crypto::kdf::derive_key_material(wrapping_key, salt, binding, 32)
                .map_err(|_| ProfileCheckpointError::Authentication)?,
        );
        let mut key = [0u8; 32];
        key.copy_from_slice(&material);
        Ok(Zeroizing::new(key))
    }

    fn artifact_directory(
        artifact_path: &Path,
        source: &str,
    ) -> Result<(crate::profile_directory::ProfileDirectory, PathBuf), ProfileCheckpointError> {
        let parent = artifact_path
            .parent()
            .ok_or(ProfileCheckpointError::Unsupported)?;
        if std::fs::canonicalize(parent)?.starts_with(source) {
            return Err(ProfileCheckpointError::Unsupported);
        }
        let name = artifact_path
            .file_name()
            .ok_or(ProfileCheckpointError::Unsupported)?;
        let directory = crate::profile_directory::ProfileDirectory::open(parent)?;
        directory.require_private()?;
        Ok((directory, PathBuf::from(name)))
    }

    /// Capture only after every runtime/app writer has acknowledged shutdown.
    /// Acquiring the original profile lease refuses concurrent live owners.
    pub fn capture(
        selected: SelectedFilesystemCheckpointSource,
        original_profile: &Path,
        artifact_path: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
    ) -> Result<Self, ProfileCheckpointError> {
        Self::capture_internal(
            selected,
            original_profile,
            artifact_path,
            revision,
            participant,
            None,
        )
    }

    /// Bind one bounded simulator-only auxiliary payload into the same
    /// selected-provider artifact as the quiescent profile. The opaque selected
    /// proof is consumed once; context bytes are authenticated metadata, not
    /// authorization to capture another profile or live owner.
    pub fn capture_with_auxiliary(
        selected: SelectedFilesystemCheckpointSource,
        original_profile: &Path,
        artifact_path: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
        context: AuxiliaryCheckpointContext,
        auxiliary: &[u8],
    ) -> Result<Self, ProfileCheckpointError> {
        if auxiliary.is_empty() || auxiliary.len() > MAX_AUXILIARY_BYTES {
            return Err(ProfileCheckpointError::Inventory);
        }
        Self::capture_internal(
            selected,
            original_profile,
            artifact_path,
            revision,
            participant,
            Some((context, auxiliary)),
        )
    }

    fn capture_internal(
        selected: SelectedFilesystemCheckpointSource,
        original_profile: &Path,
        artifact_path: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
        auxiliary: Option<(AuxiliaryCheckpointContext, &[u8])>,
    ) -> Result<Self, ProfileCheckpointError> {
        use aura_core::effects::profile_storage::ProfileStorageLease;
        let owner = Self::owner(original_profile)?;
        let directory = &owner.directory;
        let wrapping_key = Self::require_filesystem_provider(directory)?;
        let source = owner.profile_identity().to_owned();
        if source != selected.source {
            return Err(ProfileCheckpointError::ForeignSource);
        }
        let mut paths = directory.files()?;
        paths.sort();
        if paths.len() > MAX_FILES || paths.iter().any(|path| path == Path::new(MARKER)) {
            return Err(ProfileCheckpointError::Inventory);
        }
        let mut records = Vec::with_capacity(paths.len());
        let mut total = 0usize;
        for path in paths {
            if path == Path::new(".aura-profile-owner.lock") {
                continue;
            }
            let private = path.starts_with(crate::profile_storage::SECURE_PROVIDER_DIRECTORY);
            let bytes = directory
                .read_bounded(&path, private, MAX_FILE_BYTES)?
                .ok_or(ProfileCheckpointError::Inventory)?;
            total = total
                .checked_add(bytes.len())
                .ok_or(ProfileCheckpointError::Inventory)?;
            if total > MAX_ARCHIVE_BYTES {
                return Err(ProfileCheckpointError::Inventory);
            }
            records.push(Record { path, bytes });
        }
        let plaintext = Zeroizing::new(
            match auxiliary {
                Some((_, payload)) => postcard::to_allocvec(&ArchiveV2 {
                    schema: 2,
                    records,
                    auxiliary: payload.to_vec(),
                }),
                None => postcard::to_allocvec(&Archive { schema: 1, records }),
            }
            .map_err(|_| ProfileCheckpointError::Inventory)?,
        );
        if plaintext.len() > MAX_ARCHIVE_BYTES {
            return Err(ProfileCheckpointError::Inventory);
        }
        let mut salt = [0u8; 32];
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut salt)
            .map_err(ProfileCheckpointError::Entropy)?;
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(ProfileCheckpointError::Entropy)?;
        let binding = match auxiliary {
            Some((context, _)) => {
                Self::auxiliary_binding(&source, &revision, &participant, &context)
            }
            None => Self::binding(&source, &revision, &participant),
        };
        let key = Self::derived_key(&wrapping_key, &salt, &binding)?;
        let cipher = ChaCha20Poly1305::new((&*key).into());
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &binding,
                },
            )
            .map_err(|_| ProfileCheckpointError::Authentication)?;
        let artifact = match auxiliary {
            Some((context, _)) => postcard::to_allocvec(&ProtectedArtifactV2 {
                schema: 2,
                source: source.clone(),
                revision,
                participant,
                context,
                salt,
                nonce,
                ciphertext,
            }),
            None => postcard::to_allocvec(&ProtectedArtifact {
                schema: 1,
                source: source.clone(),
                revision,
                participant,
                salt,
                nonce,
                ciphertext,
            }),
        }
        .map_err(|_| ProfileCheckpointError::Inventory)?;
        if artifact.len() > MAX_ARCHIVE_BYTES + 4096 {
            return Err(ProfileCheckpointError::Inventory);
        }
        let (artifact_directory, artifact_name) = Self::artifact_directory(artifact_path, &source)?;
        let prepared = artifact_directory.prepare_private(&artifact_name, &artifact)?;
        if !prepared.publish(true)? {
            return Err(ProfileCheckpointError::Inventory);
        }
        prepared.acknowledge()?;
        Ok(Self {
            artifact_path: artifact_path.to_path_buf(),
            source,
            revision,
            participant,
            auxiliary_context: auxiliary.map(|(context, _)| context),
        })
    }

    /// Restore the same logical profile only after the preceding owner has
    /// exited. Decryption and complete inventory validation precede mutation.
    pub async fn restore(
        &self,
        original_profile: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
    ) -> Result<(), ProfileCheckpointError> {
        self.restore_internal(original_profile, revision, participant, None)
            .await
            .map(|_| ())
    }

    /// Restore a V2 artifact and return only its authenticated auxiliary bytes.
    /// V1 artifacts, missing payloads and context substitutions are refused
    /// before any profile mutation.
    pub async fn restore_with_auxiliary(
        &self,
        original_profile: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
        context: AuxiliaryCheckpointContext,
    ) -> Result<Zeroizing<Vec<u8>>, ProfileCheckpointError> {
        self.restore_internal(original_profile, revision, participant, Some(context))
            .await?
            .ok_or(ProfileCheckpointError::Authentication)
    }

    async fn restore_internal(
        &self,
        original_profile: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
        expected_context: Option<AuxiliaryCheckpointContext>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, ProfileCheckpointError> {
        use aura_core::effects::profile_storage::ProfileStorageLease;
        use aura_core::effects::secure::{SecureStorageEffects, SecureStorageLocation};
        use aura_core::effects::StorageCoreEffects;
        let owner = std::sync::Arc::new(Self::owner(original_profile)?);
        let directory = &owner.directory;
        let wrapping_key = Self::require_filesystem_provider(directory)?;
        if owner.profile_identity() != self.source
            || revision != self.revision
            || participant != self.participant
        {
            return Err(ProfileCheckpointError::ForeignSource);
        }
        let (artifact_directory, artifact_name) =
            Self::artifact_directory(&self.artifact_path, &self.source)?;
        let encoded = artifact_directory
            .read_bounded(&artifact_name, true, MAX_ARCHIVE_BYTES + 4096)?
            .ok_or(ProfileCheckpointError::Authentication)?;
        if expected_context != self.auxiliary_context {
            return Err(ProfileCheckpointError::ForeignSource);
        }
        let (salt, nonce, ciphertext, binding) = match expected_context {
            Some(context) => {
                let artifact: ProtectedArtifactV2 = postcard::from_bytes(&encoded)
                    .map_err(|_| ProfileCheckpointError::Authentication)?;
                if artifact.schema != 2
                    || artifact.source != self.source
                    || artifact.revision != self.revision
                    || artifact.participant != self.participant
                    || artifact.context != context
                {
                    return Err(ProfileCheckpointError::ForeignSource);
                }
                (
                    artifact.salt,
                    artifact.nonce,
                    artifact.ciphertext,
                    Self::auxiliary_binding(
                        &self.source,
                        &self.revision,
                        &self.participant,
                        &context,
                    ),
                )
            }
            None => {
                let artifact: ProtectedArtifact = postcard::from_bytes(&encoded)
                    .map_err(|_| ProfileCheckpointError::Authentication)?;
                if artifact.schema != 1
                    || artifact.source != self.source
                    || artifact.revision != self.revision
                    || artifact.participant != self.participant
                {
                    return Err(ProfileCheckpointError::ForeignSource);
                }
                (
                    artifact.salt,
                    artifact.nonce,
                    artifact.ciphertext,
                    Self::binding(&self.source, &self.revision, &self.participant),
                )
            }
        };
        let key = Self::derived_key(&wrapping_key, &salt, &binding)?;
        let cipher = ChaCha20Poly1305::new((&*key).into());
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &binding,
                    },
                )
                .map_err(|_| ProfileCheckpointError::Authentication)?,
        );
        let (archive, auxiliary) = match expected_context {
            Some(_) => {
                let mut archive: ArchiveV2 = postcard::from_bytes(&plaintext)
                    .map_err(|_| ProfileCheckpointError::Inventory)?;
                if archive.schema != 2
                    || archive.auxiliary.is_empty()
                    || archive.auxiliary.len() > MAX_AUXILIARY_BYTES
                {
                    return Err(ProfileCheckpointError::Inventory);
                }
                (
                    Archive {
                        schema: 1,
                        records: std::mem::take(&mut archive.records),
                    },
                    Some(Zeroizing::new(std::mem::take(&mut archive.auxiliary))),
                )
            }
            None => (
                postcard::from_bytes(&plaintext).map_err(|_| ProfileCheckpointError::Inventory)?,
                None,
            ),
        };
        if archive.schema != 1 || archive.records.len() > MAX_FILES {
            return Err(ProfileCheckpointError::Inventory);
        }
        let mut prior = None;
        let mut total = 0usize;
        for record in &archive.records {
            if record.path.is_absolute()
                || record.path == Path::new(".aura-profile-owner.lock")
                || record.path == Path::new(MARKER)
                || record
                    .path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
                || prior
                    .as_ref()
                    .is_some_and(|previous: &PathBuf| previous >= &record.path)
                || record.bytes.len() > MAX_FILE_BYTES
            {
                return Err(ProfileCheckpointError::Inventory);
            }
            total = total
                .checked_add(record.bytes.len())
                .ok_or(ProfileCheckpointError::Inventory)?;
            if total > MAX_ARCHIVE_BYTES {
                return Err(ProfileCheckpointError::Inventory);
            }
            prior = Some(record.path.clone());
        }
        let captured_paths = archive
            .records
            .iter()
            .map(|record| record.path.clone())
            .collect::<std::collections::HashSet<_>>();
        let mut new_branch_records = Vec::new();
        let mut lifetime_branch_records = Vec::new();
        let mut ordinary_validator = None;
        for path in directory.files()? {
            if path == Path::new(".aura-profile-owner.lock")
                || path == Path::new(MARKER)
                || captured_paths.contains(&path)
            {
                continue;
            }
            let private = path.starts_with(crate::profile_storage::SECURE_PROVIDER_DIRECTORY);
            let bytes = directory
                .read_bounded(&path, private, MAX_FILE_BYTES)?
                .ok_or(ProfileCheckpointError::Inventory)?;
            if private {
                let relative = path
                    .strip_prefix(crate::profile_storage::SECURE_PROVIDER_DIRECTORY)
                    .map_err(|_| ProfileCheckpointError::ForeignBranchPath)?;
                if relative.starts_with(".allocation-lifetimes-v1") {
                    lifetime_branch_records.push(path.clone());
                } else if crate::secure::FilesystemFallbackSecureStorageHandler::checkpoint_branch_location(relative).is_none() {
                    return Err(ProfileCheckpointError::NoncanonicalSecureBranchPath);
                } else {
                    let selected =
                        crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                            owner.clone(),
                        )
                        .map_err(|_| ProfileCheckpointError::Unsupported)?;
                    if !selected.authenticate_checkpoint_branch_record(relative, &bytes) {
                        return Err(ProfileCheckpointError::NoncanonicalSecureBranchRecord);
                    }
                }
            } else {
                let key = crate::storage::FilesystemStorageHandler::checkpoint_semantic_key(&path)
                    .ok_or(ProfileCheckpointError::ForeignBranchPath)?;
                if ordinary_validator.is_none() {
                    let selected = std::sync::Arc::new(
                        crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                            owner.clone(),
                        )
                        .map_err(|_| ProfileCheckpointError::Unsupported)?,
                    );
                    let master = SecureStorageLocation::new("aura-encryption", "master-key");
                    if !selected
                        .secure_exists(&master)
                        .await
                        .map_err(|_| ProfileCheckpointError::Authentication)?
                    {
                        return Err(ProfileCheckpointError::Authentication);
                    }
                    let storage = crate::storage::FilesystemStorageHandler::new(
                        original_profile.to_path_buf(),
                    )
                    .retain_profile_owner(owner.clone())
                    .map_err(|_| ProfileCheckpointError::Unsupported)?;
                    ordinary_validator = Some(crate::encrypted_storage::EncryptedStorage::new(
                        storage,
                        std::sync::Arc::new(crate::crypto::RealCryptoHandler::new()),
                        selected,
                        crate::encrypted_storage::EncryptedStorageConfig::production_required(),
                    ));
                }
                if ordinary_validator
                    .as_ref()
                    .ok_or(ProfileCheckpointError::Unsupported)?
                    .retrieve(&key)
                    .await
                    .map_err(|_| ProfileCheckpointError::UnauthenticatedOrdinaryBranchRecord)?
                    .is_none()
                {
                    return Err(ProfileCheckpointError::UnauthenticatedOrdinaryBranchRecord);
                }
            }
            total = total
                .checked_add(bytes.len())
                .ok_or(ProfileCheckpointError::Inventory)?;
            if total > MAX_ARCHIVE_BYTES
                || new_branch_records.len() + lifetime_branch_records.len() >= MAX_FILES
            {
                return Err(ProfileCheckpointError::Inventory);
            }
            if lifetime_branch_records.last() != Some(&path) {
                new_branch_records.push(path);
            }
        }
        let lifetime_plan = if lifetime_branch_records.is_empty() {
            None
        } else {
            let selected =
                crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                    owner.clone(),
                )
                .map_err(|_| ProfileCheckpointError::Unsupported)?;
            Some(
                selected
                    .prepare_simulator_allocation_branch_reset(
                        &captured_paths,
                        &lifetime_branch_records,
                    )
                    .map_err(|_| ProfileCheckpointError::UnverifiedAllocationBranch)?,
            )
        };
        let marker = directory.prepare_private(Path::new(MARKER), b"incomplete")?;
        marker.publish(true)?;
        marker.acknowledge()?;
        if let Some(plan) = lifetime_plan {
            plan.retire_after_restore_marker()
                .await
                .map_err(|_| ProfileCheckpointError::UnverifiedAllocationBranch)?;
        }
        for path in &new_branch_records {
            directory.remove(path)?;
        }
        for record in &archive.records {
            directory.remove(&record.path)?;
        }
        directory.acknowledge_entries()?;
        for record in &archive.records {
            let private = record
                .path
                .starts_with(crate::profile_storage::SECURE_PROVIDER_DIRECTORY);
            let prepared = if private {
                directory.prepare_private(&record.path, &record.bytes)?
            } else {
                directory.prepare(&record.path, &record.bytes)?
            };
            prepared.publish(true)?;
            prepared.acknowledge()?;
        }
        directory.remove(Path::new(MARKER))?;
        directory.acknowledge_entries()?;
        Ok(auxiliary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn auxiliary_context() -> AuxiliaryCheckpointContext {
        AuxiliaryCheckpointContext {
            kind: *b"broker-state-v1!",
            version: 1,
            owner: [11; 32],
            epoch: 7,
            fixture: [13; 32],
            selected_clock_ms: 5_000,
        }
    }

    fn captured_auxiliary(
        root: &tempfile::TempDir,
        context: AuxiliaryCheckpointContext,
    ) -> (tempfile::TempDir, QuiescentFilesystemProfileCheckpoint) {
        use std::os::unix::fs::PermissionsExt;
        let artifacts = tempfile::tempdir().expect("private auxiliary artifact directory");
        std::fs::set_permissions(artifacts.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private auxiliary directory");
        let checkpoint = QuiescentFilesystemProfileCheckpoint::capture_with_auxiliary(
            selected(root),
            root.path(),
            &artifacts.path().join("checkpoint-v2.bin"),
            [7; 32],
            [9; 32],
            context,
            b"opaque stopped broker state",
        )
        .expect("selected provider V2 capture");
        (artifacts, checkpoint)
    }

    fn selected(root: &tempfile::TempDir) -> SelectedFilesystemCheckpointSource {
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("selected profile owner"),
        );
        let backend =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("actual selected filesystem backend");
        let crate::ProductionSecureStorageHandler::ProfileOwned(owned) = &backend else {
            panic!("actual selected backend retains owner")
        };
        let witness = owned
            .selected_filesystem_checkpoint_source()
            .expect("actual fallback selection");
        drop(backend);
        drop(owner);
        witness
    }

    fn initialized_profile() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().expect("isolated selected profile");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private selected profile");
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("original profile owner"),
        );
        let backend =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("selected filesystem secure provider");
        let file = owner
            .directory
            .prepare(Path::new("record"), b"original")
            .expect("original record stage");
        assert!(file.publish(true).expect("original record publish"));
        file.acknowledge().expect("original record acknowledgment");
        drop(backend);
        drop(owner);
        root
    }

    async fn initialized_lifetime_profile() -> tempfile::TempDir {
        let root = initialized_profile();
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("original lifetime owner"),
        );
        let selected =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("selected filesystem provider");
        let (_selected, mut channel) = selected
            .into_selected_profile_lifetime_channel()
            .expect("original selected lifetime channel");
        assert!(channel
            .recover_owned_inventory()
            .await
            .expect("initial inventory")
            .is_empty());
        drop(channel);
        drop(_selected);
        drop(owner);
        root
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_resets_owner_authenticated_allocation_branch() {
        let root = initialized_lifetime_profile().await;
        let (_artifacts, checkpoint) = captured(&root);
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("branch lifetime owner"),
        );
        let selected =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("same selected provider");
        let (_selected, mut channel) = selected
            .into_selected_profile_lifetime_channel()
            .expect("branch selected lifetime channel");
        assert!(channel
            .recover_owned_inventory()
            .await
            .expect("original inventory")
            .is_empty());
        let allocation = channel
            .allocate(b"branch scope", b"branch signing state")
            .await
            .expect("normal owner birth");
        drop(allocation);
        drop(channel);
        drop(_selected);
        drop(owner);
        checkpoint
            .restore(root.path(), [7; 32], [9; 32])
            .await
            .expect("owner-authenticated branch reset");
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("fresh original owner"),
        );
        let selected =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("same selected provider reopens");
        let (_selected, mut channel) = selected
            .into_selected_profile_lifetime_channel()
            .expect("fresh lifetime channel");
        assert!(channel
            .recover_owned_inventory()
            .await
            .expect("restored original inventory")
            .is_empty());
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_foreign_allocation_branch_before_mutation() {
        let root = initialized_lifetime_profile().await;
        let (_artifacts, checkpoint) = captured(&root);
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("branch owner");
        let path = Path::new("secure_store/.allocation-lifetimes-v1/foreign.lifetime");
        let foreign = owner
            .directory
            .prepare_private(path, b"foreign")
            .expect("foreign record stage");
        assert!(foreign.publish(true).expect("foreign publication"));
        foreign.acknowledge().expect("foreign acknowledgment");
        drop(owner);
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::UnverifiedAllocationBranch)
        ));
        assert!(root.path().join(path).exists(), "refusal precedes mutation");
        FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("failed reset left original profile acquirable");
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_tampered_allocation_branch_before_mutation() {
        let root = initialized_lifetime_profile().await;
        let (_artifacts, checkpoint) = captured(&root);
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("branch owner"),
        );
        let selected =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("same selected provider");
        let (_selected, mut channel) = selected
            .into_selected_profile_lifetime_channel()
            .expect("original channel");
        channel
            .recover_owned_inventory()
            .await
            .expect("original inventory");
        let allocation = channel
            .allocate(b"branch scope", b"branch signing state")
            .await
            .expect("normal owner birth");
        let leaf = format!(
            "{}.lifetime",
            allocation
                .reference()
                .allocation
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let path = Path::new("secure_store/.allocation-lifetimes-v1").join(leaf);
        drop(allocation);
        drop(channel);
        drop(_selected);
        let corrupted = owner
            .directory
            .prepare_private(&path, b"forged protected lifetime")
            .expect("tamper stage");
        corrupted.publish(false).expect("tamper publication");
        corrupted.acknowledge().expect("tamper acknowledgment");
        drop(owner);
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::UnverifiedAllocationBranch)
        ));
        assert!(
            root.path().join(path).exists(),
            "tamper refusal precedes mutation"
        );
        FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("failed reset left original profile acquirable");
    }

    fn captured(
        root: &tempfile::TempDir,
    ) -> (tempfile::TempDir, QuiescentFilesystemProfileCheckpoint) {
        let artifacts = tempfile::tempdir().expect("private artifact directory");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(artifacts.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private checkpoint artifact directory");
        assert_eq!(
            std::fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
            0o700,
            "selected profile root is private"
        );
        assert_eq!(
            std::fs::metadata(root.path().join("secure_store"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "selected secure store is private"
        );
        assert_eq!(
            std::fs::metadata(artifacts.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "artifact directory is private"
        );
        let checkpoint = QuiescentFilesystemProfileCheckpoint::capture(
            selected(root),
            root.path(),
            &artifacts.path().join("checkpoint.bin"),
            [7; 32],
            [9; 32],
        )
        .expect("quiescent capture");
        (artifacts, checkpoint)
    }

    async fn encrypted_profile() -> tempfile::TempDir {
        use aura_core::effects::StorageCoreEffects;
        let root = initialized_profile();
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("original encrypted profile owner"),
        );
        let secure = Arc::new(
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("selected filesystem provider"),
        );
        let storage = crate::storage::FilesystemStorageHandler::new(root.path().to_path_buf())
            .retain_profile_owner(owner.clone())
            .expect("ordinary writer bound to same original profile");
        let encrypted = crate::encrypted_storage::EncryptedStorage::new(
            storage,
            Arc::new(crate::crypto::RealCryptoHandler::new()),
            secure,
            crate::encrypted_storage::EncryptedStorageConfig::production_required(),
        );
        encrypted
            .store("checkpoint_seed", b"original encrypted record".to_vec())
            .await
            .expect("original encrypted master key and record retained before capture");
        drop(encrypted);
        drop(owner);
        root
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_resets_authenticated_ordinary_branch_record() {
        use aura_core::effects::StorageCoreEffects;
        let root = encrypted_profile().await;
        let (_artifacts, checkpoint) = captured(&root);
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("original encrypted branch owner"),
        );
        let secure = Arc::new(
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("selected branch provider"),
        );
        let storage = crate::storage::FilesystemStorageHandler::new(root.path().to_path_buf())
            .retain_profile_owner(owner.clone())
            .expect("selected branch writer");
        let encrypted = crate::encrypted_storage::EncryptedStorage::new(
            storage,
            Arc::new(crate::crypto::RealCryptoHandler::new()),
            secure,
            crate::encrypted_storage::EncryptedStorageConfig::production_required(),
        );
        encrypted
            .store("checkpoint_branch/new", b"owned branch value".to_vec())
            .await
            .expect("effect-produced encrypted branch record");
        drop(encrypted);
        drop(owner);
        checkpoint
            .restore(root.path(), [7; 32], [9; 32])
            .await
            .expect("authenticated ordinary branch record reset");
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("fresh original profile owner"),
        );
        let secure = Arc::new(
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("same selected provider"),
        );
        let storage = crate::storage::FilesystemStorageHandler::new(root.path().to_path_buf())
            .retain_profile_owner(owner)
            .expect("same original writer");
        let encrypted = crate::encrypted_storage::EncryptedStorage::new(
            storage,
            Arc::new(crate::crypto::RealCryptoHandler::new()),
            secure,
            crate::encrypted_storage::EncryptedStorageConfig::production_required(),
        );
        assert_eq!(
            encrypted
                .retrieve("checkpoint_seed")
                .await
                .expect("original authenticated record"),
            Some(b"original encrypted record".to_vec())
        );
        assert_eq!(
            encrypted
                .retrieve("checkpoint_branch/new")
                .await
                .expect("removed branch record"),
            None
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_unauthenticated_ordinary_branch_record() {
        let root = encrypted_profile().await;
        let (_artifacts, checkpoint) = captured(&root);
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("original branch owner");
        let fake = owner
            .directory
            .prepare(Path::new("foreign.dat"), b"plaintext unrelated")
            .expect("unrelated canonical-shaped path");
        assert!(fake.publish(true).expect("unrelated publication"));
        fake.acknowledge().expect("unrelated acknowledgment");
        drop(owner);
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::UnauthenticatedOrdinaryBranchRecord)
        ));
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("refusal did not mutate original profile");
        assert_eq!(
            owner
                .directory
                .read(Path::new("foreign.dat"), false)
                .unwrap(),
            Some(b"plaintext unrelated".to_vec())
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_round_trips_owner_bound_records() {
        let root = initialized_profile();
        let (_artifacts, checkpoint) = captured(&root);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&checkpoint.artifact_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "protected artifact is owner-only"
        );
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("branch owner");
        let update = owner
            .directory
            .prepare(Path::new("record"), b"branch")
            .expect("branch stage");
        assert!(update.publish(false).expect("branch publish"));
        update.acknowledge().expect("branch acknowledgment");
        drop(owner);
        checkpoint
            .restore(root.path(), [7; 32], [9; 32])
            .await
            .expect("same source restore");
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("fresh reopened owner");
        assert_eq!(
            owner
                .directory
                .read(Path::new("record"), false)
                .unwrap()
                .as_deref(),
            Some(b"original".as_slice())
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_resets_only_canonical_branch_records() {
        use aura_core::effects::secure::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let root = initialized_profile();
        let (_artifacts, checkpoint) = captured(&root);
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("original branch owner"),
        );
        let backend =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner.clone(),
            )
            .expect("actual selected filesystem provider");
        let location = SecureStorageLocation::new("checkpoint_branch", "new_record");
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        backend
            .secure_store_immutable(&location, b"normal provider branch write", &caps)
            .await
            .expect("new protected branch record");
        drop(backend);
        drop(owner);
        checkpoint
            .restore(root.path(), [7; 32], [9; 32])
            .await
            .expect("authenticated owner resets canonical branch addition");
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("fresh original owner after restore"),
        );
        let backend =
            crate::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)
                .expect("same selected provider reopens");
        assert!(!backend
            .secure_exists(&location)
            .await
            .expect("typed protected lookup"));
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_unrelated_branch_file_before_mutation() {
        let root = initialized_profile();
        let (_artifacts, checkpoint) = captured(&root);
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("original branch owner");
        let unrelated = owner
            .directory
            .prepare(Path::new("unrelated"), b"foreign profile entry")
            .expect("unrelated branch stage");
        assert!(unrelated.publish(true).expect("unrelated branch publish"));
        unrelated
            .acknowledge()
            .expect("unrelated branch acknowledgment");
        drop(owner);
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::ForeignBranchPath)
        ));
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("refusal left original profile openable");
        assert_eq!(
            owner.directory.read(Path::new("unrelated"), false).unwrap(),
            Some(b"foreign profile entry".to_vec())
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_active_and_foreign_owners() {
        let root = initialized_profile();
        let selection = selected(&root);
        let held = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("live owner");
        let artifacts = tempfile::tempdir().expect("private artifact directory");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(artifacts.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private checkpoint artifact directory");
        assert!(matches!(
            QuiescentFilesystemProfileCheckpoint::capture(
                selection,
                root.path(),
                &artifacts.path().join("checkpoint.bin"),
                [7; 32],
                [9; 32]
            ),
            Err(ProfileCheckpointError::Busy)
        ));
        drop(held);
        let (_artifacts, checkpoint) = captured(&root);
        let held = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("live restore owner");
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::Busy)
        ));
        drop(held);
        let foreign = initialized_profile();
        assert!(matches!(
            checkpoint.restore(foreign.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        assert!(matches!(
            checkpoint.restore(root.path(), [8; 32], [9; 32]).await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [8; 32]).await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_rejects_tamper_before_mutation() {
        let root = initialized_profile();
        let (_artifacts, checkpoint) = captured(&root);
        let mut bytes = std::fs::read(&checkpoint.artifact_path).expect("protected artifact");
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&checkpoint.artifact_path, bytes).expect("tampered artifact");
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::Authentication)
        ));
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("original profile still opens");
        assert_eq!(
            owner
                .directory
                .read(Path::new("record"), false)
                .unwrap()
                .as_deref(),
            Some(b"original".as_slice())
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_interrupted_restore_owner() {
        let root = initialized_profile();
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("original owner");
        let marker = owner
            .directory
            .prepare_private(Path::new(MARKER), b"incomplete")
            .expect("durable restore marker");
        assert!(marker.publish(true).expect("marker publication"));
        marker.acknowledge().expect("marker acknowledgment");
        let partial = owner
            .directory
            .prepare(Path::new("record"), b"partial")
            .expect("partial restored record");
        assert!(partial.publish(false).expect("partial publication"));
        partial.acknowledge().expect("partial acknowledgment");
        drop(owner);
        assert!(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .is_err(),
            "ordinary owner must refuse incomplete restore"
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_foreign_branch_records() {
        let root = initialized_profile();
        let (_artifacts, checkpoint) = captured(&root);
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("branch owner");
        let foreign = owner
            .directory
            .prepare(Path::new("foreign"), b"retain")
            .expect("foreign branch file");
        assert!(foreign.publish(true).expect("foreign file publication"));
        foreign.acknowledge().expect("foreign file acknowledgment");
        drop(owner);
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::ForeignBranchPath)
        ));
        assert_eq!(
            std::fs::read(root.path().join("foreign")).unwrap(),
            b"retain"
        );
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_malformed_secure_branch_record() {
        let root = initialized_profile();
        let (_artifacts, checkpoint) = captured(&root);
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("branch owner");
        let path = Path::new("secure_store/checkpoint_branch/malformed");
        let fake = owner
            .directory
            .prepare_private(path, b"AURA-FS-FALLBACK-SECURE-V3not-authenticated")
            .expect("foreign secure branch file");
        assert!(fake.publish(true).expect("foreign secure publication"));
        fake.acknowledge().expect("foreign secure acknowledgment");
        drop(owner);
        assert!(matches!(
            checkpoint.restore(root.path(), [7; 32], [9; 32]).await,
            Err(ProfileCheckpointError::NoncanonicalSecureBranchRecord)
        ));
        assert!(root.path().join(path).exists(), "refusal precedes mutation");
    }

    #[tokio::test]
    async fn quiescent_filesystem_checkpoint_refuses_stale_key_under_platform_selection() {
        let root = initialized_profile();
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(root.path().to_path_buf())
                .acquire_owned_native()
                .expect("original selected profile owner"),
        );
        let selected =
            crate::ProductionSecureStorageHandler::for_production(root.path().to_path_buf())
                .retain_profile_owner(owner)
                .expect("actual selected platform backend");
        let crate::ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            panic!("platform backend retains original profile owner")
        };
        assert!(
            owned.selected_filesystem_checkpoint_source().is_none(),
            "stale filesystem key cannot authorize a platform-selected checkpoint"
        );
    }

    #[tokio::test]
    async fn auxiliary_v2_round_trips_only_under_selected_provider() {
        use std::os::unix::fs::PermissionsExt;
        let root = initialized_profile();
        let context = auxiliary_context();
        let (_artifacts, checkpoint) = captured_auxiliary(&root, context);
        let encoded = std::fs::read(&checkpoint.artifact_path).expect("private V2 artifact");
        assert!(!encoded
            .windows(b"opaque stopped broker state".len())
            .any(|window| window == b"opaque stopped broker state"));
        assert_eq!(
            std::fs::metadata(&checkpoint.artifact_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let restored = checkpoint
            .restore_with_auxiliary(root.path(), [7; 32], [9; 32], context)
            .await
            .expect("same selected owner and context");
        assert_eq!(restored.as_slice(), b"opaque stopped broker state");
    }

    #[tokio::test]
    async fn auxiliary_v2_refuses_foreign_context_and_source_before_mutation() {
        let root = initialized_profile();
        let context = auxiliary_context();
        let (_artifacts, checkpoint) = captured_auxiliary(&root, context);
        for foreign in [
            AuxiliaryCheckpointContext {
                kind: [0; 16],
                ..context
            },
            AuxiliaryCheckpointContext {
                version: 2,
                ..context
            },
            AuxiliaryCheckpointContext {
                owner: [0; 32],
                ..context
            },
            AuxiliaryCheckpointContext {
                epoch: 8,
                ..context
            },
            AuxiliaryCheckpointContext {
                fixture: [0; 32],
                ..context
            },
            AuxiliaryCheckpointContext {
                selected_clock_ms: 5_001,
                ..context
            },
        ] {
            assert!(matches!(
                checkpoint
                    .restore_with_auxiliary(root.path(), [7; 32], [9; 32], foreign)
                    .await,
                Err(ProfileCheckpointError::ForeignSource)
            ));
        }
        assert!(matches!(
            checkpoint
                .restore_with_auxiliary(root.path(), [8; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        assert!(matches!(
            checkpoint
                .restore_with_auxiliary(root.path(), [7; 32], [8; 32], context)
                .await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        assert_eq!(
            std::fs::read(root.path().join("record")).unwrap(),
            b"original"
        );
    }

    #[tokio::test]
    async fn auxiliary_v2_refuses_changed_or_missing_artifact() {
        let root = initialized_profile();
        let context = auxiliary_context();
        let (_artifacts, checkpoint) = captured_auxiliary(&root, context);
        let original = std::fs::read(&checkpoint.artifact_path).unwrap();
        let mut changed = original.clone();
        *changed.last_mut().expect("ciphertext byte") ^= 1;
        std::fs::write(&checkpoint.artifact_path, changed).unwrap();
        assert!(matches!(
            checkpoint
                .restore_with_auxiliary(root.path(), [7; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::Authentication)
        ));
        std::fs::remove_file(&checkpoint.artifact_path).unwrap();
        assert!(matches!(
            checkpoint
                .restore_with_auxiliary(root.path(), [7; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::Authentication)
        ));
        assert_eq!(
            std::fs::read(root.path().join("record")).unwrap(),
            b"original"
        );
    }

    #[tokio::test]
    async fn auxiliary_v2_refuses_v1_downgrade_and_missing_payload() {
        let root = initialized_profile();
        let (_old_artifacts, old) = captured(&root);
        let context = auxiliary_context();
        let artifacts = tempfile::tempdir().unwrap();
        assert!(matches!(
            QuiescentFilesystemProfileCheckpoint::capture_with_auxiliary(
                selected(&root),
                root.path(),
                &artifacts.path().join("empty.bin"),
                [7; 32],
                [9; 32],
                context,
                b"",
            ),
            Err(ProfileCheckpointError::Inventory)
        ));
        assert!(matches!(
            QuiescentFilesystemProfileCheckpoint::capture_with_auxiliary(
                selected(&root),
                root.path(),
                &artifacts.path().join("oversize.bin"),
                [7; 32],
                [9; 32],
                context,
                &vec![0; MAX_AUXILIARY_BYTES + 1],
            ),
            Err(ProfileCheckpointError::Inventory)
        ));
        assert!(matches!(
            old.restore_with_auxiliary(root.path(), [7; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        let (_artifacts, checkpoint) = captured_auxiliary(&root, context);
        let old_bytes = std::fs::read(&old.artifact_path).unwrap();
        std::fs::write(&checkpoint.artifact_path, old_bytes).unwrap();
        assert!(matches!(
            checkpoint
                .restore_with_auxiliary(root.path(), [7; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::Authentication)
        ));
        assert_eq!(
            std::fs::read(root.path().join("record")).unwrap(),
            b"original"
        );
    }

    #[tokio::test]
    async fn auxiliary_v2_refuses_stale_epoch_and_foreign_owner() {
        let root = initialized_profile();
        let context = auxiliary_context();
        let (_old_artifacts, old) = captured_auxiliary(&root, context);
        let newer = AuxiliaryCheckpointContext {
            epoch: context.epoch + 1,
            ..context
        };
        let (_artifacts, checkpoint) = captured_auxiliary(&root, newer);
        std::fs::write(
            &checkpoint.artifact_path,
            std::fs::read(&old.artifact_path).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            checkpoint
                .restore_with_auxiliary(root.path(), [7; 32], [9; 32], newer)
                .await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        let foreign = initialized_profile();
        assert!(matches!(
            old.restore_with_auxiliary(foreign.path(), [7; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::ForeignSource)
        ));
        let owner = FilesystemProfileStorageHandler::new(root.path().to_path_buf())
            .acquire_owned_native()
            .expect("original active owner");
        assert!(matches!(
            old.restore_with_auxiliary(root.path(), [7; 32], [9; 32], context)
                .await,
            Err(ProfileCheckpointError::Busy)
        ));
        drop(owner);
    }
}
