//! Private selected-profile initialization and original birth inventory.
pub(super) mod cutover;
mod legacy_migration;
use super::*;
#[derive(Debug, thiserror::Error)]
pub(super) enum AllocationLifetimeRecoveryError {
    #[error("selected profile is missing original allocation lifetime birth evidence")]
    OriginalBirth,
    #[error("selected profile is missing original allocation lifetime directory marker")]
    OriginalMarker,
    #[error("once-live allocation lifetime checkpoint is missing")]
    LiveCheckpoint,
    #[error("once-live allocation readiness seal is missing")]
    ReadinessSeal,
    #[error("acknowledged original allocation record is missing")]
    LiveAllocation,
}
fn recovery_error(reason: AllocationLifetimeRecoveryError) -> AuraError {
    AuraError::Storage {
        message: "required original allocation lifetime recovery".into(),
        source: Some(Arc::new(reason)),
    }
}
#[cfg(test)]
static INIT_FAULTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeSet<(String, &'static str)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeSet::new()));
#[cfg(test)]
fn init_fault(owned: &ProfileOwnedSecureStorage, stage: &'static str) -> Result<(), AuraError> {
    use aura_core::effects::profile_storage::ProfileStorageLease;
    init_fault_profile(&owned._owner.profile_identity().to_string(), stage)
}
#[cfg(test)]
fn init_fault_profile(profile: &str, stage: &'static str) -> Result<(), AuraError> {
    if INIT_FAULTS
        .lock()
        .expect("initialization fault registry")
        .remove(&(profile.to_string(), stage))
    {
        return Err(source_error(
            "original initialization ACK",
            std::io::Error::other("injected interrupted provider initialization"),
        ));
    }
    Ok(())
}

const BORN: &str = ".aura-allocation-lifetime-born-v1";
const READY: &str = ".aura-allocation-lifetime-ready-v1";
const HANDED: &str = ".aura-allocation-lifetime-handed-v1";
const INDEX: &str = ".allocation-lifetime-index-v1";
const MARKER: &str = ".root-identity-v1";
const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;
const ROOT_MAGIC: &[u8] = b"AURA-LIFETIME-ROOT-V1\0";
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RootSeal {
    version: u16,
    root: [u8; 32],
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
enum Phase {
    Preparing,
    Ready,
    Handed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Birth {
    allocation: [u8; 32],
    scope_hash: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u16,
    root: [u8; 32],
    phase: Phase,
    births: Vec<Birth>,
    pending: Option<LifetimeRecord>,
}
fn read<T: serde::de::DeserializeOwned>(
    directory: &crate::profile_directory::ProfileDirectory,
    path: &str,
    key: &[u8; 32],
) -> Result<Option<T>, AuraError> {
    let Some(bytes) = directory
        .read_canonical_private_publication(std::path::Path::new(path), MAX_INDEX_BYTES)
        .map_err(|source| source_error("read original lifetime initialization", source))?
    else {
        return Ok(None);
    };
    decode_root(path, key, &bytes).map(Some)
}
fn decode_root<T: serde::de::DeserializeOwned>(
    path: &str,
    key: &[u8; 32],
    bytes: &[u8],
) -> Result<T, AuraError> {
    if !bytes.starts_with(ROOT_MAGIC) || bytes.len() < ROOT_MAGIC.len() + 28 {
        return Err(invalid("invalid original lifetime root record"));
    }
    let plaintext = Zeroizing::new(
        ChaCha20Poly1305::new(key.into())
            .decrypt(
                Nonce::from_slice(&bytes[ROOT_MAGIC.len()..ROOT_MAGIC.len() + 12]),
                Payload {
                    msg: &bytes[ROOT_MAGIC.len() + 12..],
                    aad: [ROOT_MAGIC, path.as_bytes()].concat().as_slice(),
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source("authenticate lifetime root", Arc::new(source))
            })?,
    );
    serde_json::from_slice(&plaintext).map_err(|source| AuraError::Serialization {
        message: "decode original lifetime root".into(),
        source: Some(Arc::new(source)),
    })
}
fn observe_original_prelink<T: serde::de::DeserializeOwned>(
    directory: &crate::profile_directory::ProfileDirectory,
    path: &str,
    key: &[u8; 32],
    stages: &mut Vec<crate::profile_directory::PreparedProfileFile>,
) -> Result<Option<T>, AuraError> {
    let Some((bytes, staged)) = directory
        .read_private_publication(std::path::Path::new(path), MAX_INDEX_BYTES)
        .map_err(|source| source_error("observe exact original staged initialization", source))?
    else {
        return Ok(None);
    };
    let original = decode_root(path, key, &bytes)?;
    if let Some(staged) = staged {
        stages.push(staged);
    }
    Ok(Some(original))
}
#[cfg(test)]
pub(super) fn prelink_process_checkpoint(path: &str) -> Result<(), AuraError> {
    if std::env::var("AURA_LIFETIME_PRELINK_TARGET")
        .ok()
        .as_deref()
        != Some(path)
    {
        return Ok(());
    }
    let marker = std::env::var_os("AURA_LIFETIME_PRELINK_MARKER")
        .ok_or_else(|| invalid("prelink fixture marker absent"))?;
    std::fs::write(marker, path)
        .map_err(|source| source_error("prelink fixture checkpoint", source))?;
    loop {
        std::thread::park();
    }
}

fn finish_staged_publication(
    staged: Option<crate::profile_directory::PreparedProfileFile>,
) -> Result<(), AuraError> {
    if let Some(staged) = staged {
        staged
            .publish_original(MAX_INDEX_BYTES)
            .map_err(|source| source_error("finish original staged publication", source))?;
        staged
            .acknowledge()
            .map_err(|source| source_error("acknowledge original staged publication", source))?;
    }
    Ok(())
}

fn publish<T: Serialize>(
    directory: &crate::profile_directory::ProfileDirectory,
    path: &str,
    key: &[u8; 32],
    record: &T,
    initial: bool,
) -> Result<(), AuraError> {
    let plaintext = encode_protected_json(
        record,
        MAX_INDEX_BYTES - ROOT_MAGIC.len() - 28,
        "encode original lifetime root",
    )?;
    let mut nonce = [0_u8; 12];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|source| AuraError::crypto_with_source("root nonce entropy", Arc::new(source)))?;
    let protected = ChaCha20Poly1305::new(key.into())
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &[ROOT_MAGIC, path.as_bytes()].concat(),
            },
        )
        .map_err(|source| {
            AuraError::crypto_with_source("protect lifetime root", Arc::new(source))
        })?;
    let bytes = [ROOT_MAGIC, &nonce, &protected].concat();
    let prepared = directory
        .prepare_private(std::path::Path::new(path), &bytes)
        .map_err(|source| source_error("prepare original lifetime root", source))?;
    #[cfg(test)]
    prelink_process_checkpoint(path)?;
    #[cfg(test)]
    if path == INDEX && !initial {
        #[derive(Deserialize)]
        struct CheckpointPhase {
            phase: Phase,
        }
        let checkpoint: CheckpointPhase =
            serde_json::from_slice(&plaintext).map_err(|source| AuraError::Serialization {
                message: "decode actual fixture checkpoint phase".into(),
                source: Some(Arc::new(source)),
            })?;
        match checkpoint.phase {
            Phase::Ready => prelink_process_checkpoint("mutable-index-ready")?,
            Phase::Handed => prelink_process_checkpoint("mutable-index-handed")?,
            Phase::Preparing => {}
        }
    }

    if !prepared
        .publish(initial)
        .map_err(|source| source_error("publish original lifetime root", source))?
    {
        return Err(invalid("original lifetime root publication collision"));
    }
    prepared
        .acknowledge()
        .map_err(|source| source_error("acknowledge original lifetime root", source))
}
fn validate(index: &Index, root: [u8; 32]) -> Result<(), AuraError> {
    if index.version != 1 || index.root != root || index.births.len() > MAX_PROFILE_ALLOCATION_COUNT
    {
        return Err(invalid("original lifetime root binding contradiction"));
    }
    for (n, birth) in index.births.iter().enumerate() {
        if index.births[..n]
            .iter()
            .any(|prior| prior.allocation == birth.allocation)
        {
            return Err(invalid("duplicate original lifetime birth"));
        }
    }
    if let Some(pending) = &index.pending {
        FilesystemLifetimeRoot::validate(pending)?;
        if index.phase != Phase::Handed
            || index.births.len() >= MAX_PROFILE_ALLOCATION_COUNT
            || index
                .births
                .iter()
                .any(|birth| birth.allocation == pending.reference.allocation)
            || pending.first_decision.is_some()
            || pending.retired
        {
            return Err(invalid("invalid retained pre-live secret birth"));
        }
    }
    if index.phase != Phase::Handed && (!index.births.is_empty() || index.pending.is_some()) {
        return Err(invalid("pre-live root has allocated secret state"));
    }
    Ok(())
}
fn require_seal(seal: &RootSeal, root: [u8; 32]) -> Result<(), AuraError> {
    if seal.version != 1 || seal.root != root {
        return Err(invalid("original lifetime root seal contradiction"));
    }
    Ok(())
}
fn require_directory(
    parent: &crate::profile_directory::ProfileDirectory,
) -> Result<crate::profile_directory::ProfileDirectory, AuraError> {
    parent
        .child(std::path::Path::new(DIRECTORY), false)
        .map_err(|source| source_error("required once-live allocation directory", source))
}

/// Required acknowledged original root and exact protected owner anchors.
pub(super) struct InitializedLifetimeRoot {
    pub(super) directory: crate::profile_directory::ProfileDirectory,
    pub(super) root_identity: [u8; 32],
    pub(super) migration_birth_digest: [u8; 32],
    pub(super) migration_handoff_digest: [u8; 32],
    pub(super) migration_lifecycle_digest: [u8; 32],
}

fn require_initial_stage_inventory(
    selected: &crate::profile_directory::ProfileDirectory,
) -> Result<(), AuraError> {
    let marker = PathBuf::from(DIRECTORY).join(MARKER);
    let lifecycle = PathBuf::from(".aura-allocation-lifetime-owner-v1").join("lifecycle");
    let birth = PathBuf::from(".aura-allocation-lifetime-owner-v1").join("birth");
    let handed = PathBuf::from(".aura-allocation-lifetime-owner-v1").join("handed");
    let index_journal = cutover::journal_path(std::path::Path::new(INDEX))?;
    let lifecycle_journal = cutover::journal_path(&lifecycle)?;
    selected
        .require_stage_inventory(&[
            std::path::Path::new(BORN),
            std::path::Path::new(READY),
            std::path::Path::new(HANDED),
            std::path::Path::new(INDEX),
            &marker,
            &lifecycle,
            &birth,
            &handed,
            &index_journal,
            &lifecycle_journal,
        ])
        .map_err(|source| {
            source_error(
                "required selected-provider original stage inventory",
                source,
            )
        })
}

#[derive(Debug, thiserror::Error)]
enum OriginalInitializationTransitionError {
    #[error("original initialization transition is not a permitted monotone phase change")]
    Phase,
    #[error("original initialization transition contains allocated or pending secret state")]
    ExposedAllocation,
    #[error("original initialization transition lacks its required original seal")]
    Seal,
}
fn transition_error(reason: OriginalInitializationTransitionError) -> AuraError {
    source_error("required original initialization transition", reason)
}

/// A verified initial phase change under the original actual selected profile.
/// The constructor is private; byte observations and matching names cannot mint it.
pub(crate) struct VerifiedOriginalInitializationSuccessor<'a> {
    owner: &'a ProfileOwnedSecureStorage,
    publication: &'a crate::profile_directory::PreparedProfileFile,
    target: PathBuf,
    original_digest: [u8; 32],
    next_digest: [u8; 32],
}
impl<'a> VerifiedOriginalInitializationSuccessor<'a> {
    pub(crate) fn cutover_journal_path(&self) -> Result<PathBuf, AuraError> {
        cutover::journal_path(&self.target)
    }
    pub(crate) fn require_cutover_journal(&self) -> std::io::Result<cutover::CutoverIdentities> {
        cutover::require_witness_journal(self).map_err(std::io::Error::other)
    }
    pub(crate) fn require_publication(
        &self,
        publication: &crate::profile_directory::PreparedProfileFile,
    ) -> std::io::Result<([u8; 32], [u8; 32])> {
        if !std::ptr::eq(self.publication, publication) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                crate::profile_directory::StagedPublicationRecoveryError::ForeignPublicationOwner,
            ));
        }
        let ProductionSecureStorageHandler::FilesystemFallback(backend) =
            self.owner.backend.as_ref()
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider,
            ));
        };
        let directory = backend.owned_directory().map_err(std::io::Error::other)?;
        publication.require_owner_target(directory, &self.target)?;
        Ok((self.original_digest, self.next_digest))
    }
}
fn selected_initial_provider(
    owner: &ProfileOwnedSecureStorage,
) -> Result<&FilesystemFallbackSecureStorageHandler, AuraError> {
    match owner.backend.as_ref() {
        ProductionSecureStorageHandler::FilesystemFallback(backend) => Ok(backend),
        _ => Err(SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error()),
    }
}
fn require_empty_initial_index(index: &Index, root: [u8; 32]) -> Result<(), AuraError> {
    validate(index, root)?;
    if !index.births.is_empty() || index.pending.is_some() {
        return Err(transition_error(
            OriginalInitializationTransitionError::ExposedAllocation,
        ));
    }
    Ok(())
}
fn require_original_initial_seal(
    directory: &crate::profile_directory::ProfileDirectory,
    path: &str,
    key: &[u8; 32],
    root: [u8; 32],
) -> Result<(), AuraError> {
    let seal: RootSeal = read(directory, path, key)?
        .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Seal))?;
    require_seal(&seal, root)
}
fn require_empty_handed_original_root(
    owner: &ProfileOwnedSecureStorage,
    root: [u8; 32],
) -> Result<(), AuraError> {
    let backend = selected_initial_provider(owner)?;
    let provider = backend.owned_directory()?;
    for path in [BORN, READY, HANDED] {
        require_original_initial_seal(provider, path, &backend.wrapping_key, root)?;
    }
    let index: Index = read(provider, INDEX, &backend.wrapping_key)?
        .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Seal))?;
    require_empty_initial_index(&index, root)?;
    if index.phase != Phase::Handed {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    let directory = require_directory(provider)?;
    require_original_initial_seal(&directory, MARKER, &backend.wrapping_key, root)
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "VerifiedOriginalInitializationSuccessor",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
fn verify_original_index_successor<'a>(
    owner: &'a ProfileOwnedSecureStorage,
    observation: &'a crate::profile_directory::ObservedOriginalSuccessor,
) -> Result<VerifiedOriginalInitializationSuccessor<'a>, AuraError> {
    let backend = selected_initial_provider(owner)?;
    let provider = backend.owned_directory()?;
    observation
        .publication()
        .require_owner_target(provider, std::path::Path::new(INDEX))
        .map_err(|source| source_error("required original successor physical owner", source))?;
    let old: Index = decode_root(INDEX, &backend.wrapping_key, observation.original())?;
    // An already handed-off canonical index cannot admit an initialization
    // successor. Refuse at this native publication boundary before consulting
    // pre-live phase evidence or parsing the conflicting staged payload.
    if old.phase == Phase::Handed {
        return Err(source_error(
            "required once-live original publication conflict",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                crate::profile_directory::StagedPublicationRecoveryError::ConflictingOriginal,
            ),
        ));
    }
    let original = legacy_migration::require_preparing_original_anchor(owner)?;
    let next: Index = decode_root(INDEX, &backend.wrapping_key, observation.next())?;
    require_empty_initial_index(&old, original.root)?;
    require_empty_initial_index(&next, original.root)?;
    require_original_initial_seal(provider, BORN, &backend.wrapping_key, original.root)?;
    let directory = require_directory(provider)?;
    require_original_initial_seal(&directory, MARKER, &backend.wrapping_key, original.root)?;
    match (&old.phase, &next.phase) {
        (Phase::Preparing, Phase::Ready) => {
            if read::<RootSeal>(provider, READY, &backend.wrapping_key)?.is_some()
                || read::<RootSeal>(provider, HANDED, &backend.wrapping_key)?.is_some()
            {
                return Err(transition_error(
                    OriginalInitializationTransitionError::Phase,
                ));
            }
            legacy_migration::require_original_completion(owner, original.root, false)?;
        }
        (Phase::Ready, Phase::Handed) => {
            require_original_initial_seal(provider, READY, &backend.wrapping_key, original.root)?;
            require_original_initial_seal(provider, HANDED, &backend.wrapping_key, original.root)?;
            legacy_migration::require_original_completion(owner, original.root, true)?;
        }
        _ => {
            return Err(transition_error(
                OriginalInitializationTransitionError::Phase,
            ))
        }
    }
    Ok(VerifiedOriginalInitializationSuccessor {
        owner,
        publication: observation.publication(),
        target: PathBuf::from(INDEX),
        original_digest: aura_core::hash::hash(observation.original()),
        next_digest: aura_core::hash::hash(observation.next()),
    })
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
fn recover_initial_index_successor(owner: &ProfileOwnedSecureStorage) -> Result<(), AuraError> {
    let backend = selected_initial_provider(owner)?;
    let provider = backend.owned_directory()?;
    let Some(observation) = provider
        .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)
        .map_err(|source| {
            source_error(
                "observe original initialization checkpoint successor",
                source,
            )
        })?
    else {
        return Ok(());
    };
    let witness = verify_original_index_successor(owner, &observation)?;
    cutover::retain_journal(&witness, &observation)?;
    observation
        .publication()
        .publish_verified_initial_successor(&witness, MAX_INDEX_BYTES)
        .map_err(|source| {
            source_error(
                "acknowledge original initialization checkpoint successor",
                source,
            )
        })?;
    cutover::finish_journal(&witness)
}

fn has_acknowledged_initialization_metadata(
    selected: &crate::profile_directory::ProfileDirectory,
) -> Result<bool, AuraError> {
    for path in [BORN, READY, HANDED, INDEX] {
        if selected
            .read_canonical_private_publication(std::path::Path::new(path), MAX_INDEX_BYTES)
            .map_err(|source| {
                source_error("required acknowledged initialization inventory", source)
            })?
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn initialize(
    owned: &ProfileOwnedSecureStorage,
) -> Result<InitializedLifetimeRoot, AuraError> {
    let ProductionSecureStorageHandler::FilesystemFallback(backend) = owned.backend.as_ref() else {
        return Err(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error());
    };
    let selected = backend
        .owned_directory()
        .map_err(|source| source_error("required lifetime provider", source))?;
    require_initial_stage_inventory(selected)?;
    cutover::validate_archived(owned)?;
    cutover::recover_retained(owned)?;
    recover_initial_index_successor(owned)?;
    legacy_migration::recover_original_handoff_successor(owned)?;
    let key = &backend.wrapping_key;
    let mut stages = Vec::new();
    let born: Option<RootSeal> = observe_original_prelink(selected, BORN, key, &mut stages)?;
    let ready: Option<RootSeal> = observe_original_prelink(selected, READY, key, &mut stages)?;
    let handed: Option<RootSeal> = observe_original_prelink(selected, HANDED, key, &mut stages)?;
    let retained: Option<Index> = observe_original_prelink(selected, INDEX, key, &mut stages)?;
    let original = legacy_migration::original_birth(
        owned,
        has_acknowledged_initialization_metadata(selected)?,
    )?;
    // Observation cannot publish: original protected origin and every retained
    // root/index binding are validated before any staged link is acknowledged.
    for seal in [&born, &ready, &handed].into_iter().flatten() {
        require_seal(seal, original.root)?;
    }
    if let Some(index) = &retained {
        validate(index, original.root)?;
    }
    if handed.is_some() {
        for path in [BORN, READY, INDEX] {
            if selected
                .read_canonical_private_publication(std::path::Path::new(path), MAX_INDEX_BYTES)
                .map_err(|source| source_error("required once-live original publication", source))?
                .is_none()
            {
                return Err(recovery_error(
                    AllocationLifetimeRecoveryError::LiveCheckpoint,
                ));
            }
        }
    }
    if retained
        .as_ref()
        .is_some_and(|index| index.phase == Phase::Handed)
        && selected
            .read_canonical_private_publication(std::path::Path::new(HANDED), MAX_INDEX_BYTES)
            .map_err(|source| source_error("required original handoff target", source))?
            .is_none()
    {
        return Err(recovery_error(
            AllocationLifetimeRecoveryError::ReadinessSeal,
        ));
    }
    for staged in stages {
        finish_staged_publication(Some(staged))?;
    }
    let born = match born {
        Some(born) => {
            require_seal(&born, original.root)?;
            born
        }
        None => {
            if ready.is_some() || handed.is_some() || retained.is_some() {
                return Err(recovery_error(
                    AllocationLifetimeRecoveryError::OriginalBirth,
                ));
            }
            publish(selected, BORN, key, &original, true)?;
            #[cfg(test)]
            init_fault(owned, "birth-seal")?;
            original
        }
    };
    require_seal(&born, born.root)?;
    let mut index = match retained {
        Some(original) => original,
        None => {
            if ready.is_some() || handed.is_some() {
                return Err(recovery_error(
                    AllocationLifetimeRecoveryError::LiveCheckpoint,
                ));
            }
            // Only the retained pre-live birth seal allows completion. No
            // allocated file can be accepted as pre-live initialization state.
            match selected.child(std::path::Path::new(DIRECTORY), false) {
                Ok(directory) => {
                    if !directory
                        .names_bounded(MAX_PROFILE_ALLOCATION_COUNT + 1)
                        .map_err(|source| {
                            source_error("inspect interrupted root initialization", source)
                        })?
                        .is_empty()
                    {
                        return Err(invalid(
                            "missing root checkpoint has prior allocation state",
                        ));
                    }
                }
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(source_error("inspect interrupted root directory", source))
                }
            }
            let original = Index {
                version: 1,
                root: born.root,
                phase: Phase::Preparing,
                births: Vec::new(),
                pending: None,
            };
            publish(selected, INDEX, key, &original, true)?;
            original
        }
    };
    validate(&index, born.root)?;
    let directory = if index.phase == Phase::Preparing {
        if ready.is_some() || handed.is_some() {
            return Err(invalid(
                "ready lifetime seal contradicts pre-live checkpoint",
            ));
        }
        let directory = selected
            .child(std::path::Path::new(DIRECTORY), true)
            .map_err(|source| source_error("complete original pre-live directory", source))?;
        let mut markers = Vec::new();
        let marker: Option<RootSeal> =
            observe_original_prelink(&directory, MARKER, key, &mut markers)?;
        if let Some(marker) = &marker {
            require_seal(marker, born.root)?;
        }
        for stage in markers {
            finish_staged_publication(Some(stage))?;
        }
        match marker {
            Some(original) => require_seal(&original, born.root)?,
            None => publish(&directory, MARKER, key, &born, true)?,
        };
        index.phase = Phase::Ready;
        publish(selected, INDEX, key, &index, false)?;
        #[cfg(test)]
        init_fault(owned, "ready-index")?;
        publish(selected, READY, key, &born, true)?;
        directory
    } else {
        let directory = require_directory(selected)?;
        let marker: RootSeal = read(&directory, MARKER, key)?
            .ok_or_else(|| invalid("once-live root identity marker missing"))?;
        require_seal(&marker, born.root)?;
        match ready {
            Some(original) => require_seal(&original, born.root)?,
            None if index.phase == Phase::Ready
                && handed.is_none()
                && index.births.is_empty()
                && index.pending.is_none() =>
            {
                publish(selected, READY, key, &born, true)?;
            }
            None => {
                return Err(recovery_error(
                    AllocationLifetimeRecoveryError::ReadinessSeal,
                ))
            }
        }
        directory
    };
    let anchors =
        legacy_migration::complete_handoff(owned, born.root, index.phase == Phase::Handed)?;
    match (&index.phase, handed) {
        (Phase::Handed, Some(original)) => require_seal(&original, born.root)?,
        (Phase::Handed, None) => {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::ReadinessSeal,
            ))
        }
        (Phase::Ready, previous) => {
            if !index.births.is_empty() || index.pending.is_some() {
                return Err(invalid(
                    "pre-handoff checkpoint contains exposed birth state",
                ));
            }
            if let Some(original) = previous {
                require_seal(&original, born.root)?;
            } else {
                publish(selected, HANDED, key, &born, true)?;
            }
            index.phase = Phase::Handed;
            publish(selected, INDEX, key, &index, false)?;
        }
        (Phase::Preparing, _) => {
            return Err(invalid("uncompleted original provider initialization"))
        }
    }
    selected
        .acknowledge_entries()
        .map_err(|source| source_error("acknowledge original root seals", source))?;
    directory
        .acknowledge_entries()
        .map_err(|source| source_error("acknowledge original root marker", source))?;
    let lifecycle = legacy_migration::acknowledge_handoff_state(owned, born.root)?;
    selected
        .require_stage_inventory(&[])
        .map_err(|source| source_error("required original handoff stage exhaustion", source))?;
    Ok(InitializedLifetimeRoot {
        directory,
        root_identity: born.root,
        migration_birth_digest: anchors.0,
        migration_handoff_digest: anchors.1,
        migration_lifecycle_digest: lifecycle,
    })
}

fn required_index(root: &FilesystemLifetimeRoot) -> Result<Index, AuraError> {
    legacy_migration::require_original_anchors(root)?;
    let index: Index = read(&root.provider_directory, INDEX, &root.key)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveCheckpoint))?;
    validate(&index, root.root_identity)?;
    let birth: RootSeal = read(&root.provider_directory, BORN, &root.key)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
    require_seal(&birth, root.root_identity)?;
    let marker: RootSeal = read(&root.directory, MARKER, &root.key)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalMarker))?;
    require_seal(&marker, root.root_identity)?;

    if index.phase != Phase::Handed {
        return Err(invalid("original live root became pre-live"));
    }
    let ready: RootSeal = read(&root.provider_directory, READY, &root.key)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::ReadinessSeal))?;
    require_seal(&ready, root.root_identity)?;
    let handed: RootSeal = read(&root.provider_directory, HANDED, &root.key)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::ReadinessSeal))?;
    require_seal(&handed, root.root_identity)?;
    root.provider_directory
        .acknowledge_entries()
        .map_err(|source| source_error("acknowledge required original root checkpoint", source))?;
    Ok(index)
}
pub(super) fn complete_pending_birth(root: &FilesystemLifetimeRoot) -> Result<(), AuraError> {
    let mut index = required_index(root)?;
    let Some(original) = index.pending.as_ref() else {
        return Ok(());
    };
    let path = FilesystemLifetimeRoot::path(&original.reference);
    match root
        .directory
        .read_bounded(&path, true, MAX_RECORD_BYTES)
        .map_err(|source| source_error("read original pre-live birth", source))?
    {
        Some(bytes) => {
            let observed = root.decode(&path, &bytes, Some(&original.reference))?;
            if observed.first_decision.is_some()
                || observed.retired
                || observed.secret != original.secret
            {
                return Err(invalid("original pending birth was changed"));
            }
            root.publish(&observed, false)?;
        }
        None => root.publish(original, true)?,
    }
    index.births.push(Birth {
        allocation: original.reference.allocation,
        scope_hash: aura_core::hash::hash(&original.reference.scope),
    });
    index.pending = None;
    publish(&root.provider_directory, INDEX, &root.key, &index, false)
}
pub(super) fn prepare_birth(
    root: &FilesystemLifetimeRoot,
    record: LifetimeRecord,
) -> Result<(), AuraError> {
    complete_pending_birth(root)?;
    let mut index = required_index(root)?;
    if index.births.len() >= MAX_PROFILE_ALLOCATION_COUNT
        || index
            .births
            .iter()
            .any(|birth| birth.allocation == record.reference.allocation)
    {
        return Err(invalid(
            "original lifetime birth inventory exhausted or duplicate",
        ));
    }
    index.pending = Some(record);
    publish(&root.provider_directory, INDEX, &root.key, &index, false)?;
    #[cfg(test)]
    {
        use aura_core::effects::profile_storage::ProfileStorageLease;
        init_fault_profile(
            &root._profile.profile_identity().to_string(),
            "birth-prepared",
        )?;
    }
    complete_pending_birth(root)
}
pub(super) fn recover_references(
    root: &FilesystemLifetimeRoot,
) -> Result<Vec<SecretAllocationReference>, AuraError> {
    complete_pending_birth(root)?;
    let index = required_index(root)?;
    let names = root
        .directory
        .names_bounded(MAX_PROFILE_ALLOCATION_COUNT + 1)
        .map_err(|source| source_error("bounded original birth inventory", source))?;
    if names.len() != index.births.len() + 1
        || !names
            .iter()
            .any(|name| name == std::ffi::OsStr::new(MARKER))
    {
        return Err(invalid(
            "original birth inventory has missing or unowned records",
        ));
    }
    let mut result = Vec::with_capacity(index.births.len());
    for birth in index.births {
        let path = PathBuf::from(format!("{}.lifetime", hex(&birth.allocation)));
        let bytes = root
            .directory
            .read_bounded(&path, true, MAX_RECORD_BYTES)
            .map_err(|source| source_error("required live original birth", source))?
            .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveAllocation))?;
        let record = root.decode(&path, &bytes, None)?;
        if record.reference.allocation != birth.allocation
            || aura_core::hash::hash(&record.reference.scope) != birth.scope_hash
        {
            return Err(invalid("original immutable birth scope was replaced"));
        }
        result.push(record.reference.clone());
    }
    Ok(result)
}
pub(super) fn require_reference(
    root: &FilesystemLifetimeRoot,
    reference: &SecretAllocationReference,
) -> Result<(), AuraError> {
    let index = required_index(root)?;
    if !index.births.iter().any(|birth| {
        birth.allocation == reference.allocation
            && birth.scope_hash == aura_core::hash::hash(&reference.scope)
    }) {
        return Err(invalid(
            "actual original allocation is absent from live checkpoint",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::profile_storage::ProfileStorageLease;
    fn private_temporary_directory() -> std::io::Result<tempfile::TempDir> {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
    }
    #[test]
    fn original_recovery_conflict_after_observation_retains_stage_and_native_source(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = private_temporary_directory()?;
        let directory = crate::profile_directory::ProfileDirectory::open(temporary.path())?;
        let path = std::path::Path::new("original");
        let original = b"original authenticated ciphertext";
        let prepared = directory.prepare_private(path, original)?;
        let (_, observed) = directory
            .read_private_publication(path, 128)?
            .expect("original stage");
        let competing = directory.prepare_private(path, b"conflicting ciphertext")?;
        assert!(competing.publish(true)?);
        competing.acknowledge()?;
        let error =
            finish_staged_publication(observed).expect_err("conflict cannot acknowledge recovery");
        let AuraError::Storage {
            source: Some(source),
            ..
        } = error
        else {
            panic!("native recovery source required")
        };
        let io = source
            .downcast_ref::<std::io::Error>()
            .expect("actual publication IO");
        assert!(matches!(
            io.get_ref()
                .and_then(|source| source
                    .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>()),
            Some(crate::profile_directory::StagedPublicationRecoveryError::ConflictingOriginal)
        ));
        assert_eq!(
            directory.read_bounded(path, true, 128)?.as_deref(),
            Some(b"conflicting ciphertext".as_slice())
        );
        let stage = directory
            .names()?
            .into_iter()
            .find(|name| name.to_string_lossy().starts_with(".aura-stage-"))
            .expect("original stage retained");
        assert_eq!(
            directory
                .read_bounded(std::path::Path::new(&stage), true, 128)?
                .as_deref(),
            Some(original.as_slice())
        );
        drop(prepared);
        Ok(())
    }

    #[test]
    fn original_recovery_identical_acknowledged_ciphertext_finishes_without_replacement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = private_temporary_directory()?;
        let directory = crate::profile_directory::ProfileDirectory::open(temporary.path())?;
        let path = std::path::Path::new("original");
        let original = b"original authenticated ciphertext";
        let prepared = directory.prepare_private(path, original)?;
        let (_, observed) = directory
            .read_private_publication(path, 128)?
            .expect("original stage");
        let competing = directory.prepare_private(path, original)?;
        assert!(competing.publish(true)?);
        competing.acknowledge()?;
        finish_staged_publication(observed)?;
        assert_eq!(
            directory.read_bounded(path, true, 128)?.as_deref(),
            Some(original.as_slice())
        );
        assert_eq!(directory.names()?.len(), 1);
        drop(prepared);
        Ok(())
    }

    #[test]
    fn original_recovery_stage_substitution_after_observation_cannot_publish(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = private_temporary_directory()?;
        let directory = crate::profile_directory::ProfileDirectory::open(temporary.path())?;
        let path = std::path::Path::new("original");
        let prepared = directory.prepare_private(path, b"original authenticated ciphertext")?;
        let (_, observed) = directory
            .read_private_publication(path, 128)?
            .expect("original stage");
        let stage = directory
            .names()?
            .into_iter()
            .find(|name| name.to_string_lossy().starts_with(".aura-stage-"))
            .expect("original stage retained");
        std::fs::write(temporary.path().join(&stage), b"substituted ciphertext")?;
        assert!(finish_staged_publication(observed).is_err());
        assert!(directory.read_bounded(path, true, 128)?.is_none());
        assert_eq!(
            directory
                .read_bounded(std::path::Path::new(&stage), true, 128)?
                .as_deref(),
            Some(b"substituted ciphertext".as_slice())
        );
        drop(prepared);
        Ok(())
    }

    #[test]
    fn original_recovery_source_path_substitution_between_check_and_link_cannot_acknowledge(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = private_temporary_directory()?;
        let directory = crate::profile_directory::ProfileDirectory::open(temporary.path())?;
        let path = std::path::Path::new("original");
        let original = b"original authenticated ciphertext";
        let prepared = directory.prepare_private(path, original)?;
        let (_, observed) = directory
            .read_private_publication(path, 128)?
            .expect("original stage");
        let stage = directory
            .names()?
            .into_iter()
            .find(|name| name.to_string_lossy().starts_with(".aura-stage-"))
            .expect("original stage retained");
        let evidence = temporary.path().join("retained-source-evidence");
        let error = observed
            .expect("original recovery handle")
            .publish_original_with_interleaving(128, || {
                std::fs::rename(temporary.path().join(&stage), &evidence)?;
                let mut replacement = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(temporary.path().join(&stage))?;
                std::io::Write::write_all(&mut replacement, original)?;
                use std::os::unix::fs::PermissionsExt;
                replacement.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                Ok(())
            })
            .expect_err("a linked foreign inode cannot acknowledge original recovery");
        assert!(matches!(
            error
                .get_ref()
                .and_then(|source| source
                    .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>()),
            Some(crate::profile_directory::StagedPublicationRecoveryError::ConflictingOriginal)
        ));
        assert_eq!(std::fs::read(&evidence)?, original);
        assert!(
            directory.names()?.iter().any(|name| name == &stage),
            "rejected stage remains"
        );
        drop(prepared);
        Ok(())
    }

    #[test]
    fn prelink_process_worker() -> Result<(), Box<dyn std::error::Error>> {
        let Some(path) = std::env::var_os("AURA_LIFETIME_PRELINK_PROFILE") else {
            return Ok(());
        };
        let storage = selected(std::path::Path::new(&path))?;
        initialize(owned(&storage))?;
        Err("child must reach the original pre-link checkpoint".into())
    }
    #[test]
    fn unknown_stage_in_other_namespace_blocks_original_and_live_handoff_without_deletion(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for live in [false, true] {
            let temporary = tempfile::tempdir()?;
            let profile = temporary.path().join("profile");
            let storage = selected(&profile)?;
            let original = if live {
                Some(initialize(owned(&storage))?.root_identity)
            } else {
                None
            };
            let backend = backend(&storage);
            let provider = backend.owned_directory()?;
            let foreign = provider
                .child(std::path::Path::new("other"), true)?
                .child(std::path::Path::new("nested"), true)?;
            let stage =
                foreign.prepare_private(std::path::Path::new("foreign"), b"unowned ciphertext")?;
            let error = initialize(owned(&storage))
                .err()
                .expect("foreign stage cannot be ignored");
            let AuraError::Storage {
                source: Some(source),
                ..
            } = error
            else {
                panic!("native inventory source required")
            };
            let io = source
                .downcast_ref::<std::io::Error>()
                .expect("actual descriptor error");
            assert!(matches!(
                io.get_ref()
                    .and_then(|source| source
                        .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>(
                    )),
                Some(crate::profile_directory::StagedPublicationRecoveryError::UnownedStage)
            ));
            assert_eq!(foreign.names()?.len(), 1, "unknown evidence must remain");
            if let Some(original) = original {
                assert_eq!(root_id(&storage)?, original);
            } else {
                assert!(provider
                    .read_bounded(std::path::Path::new(BORN), true, MAX_INDEX_BYTES)?
                    .is_none());
            }
            drop(stage);
        }
        Ok(())
    }

    #[test]
    fn initial_stage_names_cannot_authorize_payload_or_survive_handoff_exhaustion(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        let storage = selected(&profile)?;
        let backend = backend(&storage);
        let provider = backend.owned_directory()?;
        let staged = provider.prepare_private(
            std::path::Path::new(BORN),
            b"malformed allowed-name ciphertext",
        )?;
        require_initial_stage_inventory(provider)?;
        assert!(
            initialize(owned(&storage)).is_err(),
            "allowed filename is not cryptographic proof"
        );
        assert!(provider
            .read_bounded(std::path::Path::new(BORN), true, MAX_INDEX_BYTES)?
            .is_none());
        assert!(
            provider.require_stage_inventory(&[]).is_err(),
            "handoff requires no remaining stage"
        );
        assert!(provider
            .names()?
            .iter()
            .any(|name| name.as_encoded_bytes().starts_with(b".aura-stage-")));
        drop(staged);
        Ok(())
    }

    #[test]
    fn stage_inventory_streams_max_allocation_layout_and_more_than_4096_ordinary_records(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::OpenOptionsExt;
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        let storage = selected(&profile)?;
        let backend = backend(&storage);
        let provider = backend.owned_directory()?;
        provider.child(std::path::Path::new(DIRECTORY), true)?;
        provider.child(std::path::Path::new("ordinary"), true)?;
        // Metadata inventory is not decoded allocation authority. The genuine
        // allocated-owner regressions independently validate birth evidence.
        for index in 0..aura_core::effects::secret_lifetime::MAX_PROFILE_ALLOCATION_COUNT {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(
                    profile
                        .join("secure_store")
                        .join(DIRECTORY)
                        .join(format!("{index}.lifetime")),
                )?;
        }
        for index in 0..=4096 {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(
                    profile
                        .join("secure_store")
                        .join("ordinary")
                        .join(format!("record-{index}")),
                )?;
        }
        provider.require_stage_inventory(&[])?;
        Ok(())
    }

    #[test]
    fn stage_inventory_retains_ambiguity_and_structural_depth_native_causes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = private_temporary_directory()?;
        let directory = crate::profile_directory::ProfileDirectory::open(temporary.path())?;
        let first = directory.prepare_private(std::path::Path::new(BORN), b"candidate one")?;
        let second = directory.prepare_private(std::path::Path::new(BORN), b"candidate two")?;
        let error = require_initial_stage_inventory(&directory).expect_err("same-target ambiguity");
        let AuraError::Storage {
            source: Some(source),
            ..
        } = error
        else {
            panic!("native inventory source")
        };
        let io = source
            .downcast_ref::<std::io::Error>()
            .expect("actual descriptor inventory");
        assert!(matches!(
            io.get_ref()
                .and_then(|source| source
                    .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>()),
            Some(crate::profile_directory::StagedPublicationRecoveryError::Ambiguous)
        ));
        assert_eq!(directory.names()?.len(), 2);
        drop((first, second));
        let separate = private_temporary_directory()?;
        let root = crate::profile_directory::ProfileDirectory::open(separate.path())?;
        root.child(
            std::path::Path::new("namespace/key/unowned-directory"),
            true,
        )?;
        let error = root
            .require_stage_inventory(&[])
            .expect_err("secure layout excludes third descendant directory");
        assert!(matches!(
            error
                .get_ref()
                .and_then(|source| source
                    .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>()),
            Some(
                crate::profile_directory::StagedPublicationRecoveryError::InventoryLimit {
                    maximum_depth: 2,
                    observed_depth: 3
                }
            )
        ));
        assert!(separate
            .path()
            .join("namespace/key/unowned-directory")
            .is_dir());
        Ok(())
    }

    #[test]
    fn original_publication_reader_reopens_live_profile_with_more_than_4096_namespace_siblings(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::DirBuilderExt;
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        let storage = selected(&profile)?;
        let original = initialize(owned(&storage))?.root_identity;
        let provider = backend(&storage).owned_directory()?;
        for index in 0..=4096 {
            std::fs::DirBuilder::new().mode(0o700).create(
                profile
                    .join("secure_store")
                    .join(format!("namespace-{index}")),
            )?;
        }
        let bytes = provider
            .read_bounded(std::path::Path::new(BORN), true, MAX_INDEX_BYTES)?
            .expect("actual original born seal");
        let staged = provider.prepare_private(std::path::Path::new(BORN), &bytes)?;
        assert!(provider
            .read_private_publication(std::path::Path::new(BORN), MAX_INDEX_BYTES)?
            .expect("actual original stage")
            .1
            .is_some());
        assert_eq!(initialize(owned(&storage))?.root_identity, original);
        assert_eq!(
            provider
                .read_bounded(std::path::Path::new(BORN), true, MAX_INDEX_BYTES)?
                .expect("original seal retained"),
            bytes
        );
        assert!(provider
            .read_private_publication(std::path::Path::new(BORN), MAX_INDEX_BYTES)?
            .expect("actual canonical seal")
            .1
            .is_none());
        drop(staged);
        Ok(())
    }

    #[test]
    fn original_initialization_recovers_linked_stage_after_actual_process_death(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::MetadataExt;
        for target in ["birth", "handed", BORN, INDEX, READY, HANDED, MARKER] {
            let temporary = tempfile::tempdir()?;
            let profile = temporary.path().join("profile");
            killed_original_stage(
                &profile,
                &temporary.path().join("checkpoint"),
                &format!("after-link:{target}"),
            )?;
            let storage = selected(&profile)?;
            let provider = backend(&storage).owned_directory()?;
            let parent = match target {
                "birth" | "handed" => provider.child(
                    std::path::Path::new(".aura-allocation-lifetime-owner-v1"),
                    false,
                )?,
                MARKER => provider.child(std::path::Path::new(DIRECTORY), false)?,
                _ => provider.clone(),
            };
            let (original, stage) = parent
                .read_private_publication(std::path::Path::new(target), MAX_INDEX_BYTES)?
                .expect("retained target/stage pair");
            assert!(stage.is_some());
            let root = profile.join("secure_store");
            let target_path = match target {
                "birth" | "handed" => root.join(".aura-allocation-lifetime-owner-v1").join(target),
                MARKER => root.join(DIRECTORY).join(target),
                _ => root.join(target),
            };
            assert_eq!(std::fs::metadata(&target_path)?.nlink(), 2);
            let initialized = initialize(owned(&storage))?;
            assert_eq!(std::fs::metadata(&target_path)?.nlink(), 1);
            let final_bytes = parent
                .read_bounded(std::path::Path::new(target), true, MAX_INDEX_BYTES)?
                .expect("acknowledged original target");
            if target == INDEX {
                let before: Index = decode_root(INDEX, &backend(&storage).wrapping_key, &original)?;
                let after: Index =
                    decode_root(INDEX, &backend(&storage).wrapping_key, &final_bytes)?;
                assert_eq!(before.root, after.root);
                assert!(before.births.is_empty() && after.births.is_empty());
            } else {
                assert_eq!(original, final_bytes);
            }
            assert_eq!(initialized.root_identity, root_id(&storage)?);
            assert!(parent
                .read_private_publication(std::path::Path::new(target), MAX_INDEX_BYTES)?
                .expect("retained canonical target")
                .1
                .is_none());
        }
        Ok(())
    }

    #[test]
    fn unrelated_ciphertext_alias_cannot_authorize_original_link_recovery(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        let storage = selected(&profile)?;
        initialize(owned(&storage))?;
        let provider = backend(&storage).owned_directory()?;
        let target = profile.join("secure_store").join(BORN);
        let original = std::fs::read(&target)?;
        let stage = provider.prepare_private(std::path::Path::new(BORN), &original)?;
        std::fs::hard_link(
            &target,
            profile.join("secure_store").join("unrelated-alias"),
        )?;
        let error = provider
            .read_private_publication(std::path::Path::new(BORN), MAX_INDEX_BYTES)
            .expect_err("an unrelated identical alias is not an original publication pair");
        assert!(matches!(
            error
                .get_ref()
                .and_then(|source| source
                    .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>()),
            Some(crate::profile_directory::StagedPublicationRecoveryError::ConflictingOriginal)
        ));
        assert!(initialize(owned(&storage)).is_err());
        assert_eq!(std::fs::read(&target)?, original);
        assert!(provider
            .names()?
            .iter()
            .any(|name| name.as_encoded_bytes().starts_with(b".aura-stage-")));
        drop(stage);
        Ok(())
    }

    #[test]
    fn original_mutable_initialization_successors_recover_after_process_death(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for target in [
            "mutable-index-ready",
            "mutable-index-handed",
            "mutable-lifecycle-handed",
        ] {
            let temporary = tempfile::tempdir()?;
            let profile = temporary.path().join("profile");
            killed_original_stage(&profile, &temporary.path().join("checkpoint"), target)?;
            let storage = selected(&profile)?;
            let provider = backend(&storage);
            let original = legacy_migration::require_preparing_original_anchor(owned(&storage))?;
            let directory = provider.owned_directory()?;
            let born =
                directory.read_bounded(std::path::Path::new(BORN), false, MAX_RECORD_BYTES)?;
            let initialized = initialize(owned(&storage))?;
            assert_eq!(initialized.root_identity, original.root);
            assert_eq!(
                born,
                directory.read_bounded(std::path::Path::new(BORN), false, MAX_RECORD_BYTES)?
            );
            let index: Index = read(
                backend(&storage).owned_directory()?,
                INDEX,
                &backend(&storage).wrapping_key,
            )?
            .ok_or("required original index")?;
            validate(&index, initialized.root_identity)?;
            assert!(index.phase == Phase::Handed);
            assert!(index.births.is_empty() && index.pending.is_none());
            require_initial_stage_inventory(&directory)?;
            directory.require_stage_inventory(&[])?;
            drop(initialized);
            drop(storage);
            let reopened = selected(&profile)?;
            assert_eq!(root_id(&reopened)?, original.root);
        }
        Ok(())
    }

    #[test]
    fn original_mutable_successor_requires_independent_seal_and_retains_evidence(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        killed_original_stage(
            &profile,
            &temporary.path().join("checkpoint"),
            "mutable-index-ready",
        )?;
        let storage = selected(&profile)?;
        let directory = backend(&storage).owned_directory()?;
        let before = directory.read_bounded(std::path::Path::new(INDEX), false, MAX_INDEX_BYTES)?;
        std::fs::remove_file(profile.join("secure_store").join(DIRECTORY).join(MARKER))?;
        assert!(initialize(owned(&storage)).is_err());
        assert_eq!(
            before,
            directory.read_bounded(std::path::Path::new(INDEX), false, MAX_INDEX_BYTES)?
        );
        assert!(directory
            .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)?
            .is_some());
        Ok(())
    }

    #[test]
    fn original_mutable_successor_rejects_foreign_physical_profile(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("original");
        killed_original_stage(
            &profile,
            &temporary.path().join("checkpoint"),
            "mutable-index-ready",
        )?;
        let storage = selected(&profile)?;
        let directory = backend(&storage).owned_directory()?;
        let observation = directory
            .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)?
            .ok_or("actual staged successor")?;
        let foreign = selected(&temporary.path().join("foreign"))?;
        let error = verify_original_index_successor(owned(&foreign), &observation)
            .err()
            .ok_or("foreign owner accepted")?;
        assert!(std::error::Error::source(&error).is_some());
        assert!(directory
            .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)?
            .is_some());
        Ok(())
    }

    #[tokio::test]
    async fn original_mutable_successor_cannot_replay_over_exposed_positive_allocation(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        killed_original_stage(
            &profile,
            &temporary.path().join("checkpoint"),
            "mutable-index-ready",
        )?;
        let selected_storage = selected(&profile)?;
        let staged_ciphertext = backend(&selected_storage)
            .owned_directory()?
            .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)?
            .ok_or("actual original successor")?
            .next()
            .to_vec();
        let (ordinary, mut root) = selected_storage.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        let secret = root
            .allocate(b"actual-original", b"original-secret")
            .await?;
        secret.decide_positive(b"original-positive").await?;
        let directory = backend(&ordinary).owned_directory()?;
        let original =
            directory.read_bounded(std::path::Path::new(INDEX), false, MAX_INDEX_BYTES)?;
        let _stage = directory.prepare_private(std::path::Path::new(INDEX), &staged_ciphertext)?;
        assert!(initialize(owned(&ordinary)).is_err());
        assert_eq!(
            original,
            directory.read_bounded(std::path::Path::new(INDEX), false, MAX_INDEX_BYTES)?
        );
        match secret.state().await? {
            aura_core::effects::secret_lifetime::SecretLifetimeState::Positive { decision } => {
                assert_eq!(decision, b"original-positive")
            }
            _ => panic!("original decision must survive stale initial successor"),
        }
        assert_eq!(secret.read_live_secret().await?, b"original-secret");
        assert!(directory
            .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)?
            .is_some());
        Ok(())
    }

    #[test]
    fn original_initial_cutover_preserves_substituted_source_and_target_evidence(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        for substitute_source in [false, true] {
            let temporary = tempfile::tempdir()?;
            let profile = temporary.path().join("profile");
            killed_original_stage(
                &profile,
                &temporary.path().join("checkpoint"),
                "mutable-index-ready",
            )?;
            let storage = selected(&profile)?;
            let directory = backend(&storage).owned_directory()?;
            let observation = directory
                .observe_original_successor(std::path::Path::new(INDEX), MAX_INDEX_BYTES)?
                .ok_or("actual successor")?;
            let witness = verify_original_index_successor(owned(&storage), &observation)?;
            cutover::retain_journal(&witness, &observation)?;
            let original = observation.original().to_vec();
            let next = observation.next().to_vec();
            let stage = observation.publication().initial_stage_name().to_owned();
            let physical = profile.join("secure_store");
            let foreign = b"actual foreign substituted ciphertext";
            let result = observation
                .publication()
                .publish_initial_cutover_with_interleaving(&witness, MAX_INDEX_BYTES, || {
                    let path = physical.join(if substitute_source {
                        stage.as_str()
                    } else {
                        INDEX
                    });
                    std::fs::rename(&path, physical.join("retained-substituted-original"))?;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)?;
                    file.write_all(foreign)?;
                    file.sync_all()
                });
            assert!(
                result.is_err(),
                "raw substitution cannot produce cutover success"
            );
            assert_eq!(
                std::fs::read(physical.join(format!(".aura-initial-before-{stage}")))?,
                original
            );
            assert_eq!(
                std::fs::read(physical.join(format!(".aura-initial-next-{stage}")))?,
                next
            );
            assert_eq!(
                std::fs::read(physical.join(if substitute_source {
                    INDEX
                } else {
                    stage.as_str()
                }))?,
                foreign
            );
            assert!(
                initialize(owned(&storage)).is_err(),
                "conflict remains fail closed on actual reopen path"
            );
        }
        Ok(())
    }

    #[test]
    fn original_initial_cutover_recovers_acknowledged_custody_and_exchange_after_process_death(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for point in [
            "journal-stage",
            "journal",
            "custody-original",
            "custody-next",
            "exchange",
            "ack",
            "archive",
        ] {
            let temporary = tempfile::tempdir()?;
            let profile = temporary.path().join("profile");
            killed_original_stage(
                &profile,
                &temporary.path().join("initial"),
                "mutable-index-ready",
            )?;
            // A separate actual creator reopens original evidence and is killed
            // inside the owned cutover, never a fabricated journal fixture.
            killed_original_stage(
                &profile,
                &temporary.path().join("cutover"),
                &format!("cutover:{point}"),
            )?;
            let storage = selected(&profile)?;
            let original = legacy_migration::require_preparing_original_anchor(owned(&storage))?;
            let initialized = initialize(owned(&storage))?;
            assert_eq!(initialized.root_identity, original.root);
            backend(&storage)
                .owned_directory()?
                .require_stage_inventory(&[])?;
            let index: Index = read(
                backend(&storage).owned_directory()?,
                INDEX,
                &backend(&storage).wrapping_key,
            )?
            .ok_or("required original index")?;
            validate(&index, initialized.root_identity)?;
            assert!(
                index.phase == Phase::Handed && index.births.is_empty() && index.pending.is_none()
            );
            drop(initialized);
            drop(storage);
            assert_eq!(root_id(&selected(&profile)?)?, original.root);
        }
        Ok(())
    }

    #[test]
    fn original_initial_cutover_requires_exact_archived_custody_on_reopen(
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[derive(Clone, Copy)]
        enum Fault {
            Loss,
            ForeignInode,
            CorruptCiphertext,
        }
        for kind in ["before", "next", "displaced"] {
            for fault in [Fault::Loss, Fault::ForeignInode, Fault::CorruptCiphertext] {
                let temporary = tempfile::tempdir()?;
                let profile = temporary.path().join("profile");
                killed_original_stage(
                    &profile,
                    &temporary.path().join("checkpoint"),
                    "mutable-index-ready",
                )?;
                let storage = selected(&profile)?;
                initialize(owned(&storage))?;
                let physical = profile.join("secure_store");
                let birth = std::fs::read(physical.join(BORN))?;
                let head = std::fs::read(physical.join(INDEX))?;
                let prefix = format!(".aura-initial-{kind}-");
                let mut custody = None;
                for entry in std::fs::read_dir(&physical)? {
                    let entry = entry?;
                    if entry
                        .file_name()
                        .as_encoded_bytes()
                        .starts_with(prefix.as_bytes())
                    {
                        custody = Some(entry.path());
                        break;
                    }
                }
                let custody = custody.ok_or("actual archived custody")?;
                let original = std::fs::read(&custody)?;
                let preserved = temporary.path().join("preserved-original-custody");
                if matches!(fault, Fault::CorruptCiphertext) {
                    std::fs::copy(&custody, &preserved)?;
                    let mut corrupted = original.clone();
                    *corrupted.last_mut().ok_or("actual custody ciphertext")? ^= 1;
                    std::fs::write(&custody, &corrupted)?;
                } else {
                    std::fs::rename(&custody, &preserved)?;
                }
                if matches!(fault, Fault::ForeignInode) {
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut foreign = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&custody)?;
                    foreign.write_all(&original)?;
                    foreign.sync_all()?;
                }
                drop(storage);
                let reopened = selected(&profile)?;
                let failure = initialize(owned(&reopened))
                    .err()
                    .ok_or("missing or foreign archived custody admitted")?;
                assert!(matches!(
                    failure,
                    AuraError::Crypto {
                        source: Some(_),
                        ..
                    } | AuraError::Storage {
                        source: Some(_),
                        ..
                    }
                ));
                assert_eq!(std::fs::read(&preserved)?, original);
                if matches!(fault, Fault::ForeignInode) {
                    assert_eq!(std::fs::read(&custody)?, original);
                } else if matches!(fault, Fault::Loss) {
                    assert!(!custody.exists());
                } else {
                    assert_ne!(std::fs::read(&custody)?, original);
                }
                assert_eq!(std::fs::read(physical.join(BORN))?, birth);
                assert_eq!(std::fs::read(physical.join(INDEX))?, head);
            }
        }
        Ok(())
    }

    #[test]
    fn original_initial_cutover_authenticates_history_and_rejects_unknown_transaction_inventory(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        killed_original_stage(
            &profile,
            &temporary.path().join("checkpoint"),
            "mutable-index-ready",
        )?;
        let storage = selected(&profile)?;
        initialize(owned(&storage))?;
        let directory = backend(&storage).owned_directory()?;
        let born = directory.read_bounded(std::path::Path::new(BORN), false, MAX_RECORD_BYTES)?;
        let physical = profile.join("secure_store");
        let mut history = None;
        for entry in std::fs::read_dir(&physical)? {
            let entry = entry?;
            if entry
                .file_name()
                .as_encoded_bytes()
                .starts_with(b".aura-initial-history-")
            {
                history = Some(entry.path());
                break;
            }
        }
        let history = history.ok_or("actual original history")?;
        let mut ciphertext = std::fs::read(&history)?;
        let last = ciphertext.last_mut().ok_or("history ciphertext")?;
        *last ^= 1;
        std::fs::write(&history, &ciphertext)?;
        let error = initialize(owned(&storage))
            .err()
            .ok_or("corrupted protected history admitted")?;
        assert!(
            matches!(
                error,
                AuraError::Crypto {
                    source: Some(_),
                    ..
                }
            ),
            "actual AEAD corruption retains crypto source"
        );
        assert_eq!(
            born,
            directory.read_bounded(std::path::Path::new(BORN), false, MAX_RECORD_BYTES)?
        );
        let unknown = physical.join(".aura-initial-unknown");
        std::fs::write(&unknown, b"foreign transaction")?;
        let error = initialize(owned(&storage))
            .err()
            .ok_or("unknown transaction admitted")?;
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(std::fs::read(&unknown)?, b"foreign transaction");
        Ok(())
    }

    #[test]
    fn original_initial_cutover_recovers_two_successive_original_index_transitions(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("profile");
        killed_original_stage(
            &profile,
            &temporary.path().join("ready"),
            "mutable-index-ready",
        )?;
        killed_original_stage(
            &profile,
            &temporary.path().join("handed"),
            "mutable-index-handed",
        )?;
        killed_original_stage(
            &profile,
            &temporary.path().join("exchange"),
            "cutover:exchange",
        )?;
        let storage = selected(&profile)?;
        let original = legacy_migration::require_preparing_original_anchor(owned(&storage))?;
        let initialized = initialize(owned(&storage))?;
        assert_eq!(initialized.root_identity, original.root);
        let index: Index = read(
            backend(&storage).owned_directory()?,
            INDEX,
            &backend(&storage).wrapping_key,
        )?
        .ok_or("required original index")?;
        validate(&index, initialized.root_identity)?;
        assert!(index.phase == Phase::Handed && index.births.is_empty() && index.pending.is_none());
        drop(initialized);
        drop(storage);
        assert_eq!(root_id(&selected(&profile)?)?, original.root);
        Ok(())
    }

    struct KilledChild(std::process::Child);
    impl Drop for KilledChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn killed_original_stage(
        profile: &std::path::Path,
        marker: &std::path::Path,
        target: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let executable = std::env::current_exe()?;
        let mut child = KilledChild(
            std::process::Command::new(executable)
                .args([
                    "--exact",
                    "secure::allocation_lifetime::initialization::tests::prelink_process_worker",
                ])
                .env("AURA_LIFETIME_PRELINK_PROFILE", profile)
                .env("AURA_LIFETIME_PRELINK_TARGET", target)
                .env("AURA_LIFETIME_PRELINK_MARKER", marker)
                .stdout(std::process::Stdio::null())
                .spawn()?,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !marker.exists() {
            if let Some(status) = child.0.try_wait()? {
                return Err(format!("prelink child exited: {status}").into());
            }
            if std::time::Instant::now() >= deadline {
                return Err("prelink child checkpoint deadline".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.0.kill()?;
        assert!(
            !child.0.wait()?.success(),
            "actual creator must die before target link"
        );
        Ok(())
    }
    #[test]
    fn original_initialization_recovers_exact_prelink_ciphertext_after_process_death(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for target in [
            "lifecycle",
            "birth",
            "handed",
            BORN,
            INDEX,
            READY,
            HANDED,
            MARKER,
        ] {
            eprintln!("checking original pre-link target: {target}");
            let temporary = tempfile::tempdir()?;
            let profile = temporary.path().join("profile");
            killed_original_stage(&profile, &temporary.path().join("checkpoint"), target)?;
            let storage = selected(&profile)?;
            let selected = backend(&storage).owned_directory()?;
            let directory = if ["lifecycle", "birth", "handed"].contains(&target) {
                selected.child(
                    std::path::Path::new(".aura-allocation-lifetime-owner-v1"),
                    false,
                )?
            } else if target == MARKER {
                selected.child(std::path::Path::new(DIRECTORY), false)?
            } else {
                selected.clone()
            };
            let (original, staged) = directory
                .read_private_publication(std::path::Path::new(target), MAX_INDEX_BYTES)?
                .ok_or("actual pre-link ciphertext missing")?;
            assert!(staged.is_some());
            assert!(directory
                .read_bounded(std::path::Path::new(target), true, MAX_INDEX_BYTES)?
                .is_none());
            let initialized = initialize(owned(&storage))?;
            let recovered = directory
                .read_bounded(std::path::Path::new(target), true, MAX_INDEX_BYTES)?
                .ok_or("recovered target missing")?;
            if target == "lifecycle" {
                let (original, _) = backend(&storage).decrypt_fallback_record_with_protection(
                    &SecureStorageLocation::new(".aura-allocation-lifetime-owner-v1", target),
                    &original,
                )?;
                let observed: serde_json::Value =
                    serde_json::from_slice(&Zeroizing::new(original))?;
                let original_root: [u8; 32] =
                    serde_json::from_value(observed["original"]["root"].clone())?;
                assert_eq!(original_root, initialized.root_identity);
            } else if target == INDEX {
                let initial: Index =
                    decode_root(target, &backend(&storage).wrapping_key, &original)?;
                let final_index: Index =
                    decode_root(target, &backend(&storage).wrapping_key, &recovered)?;
                assert_eq!(initial.root, final_index.root);
                assert!(initial.births.is_empty() && final_index.births.is_empty());
                assert!(initial.pending.is_none() && final_index.pending.is_none());
                assert!(final_index.phase == Phase::Handed);
            } else {
                assert_eq!(recovered, original);
            }
            assert!(directory
                .read_private_publication(std::path::Path::new(target), MAX_INDEX_BYTES)?
                .ok_or("acknowledged target")?
                .1
                .is_none());
            assert_eq!(initialized.root_identity, root_id(&storage)?);
        }
        Ok(())
    }
    #[test]
    fn anonymous_legacy_stage_is_retained_and_cannot_authorize_a_new_root(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let storage = selected(temporary.path())?;
        let directory = backend(&storage).owned_directory()?;
        let path = std::path::Path::new(".aura-stage-00000000000000000000000000000000");
        let stage = directory.prepare_private(path, b"unmapped historical ciphertext")?;
        assert!(stage.publish(true)?);
        stage.acknowledge()?;
        assert!(initialize(owned(&storage)).is_err());
        assert_eq!(
            directory
                .read_bounded(path, true, MAX_INDEX_BYTES)?
                .ok_or("historical intent was deleted")?,
            b"unmapped historical ciphertext"
        );
        assert!(directory
            .read_bounded(std::path::Path::new(BORN), true, MAX_INDEX_BYTES)?
            .is_none());
        Ok(())
    }

    #[test]
    fn once_live_target_loss_cannot_be_repaired_from_a_stale_original_stage(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for missing in [BORN, READY, HANDED, INDEX] {
            let temporary = tempfile::tempdir()?;
            let storage = selected(temporary.path())?;
            initialize(owned(&storage))?;
            let directory = backend(&storage).owned_directory()?;
            let original = directory
                .read_bounded(std::path::Path::new(missing), true, MAX_INDEX_BYTES)?
                .ok_or("original target")?;
            let _stage = directory.prepare_private(std::path::Path::new(missing), &original)?;
            assert!(directory.remove(std::path::Path::new(missing))?);
            assert!(initialize(owned(&storage)).is_err());
            assert!(directory
                .read_bounded(std::path::Path::new(missing), true, MAX_INDEX_BYTES)?
                .is_none());
            let (retained, stage) = directory
                .read_private_publication(std::path::Path::new(missing), MAX_INDEX_BYTES)?
                .ok_or("stale evidence removed")?;
            assert_eq!(retained, original);
            assert!(stage.is_some());
        }
        Ok(())
    }
    #[tokio::test]
    async fn conflicting_staged_checkpoint_preserves_actual_positive_first_decision(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let (ordinary, mut root) =
            selected(temporary.path())?.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        let secret = root
            .allocate(b"actual-original", b"original-secret")
            .await?;
        secret
            .decide_positive(b"acknowledged-original-positive")
            .await?;
        let directory = backend(&ordinary).owned_directory()?;
        let _stage =
            directory.prepare_private(std::path::Path::new(INDEX), b"conflicting checkpoint")?;
        assert!(initialize(owned(&ordinary)).is_err());
        match secret.state().await? {
            aura_core::effects::secret_lifetime::SecretLifetimeState::Positive { decision } => {
                assert_eq!(decision, b"acknowledged-original-positive");
            }
            _ => panic!("pre-link recovery cannot rewrite the original decision"),
        }
        assert_eq!(secret.read_live_secret().await?, b"original-secret");
        Ok(())
    }

    #[test]
    fn malformed_prelink_ciphertext_does_not_publish_a_birth_or_replace_key(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let storage = selected(temporary.path())?;
        let backend = backend(&storage);
        let key = backend.wrapping_key;
        let directory = backend.owned_directory()?;
        let _stage = directory.prepare_private(std::path::Path::new(BORN), b"malformed")?;
        assert!(initialize(owned(&storage)).is_err());
        assert!(directory
            .read_bounded(std::path::Path::new(BORN), true, MAX_INDEX_BYTES)?
            .is_none());
        assert_eq!(backend.wrapping_key, key);
        assert!(directory
            .read_private_publication(std::path::Path::new(BORN), MAX_INDEX_BYTES)?
            .ok_or("retained original fault")?
            .1
            .is_some());
        Ok(())
    }

    #[test]
    fn malformed_and_conflicting_prelink_initialization_cannot_replace_original_root(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let storage = selected(temporary.path())?;
        let initialized = initialize(owned(&storage))?;
        let directory = backend(&storage).owned_directory()?;
        let original = directory
            .read_bounded(std::path::Path::new(INDEX), true, MAX_INDEX_BYTES)?
            .ok_or("original index")?;
        let _staged = directory.prepare_private(
            std::path::Path::new(INDEX),
            b"malformed conflicting ciphertext",
        )?;
        let error = initialize(owned(&storage))
            .err()
            .ok_or("conflicting prelink stage authorized overwrite")?;
        let AuraError::Storage {
            source: Some(source),
            ..
        } = error
        else {
            panic!("staged conflict must retain actual native storage failure");
        };
        let native = source
            .downcast_ref::<std::io::Error>()
            .ok_or("native stage error erased")?;
        assert_eq!(native.kind(), std::io::ErrorKind::InvalidData);
        assert!(matches!(
            native
                .get_ref()
                .and_then(|source| source
                    .downcast_ref::<crate::profile_directory::StagedPublicationRecoveryError>()),
            Some(crate::profile_directory::StagedPublicationRecoveryError::ConflictingOriginal)
        ));

        assert_eq!(
            directory
                .read_bounded(std::path::Path::new(INDEX), true, MAX_INDEX_BYTES)?
                .ok_or("original index missing")?,
            original
        );
        assert_eq!(initialized.root_identity, root_id(&storage)?);
        Ok(())
    }

    fn selected(
        path: &std::path::Path,
    ) -> Result<ProductionSecureStorageHandler, Box<dyn std::error::Error>> {
        let owner = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(path.to_path_buf())
                .acquire_owned_native()?,
        );
        Ok(ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)?)
    }
    fn owned(storage: &ProductionSecureStorageHandler) -> &ProfileOwnedSecureStorage {
        let ProductionSecureStorageHandler::ProfileOwned(owned) = storage else {
            panic!("fixture must retain actual selected owner")
        };
        owned
    }
    fn backend(
        storage: &ProductionSecureStorageHandler,
    ) -> &FilesystemFallbackSecureStorageHandler {
        let ProductionSecureStorageHandler::FilesystemFallback(backend) =
            owned(storage).backend.as_ref()
        else {
            panic!("fixture must select actual descriptor provider")
        };
        backend
    }
    fn arm(storage: &ProductionSecureStorageHandler, stage: &'static str) {
        INIT_FAULTS
            .lock()
            .expect("initialization faults")
            .insert((owned(storage)._owner.profile_identity().to_string(), stage));
    }
    fn root_id(storage: &ProductionSecureStorageHandler) -> Result<[u8; 32], AuraError> {
        let backend = backend(storage);
        let directory = backend
            .owned_directory()
            .map_err(|source| source_error("test required directory", source))?;
        let seal: RootSeal = read(directory, BORN, &backend.wrapping_key)?
            .ok_or_else(|| invalid("fixture original birth seal missing"))?;
        Ok(seal.root)
    }
    #[tokio::test]
    async fn live_owner_requires_original_birth_and_directory_marker_on_every_read(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for missing in [BORN, MARKER] {
            let profile = tempfile::tempdir()?;
            let (ordinary, mut root) =
                selected(profile.path())?.into_selected_profile_lifetime_channel()?;
            root.recover_owned_inventory().await?;
            let original = root
                .allocate(b"actual-original", b"retained-secret")
                .await?;
            let selected = backend(&ordinary).owned_directory()?;
            if missing == BORN {
                selected.remove(std::path::Path::new(BORN))?;
            } else {
                selected
                    .child(std::path::Path::new(DIRECTORY), false)?
                    .remove(std::path::Path::new(MARKER))?;
            }
            let failure = original.read_live_secret().await.expect_err(
                "live owner cannot use cached birth identity after original evidence loss",
            );
            assert!(matches!(failure, AuraError::Storage { .. }));
            let reason = std::error::Error::source(&failure)
                .and_then(|source| source.downcast_ref::<AllocationLifetimeRecoveryError>())
                .expect("actual missing original evidence retains structural cause");
            assert!(matches!(
                (missing, reason),
                (BORN, AllocationLifetimeRecoveryError::OriginalBirth)
                    | (MARKER, AllocationLifetimeRecoveryError::OriginalMarker)
            ));
            assert!(
                original
                    .decide_negative(b"infra-test-negative")
                    .await
                    .is_err(),
                "missing original evidence blocks mutation as well as reads"
            );
        }
        Ok(())
    }
    #[tokio::test]
    async fn handed_empty_profile_missing_ready_seal_cannot_reinitialize(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (ordinary, root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        backend(&ordinary)
            .owned_directory()?
            .remove(std::path::Path::new(READY))?;
        drop(root);
        drop(ordinary);
        let restored = selected(profile.path())?;
        assert!(
            restored.into_selected_profile_lifetime_channel().is_err(),
            "already transferred empty owner cannot claim interrupted pre-live initialization"
        );
        Ok(())
    }
    #[tokio::test]
    async fn interrupted_birth_handoff_recovers_original_secret_once(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (ordinary, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        assert!(root.recover_owned_inventory().await?.is_empty());
        arm(&ordinary, "birth-prepared");
        let error = match root
            .allocate(b"original-generation", b"original-secret")
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("injected required birth handoff interruption"),
        };
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        while cause.downcast_ref::<std::io::Error>().is_none() {
            cause = cause
                .source()
                .expect("original required initialization fault retained");
        }
        let recovered = root.reconcile_unhanded_births().await?;
        assert_eq!(
            recovered.len(),
            1,
            "recover exact pre-live birth, no replacement"
        );
        assert_eq!(recovered[0].reference().scope, b"original-generation");
        assert_eq!(
            recovered[0].read_live_secret().await?.as_slice(),
            b"original-secret"
        );
        assert!(
            root.reconcile_unhanded_births().await?.is_empty(),
            "custody cannot be handed out twice"
        );
        Ok(())
    }
    #[tokio::test]
    async fn interrupted_pre_live_initialization_finishes_original_birth_seal(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::error::Error;
        let profile = tempfile::tempdir()?;
        let storage = selected(profile.path())?;
        arm(&storage, "birth-seal");
        let error = match storage.into_selected_profile_lifetime_channel() {
            Err(error) => error,
            Ok(_) => panic!("interrupted initial ACK cannot issue channel"),
        };
        assert!(error
            .source()
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .is_some());
        let restored = selected(profile.path())?;
        let before = root_id(&restored)?;
        let (ordinary, mut root) = restored.into_selected_profile_lifetime_channel()?;
        assert_eq!(root_id(&ordinary)?, before);
        assert!(root.recover_owned_inventory().await?.is_empty());
        root.allocate(b"actual-after-original-initialization", b"secret")
            .await?;
        Ok(())
    }
    #[tokio::test]
    async fn interrupted_ready_publication_does_not_allocate_a_replacement_root(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let storage = selected(profile.path())?;
        arm(&storage, "ready-index");
        assert!(storage.into_selected_profile_lifetime_channel().is_err());
        let restored = selected(profile.path())?;
        let before = root_id(&restored)?;
        let (ordinary, mut root) = restored.into_selected_profile_lifetime_channel()?;
        assert_eq!(root_id(&ordinary)?, before);
        assert!(root.recover_owned_inventory().await?.is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn once_live_missing_checkpoint_fails_without_reconstruction(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::error::Error;
        let profile = tempfile::tempdir()?;
        let (ordinary, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        let secret = root
            .allocate(b"actual-original-generation", b"secret")
            .await?;
        backend(&ordinary)
            .owned_directory()?
            .remove(std::path::Path::new(INDEX))?;
        drop(secret);
        drop(root);
        drop(ordinary);
        let restored = selected(profile.path())?;
        let error = match restored.into_selected_profile_lifetime_channel() {
            Err(error) => error,
            Ok(_) => panic!("lost live checkpoint cannot issue new root"),
        };
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<AllocationLifetimeRecoveryError>()),
            Some(AllocationLifetimeRecoveryError::LiveCheckpoint)
        ));
        let inspected = selected(profile.path())?;
        assert!(
            backend(&inspected)
                .owned_directory()?
                .read(std::path::Path::new(INDEX), true)?
                .is_none(),
            "failure must not reconstruct lost live checkpoint"
        );
        Ok(())
    }
    #[tokio::test]
    async fn missing_live_leaf_cannot_restore_from_initialization_or_use_empty_inventory(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (ordinary, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        let secret = root.allocate(b"actual-live-generation", b"secret").await?;
        let path = FilesystemLifetimeRoot::path(secret.reference());
        let ledger = backend(&ordinary)
            .owned_directory()?
            .child(std::path::Path::new(DIRECTORY), false)?;
        ledger.remove(&path)?;
        drop(secret);
        drop(root);
        drop(ordinary);
        let (_ordinary, mut restored) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        assert!(restored.recover_owned_inventory().await.is_err());
        assert!(
            ledger.read(&path, true)?.is_none(),
            "live leaf is never recreated from duration, key or scope"
        );
        Ok(())
    }
    #[tokio::test]
    async fn missing_original_birth_seal_rejects_existing_live_profile(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (ordinary, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        backend(&ordinary)
            .owned_directory()?
            .remove(std::path::Path::new(BORN))?;
        drop(root);
        drop(ordinary);
        let restored = selected(profile.path())?;
        assert!(restored.into_selected_profile_lifetime_channel().is_err());
        Ok(())
    }
    #[tokio::test]
    async fn legacy_selected_profile_migrates_without_rewriting_permanent_records(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageEffects};
        let profile = tempfile::tempdir()?;
        let ordinary = selected(profile.path())?;
        let permanent = SecureStorageLocation::new("legacy", "signing-wrap");
        let mutable = SecureStorageLocation::new("legacy", "observed-state");
        ordinary
            .secure_store_immutable(
                &permanent,
                b"original-permanent",
                &[SecureStorageCapability::Write],
            )
            .await?;
        ordinary
            .secure_store(
                &mutable,
                b"original-mutable",
                &[SecureStorageCapability::Write],
            )
            .await?;
        let original_bytes = backend(&ordinary)
            .owned_directory()?
            .read(&backend(&ordinary).descriptor_path(&permanent)?, true)?
            .ok_or("original encrypted record missing")?;
        let (ordinary, mut root) = ordinary.into_selected_profile_lifetime_channel()?;
        assert!(root.recover_owned_inventory().await?.is_empty());
        let allocated = root
            .allocate(b"new-generation", b"new-retirable-wrap")
            .await?;
        allocated
            .decide_negative(b"owned-negative-first-decision")
            .await?
            .retire()
            .await?;
        let retained_bytes = backend(&ordinary)
            .owned_directory()?
            .read(&backend(&ordinary).descriptor_path(&permanent)?, true)?
            .ok_or("retained encrypted record missing")?;
        assert_eq!(
            original_bytes, retained_bytes,
            "migration must preserve actual ciphertext and protection header"
        );
        assert_eq!(
            ordinary
                .secure_retrieve(&permanent, &[SecureStorageCapability::Read])
                .await?,
            b"original-permanent"
        );
        assert!(ordinary
            .secure_delete(&permanent, &[SecureStorageCapability::Delete])
            .await
            .is_err());
        assert!(ordinary
            .secure_store(
                &permanent,
                b"replacement",
                &[SecureStorageCapability::Write]
            )
            .await
            .is_err());
        assert_eq!(
            ordinary
                .secure_retrieve(&mutable, &[SecureStorageCapability::Read])
                .await?,
            b"original-mutable"
        );
        drop(allocated);
        drop(root);
        drop(ordinary);
        let (ordinary, mut restarted) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        let recovered = restarted.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 1);
        assert!(matches!(
            recovered[0].state().await?,
            SecretLifetimeState::Retired { .. }
        ));
        assert_eq!(
            ordinary
                .secure_retrieve(&permanent, &[SecureStorageCapability::Read])
                .await?,
            b"original-permanent"
        );
        Ok(())
    }
    #[tokio::test]
    async fn corrupted_legacy_record_prevents_migration_without_replacing_original_key(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageEffects};
        let profile = tempfile::tempdir()?;
        let ordinary = selected(profile.path())?;
        let record = SecureStorageLocation::new("legacy", "original");
        ordinary
            .secure_store(&record, b"original", &[SecureStorageCapability::Write])
            .await?;
        let directory = backend(&ordinary).owned_directory()?;
        let key = directory
            .read(std::path::Path::new(FALLBACK_WRAPPING_KEY_FILENAME), true)?
            .ok_or("original key missing")?;
        let prepared = directory.prepare_private(
            &backend(&ordinary).descriptor_path(&record)?,
            b"actually corrupted ciphertext",
        )?;
        assert!(prepared.publish(false)?);
        prepared.acknowledge()?;
        let failure = initialize(owned(&ordinary))
            .err()
            .expect("corrupt existing selected record cannot establish birth");
        assert!(matches!(failure, AuraError::Storage { .. }));
        assert_eq!(
            directory
                .read(std::path::Path::new(FALLBACK_WRAPPING_KEY_FILENAME), true)?
                .ok_or("retained key missing")?,
            key
        );
        assert!(read::<RootSeal>(directory, BORN, &backend(&ordinary).wrapping_key)?.is_none());
        Ok(())
    }
    #[tokio::test]
    async fn completed_legacy_handoff_missing_original_birth_cannot_remigrate(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageEffects};
        let profile = tempfile::tempdir()?;
        let ordinary = selected(profile.path())?;
        ordinary
            .secure_store_immutable(
                &SecureStorageLocation::new("legacy", "original"),
                b"permanent",
                &[SecureStorageCapability::Write],
            )
            .await?;
        let (ordinary, root) = ordinary.into_selected_profile_lifetime_channel()?;
        let directory = backend(&ordinary).owned_directory()?;
        assert!(directory.remove(std::path::Path::new(BORN))?);
        drop(root);
        drop(ordinary);
        let restarted = selected(profile.path())?;
        let failure = initialize(owned(&restarted))
            .err()
            .expect("completed original migration cannot be reconstructed");
        assert!(matches!(failure, AuraError::Storage { .. }));
        assert!(read::<RootSeal>(
            backend(&restarted).owned_directory()?,
            BORN,
            &backend(&restarted).wrapping_key
        )?
        .is_none());
        Ok(())
    }
    #[tokio::test]
    async fn interrupted_legacy_migration_reuses_acknowledged_original_birth_anchor(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageEffects};
        let profile = tempfile::tempdir()?;
        let ordinary = selected(profile.path())?;
        let original = SecureStorageLocation::new("legacy", "original");
        ordinary
            .secure_store_immutable(
                &original,
                b"retained-permanent",
                &[SecureStorageCapability::Write],
            )
            .await?;
        arm(&ordinary, "migration-birth-anchor");
        let failure = initialize(owned(&ordinary))
            .err()
            .expect("actual fault interrupts acknowledged migration before ledger birth");
        assert!(std::error::Error::source(&failure).is_some());
        let path = std::path::Path::new(".aura-allocation-lifetime-owner-v1/birth");
        let anchored = backend(&ordinary)
            .owned_directory()?
            .read(path, true)?
            .ok_or("original acknowledged migration anchor absent")?;
        assert!(read::<RootSeal>(
            backend(&ordinary).owned_directory()?,
            BORN,
            &backend(&ordinary).wrapping_key
        )?
        .is_none());
        drop(ordinary);
        let (ordinary, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        assert!(root.recover_owned_inventory().await?.is_empty());
        assert_eq!(
            backend(&ordinary)
                .owned_directory()?
                .read(path, true)?
                .ok_or("original anchor disappeared")?,
            anchored,
            "interrupted migration must retain exact original authenticated birth"
        );
        assert_eq!(
            ordinary
                .secure_retrieve(&original, &[SecureStorageCapability::Read])
                .await?,
            b"retained-permanent"
        );
        Ok(())
    }
    #[tokio::test]
    async fn retained_birth_cannot_reconstruct_lost_completed_lifetime_state(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (ordinary, root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        let directory = backend(&ordinary).owned_directory()?;
        let birth_path = std::path::Path::new(".aura-allocation-lifetime-owner-v1/birth");
        let birth = directory
            .read(birth_path, true)?
            .ok_or("original protected birth anchor")?;
        assert!(directory.remove(std::path::Path::new(
            ".aura-allocation-lifetime-owner-v1/handed"
        ))?);
        for path in [BORN, READY, HANDED, INDEX] {
            assert!(directory.remove(std::path::Path::new(path))?);
        }
        assert!(directory
            .child(std::path::Path::new(DIRECTORY), false)?
            .remove(std::path::Path::new(MARKER))?);
        drop(root);
        drop(ordinary);
        let restored = selected(profile.path())?;
        let error = initialize(owned(&restored))
            .err()
            .expect("actual Handed lifecycle cannot become Preparing from missing records");
        assert!(matches!(
            std::error::Error::source(&error)
                .and_then(|cause| cause.downcast_ref::<AllocationLifetimeRecoveryError>()),
            Some(AllocationLifetimeRecoveryError::ReadinessSeal)
        ));
        assert_eq!(
            backend(&restored)
                .owned_directory()?
                .read(birth_path, true)?
                .ok_or("original birth retained")?,
            birth
        );
        for path in [BORN, READY, HANDED, INDEX] {
            assert!(backend(&restored)
                .owned_directory()?
                .read(std::path::Path::new(path), true)?
                .is_none());
        }
        Ok(())
    }
    #[tokio::test]
    async fn missing_lifecycle_with_retained_birth_never_reconstructs_phase(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (ordinary, root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        let directory = backend(&ordinary).owned_directory()?;
        assert!(directory.remove(std::path::Path::new(
            ".aura-allocation-lifetime-owner-v1/lifecycle"
        ))?);
        drop(root);
        drop(ordinary);
        let restored = selected(profile.path())?;
        let error = initialize(owned(&restored))
            .err()
            .expect("missing required lifecycle is not pre-live proof");
        assert!(matches!(
            std::error::Error::source(&error)
                .and_then(|cause| cause.downcast_ref::<AllocationLifetimeRecoveryError>()),
            Some(AllocationLifetimeRecoveryError::LiveCheckpoint)
        ));
        assert!(backend(&restored)
            .owned_directory()?
            .read(
                std::path::Path::new(".aura-allocation-lifetime-owner-v1/lifecycle"),
                true
            )?
            .is_none());
        Ok(())
    }
    #[tokio::test]
    async fn authenticated_root_and_anchor_codec_failures_remain_serialization(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for corrupt_anchor in [false, true] {
            let profile = tempfile::tempdir()?;
            let (ordinary, root) =
                selected(profile.path())?.into_selected_profile_lifetime_channel()?;
            let backend = backend(&ordinary);
            let directory = backend.owned_directory()?;
            if corrupt_anchor {
                let location =
                    SecureStorageLocation::new(".aura-allocation-lifetime-owner-v1", "birth");
                let bytes =
                    backend.encrypt_fallback_record_with_protection(&location, b"{", true)?;
                let prepared = directory.prepare_private(
                    std::path::Path::new(".aura-allocation-lifetime-owner-v1/birth"),
                    &bytes,
                )?;
                assert!(prepared.publish(false)?);
                prepared.acknowledge()?;
            } else {
                publish(
                    directory,
                    BORN,
                    &backend.wrapping_key,
                    &"authenticated wrong root schema",
                    false,
                )?;
            }
            let error = initialize(owned(&ordinary)).err().expect(
                "genuinely authenticated malformed codec must fail before recovery mutation",
            );
            assert!(matches!(error, AuraError::Serialization { .. }));
            assert!(std::error::Error::source(&error)
                .and_then(|cause| cause.downcast_ref::<serde_json::Error>())
                .is_some());
            drop(root);
            drop(ordinary);
        }
        Ok(())
    }
}

pub(super) fn missing_live_allocation() -> AuraError {
    recovery_error(AllocationLifetimeRecoveryError::LiveAllocation)
}
