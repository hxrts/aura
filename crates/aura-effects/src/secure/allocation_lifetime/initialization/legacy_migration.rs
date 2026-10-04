//! Local selected-provider migration proof. No identity or retirement policy is
//! inferred from legacy plaintext; every existing protection bit is preserved.
use super::*;
const NAMESPACE: &str = ".aura-allocation-lifetime-owner-v1";
const BIRTH: &str = "birth";
const COMPLETE: &str = "handed";
const MAX_LEGACY_RECORDS: usize = 4096;
const MAX_LEGACY_BYTES: usize = 8 * 1024 * 1024;
const MAX_LEGACY_TOTAL_BYTES: usize = 32 * 1024 * 1024;
#[derive(Debug, thiserror::Error)]
#[error("required legacy migration inventory exceeds {kind} bound ({observed} > {limit})")]
struct LegacyMigrationLimitExceeded {
    kind: &'static str,
    limit: usize,
    observed: usize,
}
fn migration_limit(kind: &'static str, limit: usize, observed: usize) -> AuraError {
    source_error(
        "bounded required original legacy inventory",
        LegacyMigrationLimitExceeded {
            kind,
            limit,
            observed,
        },
    )
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Anchor {
    version: u16,
    root: [u8; 32],
    authenticated_inventory: [u8; 32],
}
fn location(key: &str) -> SecureStorageLocation {
    SecureStorageLocation {
        namespace: NAMESPACE.into(),
        key: key.into(),
        sub_key: None,
    }
}
fn selected(
    owned: &ProfileOwnedSecureStorage,
) -> Result<&FilesystemFallbackSecureStorageHandler, AuraError> {
    match owned.backend.as_ref() {
        ProductionSecureStorageHandler::FilesystemFallback(backend) => Ok(backend),
        _ => Err(SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error()),
    }
}
fn anchor_directory(
    backend: &FilesystemFallbackSecureStorageHandler,
    create: bool,
) -> Result<Option<crate::profile_directory::ProfileDirectory>, AuraError> {
    match backend
        .owned_directory()?
        .child(std::path::Path::new(NAMESPACE), create)
    {
        Ok(directory) => Ok(Some(directory)),
        Err(source) if !create && source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(source_error(
            "required original migration anchor directory",
            source,
        )),
    }
}
fn read_anchor(
    backend: &FilesystemFallbackSecureStorageHandler,
    key: &str,
) -> Result<Option<(Anchor, [u8; 32])>, AuraError> {
    let Some(directory) = anchor_directory(backend, false)? else {
        return Ok(None);
    };
    let Some((bytes, staged)) = directory
        .read_private_publication(std::path::Path::new(key), MAX_LEGACY_BYTES)
        .map_err(|source| source_error("required immutable migration anchor", source))?
    else {
        return Ok(None);
    };
    let (plaintext, immutable) =
        backend.decrypt_fallback_record_with_protection(&location(key), &bytes)?;
    let plaintext = Zeroizing::new(plaintext);
    if !immutable {
        return Err(invalid(
            "migration anchor lost original permanent protection",
        ));
    }
    let anchor: Anchor = serde_json::from_slice(&plaintext)
        .map_err(|source| codec_error("decode authenticated migration anchor", source))?;
    if anchor.version != 1 || anchor.root == [0; 32] {
        return Err(invalid("invalid authenticated migration anchor"));
    }
    if staged.is_some() {
        let (state, _) = read_state(backend)?
            .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveCheckpoint))?;
        if state.phase == OwnerPhase::Handed
            && directory
                .read_canonical_private_publication(std::path::Path::new(key), MAX_LEGACY_BYTES)
                .map_err(|source| source_error("required handed original anchor target", source))?
                .is_none()
        {
            return Err(recovery_error(if key == BIRTH {
                AllocationLifetimeRecoveryError::OriginalBirth
            } else {
                AllocationLifetimeRecoveryError::ReadinessSeal
            }));
        }
        if anchor != state.original {
            return Err(invalid(
                "staged migration anchor differs from original protected lifecycle",
            ));
        }
    }
    finish_staged_publication(staged)?;
    Ok(Some((anchor, aura_core::hash::hash(&bytes))))
}
fn publish_anchor(
    backend: &FilesystemFallbackSecureStorageHandler,
    key: &str,
    original: &Anchor,
) -> Result<[u8; 32], AuraError> {
    let directory = anchor_directory(backend, true)?
        .ok_or_else(|| invalid("created anchor directory absent"))?;
    if let Some((retained, digest)) = read_anchor(backend, key)? {
        if &retained != original {
            return Err(invalid("immutable migration anchor conflict"));
        }
        directory
            .acknowledge_entries()
            .map_err(|source| source_error("acknowledge retained migration anchor", source))?;
        return Ok(digest);
    }
    let plaintext = Zeroizing::new(
        serde_json::to_vec(original)
            .map_err(|source| codec_error("encode original migration anchor", source))?,
    );
    let bytes =
        backend.encrypt_fallback_record_with_protection(&location(key), &plaintext, true)?;
    let prepared = directory
        .prepare_private(std::path::Path::new(key), &bytes)
        .map_err(|source| source_error("prepare immutable migration anchor", source))?;
    #[cfg(test)]
    prelink_process_checkpoint(key)?;
    if !prepared
        .publish(true)
        .map_err(|source| source_error("publish immutable migration anchor", source))?
    {
        return Err(invalid("immutable migration anchor publication conflict"));
    }
    prepared
        .acknowledge()
        .map_err(|source| source_error("acknowledge immutable migration anchor", source))?;
    Ok(aura_core::hash::hash(&bytes))
}
fn component(name: &std::ffi::OsStr) -> Result<String, AuraError> {
    let encoded = name
        .to_str()
        .ok_or_else(|| invalid("legacy secure record path is not UTF-8"))?;
    let decoded = FilesystemFallbackSecureStorageHandler::decode_component(encoded)?;
    if FilesystemFallbackSecureStorageHandler::encode_component(&decoded) != encoded {
        return Err(invalid("legacy secure record path is not canonical"));
    }
    Ok(decoded)
}
fn authenticate_leaf(
    backend: &FilesystemFallbackSecureStorageHandler,
    directory: &crate::profile_directory::ProfileDirectory,
    name: &std::ffi::OsStr,
    location: SecureStorageLocation,
    inventory: &mut Vec<u8>,
    count: &mut usize,
    total_bytes: &mut usize,
) -> Result<(), AuraError> {
    FilesystemFallbackSecureStorageHandler::validate_location(&location)?;
    *count = count
        .checked_add(1)
        .ok_or_else(|| invalid("legacy secure inventory count overflow"))?;
    if *count > MAX_LEGACY_RECORDS {
        return Err(migration_limit("entries", MAX_LEGACY_RECORDS, *count));
    }
    let bytes = directory
        .read_bounded(std::path::Path::new(name), true, MAX_LEGACY_BYTES)
        .map_err(|source| source_error("required legacy secure record", source))?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveAllocation))?;
    *total_bytes = total_bytes
        .checked_add(bytes.len())
        .ok_or_else(|| migration_limit("bytes", MAX_LEGACY_TOTAL_BYTES, usize::MAX))?;
    if *total_bytes > MAX_LEGACY_TOTAL_BYTES {
        return Err(migration_limit(
            "bytes",
            MAX_LEGACY_TOTAL_BYTES,
            *total_bytes,
        ));
    }
    let (plaintext, protected) =
        backend.decrypt_fallback_record_with_protection(&location, &bytes)?;
    let _plaintext = Zeroizing::new(plaintext);
    let path = location.full_path();
    inventory.extend_from_slice(&(path.len() as u64).to_le_bytes());
    inventory.extend_from_slice(path.as_bytes());
    inventory.push(u8::from(protected));
    inventory.extend_from_slice(&aura_core::hash::hash(&bytes));
    Ok(())
}
fn authenticate_legacy_inventory(
    backend: &FilesystemFallbackSecureStorageHandler,
) -> Result<[u8; 32], AuraError> {
    authenticate_inventory_excluding_original_stage(backend, None)
}
fn authenticate_inventory_excluding_original_stage(
    backend: &FilesystemFallbackSecureStorageHandler,
    original_stage: Option<&crate::profile_directory::PreparedProfileFile>,
) -> Result<[u8; 32], AuraError> {
    let provider = backend.owned_directory()?;
    let mut names = provider
        .names_bounded(MAX_LEGACY_RECORDS + 1)
        .map_err(|source| source_error("required legacy provider inventory", source))?;
    names.sort();
    let mut count = 0;
    let mut total_bytes = 0;
    let mut inventory = Vec::new();
    for namespace in names {
        count += 1;
        if count > MAX_LEGACY_RECORDS {
            return Err(migration_limit("entries", MAX_LEGACY_RECORDS, count));
        }
        if namespace == std::ffi::OsStr::new(FALLBACK_WRAPPING_KEY_FILENAME) {
            continue;
        }
        // Initial proof never treats any prior lifetime metadata as legacy data.
        let decoded_namespace = component(&namespace)?;
        if decoded_namespace == NAMESPACE {
            if let Some(stage) = original_stage {
                let directory = provider
                    .child(std::path::Path::new(&namespace), false)
                    .map_err(|source| source_error("required original staged namespace", source))?;
                stage.require_only_entry_in(&directory).map_err(|source| {
                    source_error("required exact original staged inventory", source)
                })?;
                continue;
            }
        }
        if [NAMESPACE, DIRECTORY, BORN, READY, HANDED, INDEX].contains(&decoded_namespace.as_str())
        {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::OriginalBirth,
            ));
        }
        let directory = provider
            .child(std::path::Path::new(&namespace), false)
            .map_err(|source| source_error("required canonical legacy namespace", source))?;
        let mut keys = directory
            .names_bounded(MAX_LEGACY_RECORDS + 1)
            .map_err(|source| source_error("required legacy key inventory", source))?;
        keys.sort();
        for key in keys {
            count += 1;
            if count > MAX_LEGACY_RECORDS {
                return Err(migration_limit("entries", MAX_LEGACY_RECORDS, count));
            }
            let decoded_key = component(&key)?;
            match directory.child(std::path::Path::new(&key), false) {
                Ok(subdirectory) => {
                    let mut subkeys =
                        subdirectory
                            .names_bounded(MAX_LEGACY_RECORDS + 1)
                            .map_err(|source| {
                                source_error("required legacy subkey inventory", source)
                            })?;
                    subkeys.sort();
                    for subkey in subkeys {
                        count += 1;
                        if count > MAX_LEGACY_RECORDS {
                            return Err(migration_limit("entries", MAX_LEGACY_RECORDS, count));
                        }
                        let decoded_subkey = component(&subkey)?;
                        authenticate_leaf(
                            backend,
                            &subdirectory,
                            &subkey,
                            SecureStorageLocation {
                                namespace: decoded_namespace.clone(),
                                key: decoded_key.clone(),
                                sub_key: Some(decoded_subkey),
                            },
                            &mut inventory,
                            &mut count,
                            &mut total_bytes,
                        )?;
                    }
                }
                Err(source)
                    if source.raw_os_error() == Some(rustix::io::Errno::NOTDIR.raw_os_error()) =>
                {
                    authenticate_leaf(
                        backend,
                        &directory,
                        &key,
                        SecureStorageLocation {
                            namespace: decoded_namespace.clone(),
                            key: decoded_key,
                            sub_key: None,
                        },
                        &mut inventory,
                        &mut count,
                        &mut total_bytes,
                    )?;
                }
                Err(source) => return Err(source_error("required legacy record shape", source)),
            }
        }
    }
    Ok(aura_core::hash::hash(&inventory))
}
const STATE: &str = "lifecycle";
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
enum OwnerPhase {
    Preparing,
    Handed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerState {
    version: u16,
    original: Anchor,
    phase: OwnerPhase,
}
fn codec_error(message: &'static str, source: serde_json::Error) -> AuraError {
    AuraError::Serialization {
        message: message.into(),
        source: Some(Arc::new(source)),
    }
}
fn read_state(
    backend: &FilesystemFallbackSecureStorageHandler,
) -> Result<Option<(OwnerState, [u8; 32])>, AuraError> {
    let Some(directory) = anchor_directory(backend, false)? else {
        return Ok(None);
    };
    let Some((bytes, staged)) = directory
        .read_private_publication(std::path::Path::new(STATE), MAX_LEGACY_BYTES)
        .map_err(|source| source_error("required original lifecycle state", source))?
    else {
        return Ok(None);
    };
    let (plaintext, immutable) =
        backend.decrypt_fallback_record_with_protection(&location(STATE), &bytes)?;
    let plaintext = Zeroizing::new(plaintext);
    if immutable {
        return Err(invalid(
            "original lifecycle state has incompatible protection",
        ));
    }
    let state: OwnerState = serde_json::from_slice(&plaintext)
        .map_err(|source| codec_error("decode authenticated lifecycle state", source))?;
    if state.version != 1 || state.original.version != 1 || state.original.root == [0; 32] {
        return Err(invalid("invalid original lifecycle state"));
    }
    if staged.is_some()
        && directory
            .read_canonical_private_publication(std::path::Path::new(STATE), MAX_LEGACY_BYTES)
            .map_err(|source| source_error("required original lifecycle target", source))?
            .is_none()
    {
        require_original_initial_state(
            backend,
            &directory,
            &state,
            staged
                .as_ref()
                .ok_or_else(|| invalid("original lifecycle stage absent"))?,
        )?;
    }
    finish_staged_publication(staged)?;
    Ok(Some((state, aura_core::hash::hash(&bytes))))
}
fn require_original_initial_state(
    backend: &FilesystemFallbackSecureStorageHandler,
    directory: &crate::profile_directory::ProfileDirectory,
    state: &OwnerState,
    stage: &crate::profile_directory::PreparedProfileFile,
) -> Result<(), AuraError> {
    if state.phase != OwnerPhase::Preparing {
        return Err(recovery_error(
            AllocationLifetimeRecoveryError::LiveCheckpoint,
        ));
    }
    for path in [BIRTH, COMPLETE] {
        if directory
            .read_canonical_private_publication(std::path::Path::new(path), MAX_LEGACY_BYTES)
            .map_err(|source| source_error("required initial lifecycle anchor absence", source))?
            .is_some()
        {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::LiveCheckpoint,
            ));
        }
    }
    let provider = backend.owned_directory()?;
    for path in [BORN, READY, HANDED, INDEX] {
        if provider
            .read_canonical_private_publication(std::path::Path::new(path), MAX_LEGACY_BYTES)
            .map_err(|source| source_error("required initial lifetime metadata absence", source))?
            .is_some()
        {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::LiveCheckpoint,
            ));
        }
    }
    if authenticate_inventory_excluding_original_stage(backend, Some(stage))?
        != state.original.authenticated_inventory
    {
        return Err(invalid("original staged lifecycle inventory changed"));
    }
    Ok(())
}

fn canonical_original_state(
    backend: &FilesystemFallbackSecureStorageHandler,
) -> Result<OwnerState, AuraError> {
    let directory = anchor_directory(backend, false)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveCheckpoint))?;
    let bytes = directory
        .read_canonical_private_publication(std::path::Path::new(STATE), MAX_LEGACY_BYTES)
        .map_err(|source| source_error("required canonical original lifecycle", source))?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveCheckpoint))?;
    decode_original_state(backend, &bytes)
}
fn decode_original_state(
    backend: &FilesystemFallbackSecureStorageHandler,
    bytes: &[u8],
) -> Result<OwnerState, AuraError> {
    let (plaintext, immutable) =
        backend.decrypt_fallback_record_with_protection(&location(STATE), bytes)?;
    let plaintext = Zeroizing::new(plaintext);
    if immutable {
        return Err(invalid(
            "original lifecycle state has incompatible protection",
        ));
    }
    let state: OwnerState = serde_json::from_slice(&plaintext)
        .map_err(|source| codec_error("decode canonical original lifecycle", source))?;
    if state.version != 1 || state.original.version != 1 || state.original.root == [0; 32] {
        return Err(invalid("invalid canonical original lifecycle"));
    }
    Ok(state)
}
fn canonical_original_anchor(
    backend: &FilesystemFallbackSecureStorageHandler,
    path: &str,
) -> Result<Option<Anchor>, AuraError> {
    let directory = anchor_directory(backend, false)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
    let Some(bytes) = directory
        .read_canonical_private_publication(std::path::Path::new(path), MAX_LEGACY_BYTES)
        .map_err(|source| source_error("required canonical protected original anchor", source))?
    else {
        return Ok(None);
    };
    let (plaintext, immutable) =
        backend.decrypt_fallback_record_with_protection(&location(path), &bytes)?;
    let plaintext = Zeroizing::new(plaintext);
    if !immutable {
        return Err(invalid("original anchor lacks required protection"));
    }
    let anchor: Anchor = serde_json::from_slice(&plaintext)
        .map_err(|source| codec_error("decode canonical original anchor", source))?;
    if anchor.version != 1 || anchor.root == [0; 32] {
        return Err(invalid("invalid canonical original anchor"));
    }
    Ok(Some(anchor))
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn require_preparing_original_anchor(
    owner: &ProfileOwnedSecureStorage,
) -> Result<RootSeal, AuraError> {
    let backend = selected(owner)?;
    let state = canonical_original_state(backend)?;
    if state.phase != OwnerPhase::Preparing {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    let birth = canonical_original_anchor(backend, BIRTH)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
    if birth != state.original {
        return Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        ));
    }
    Ok(RootSeal {
        version: 1,
        root: birth.root,
    })
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn require_original_completion(
    owner: &ProfileOwnedSecureStorage,
    root: [u8; 32],
    required: bool,
) -> Result<(), AuraError> {
    let backend = selected(owner)?;
    let state = canonical_original_state(backend)?;
    if state.phase != OwnerPhase::Preparing || state.original.root != root {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    let complete = canonical_original_anchor(backend, COMPLETE)?;
    match (required, complete) {
        (false, None) => Ok(()),
        (true, Some(anchor)) if anchor == state.original => Ok(()),
        _ => Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        )),
    }
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "VerifiedOriginalInitializationSuccessor",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(super) fn verify_original_lifecycle_successor<'a>(
    owner: &'a ProfileOwnedSecureStorage,
    observation: &'a crate::profile_directory::ObservedOriginalSuccessor,
) -> Result<VerifiedOriginalInitializationSuccessor<'a>, AuraError> {
    let backend = selected(owner)?;
    let target = PathBuf::from(NAMESPACE).join(STATE);
    observation
        .publication()
        .require_owner_target(backend.owned_directory()?, &target)
        .map_err(|source| {
            source_error(
                "required original lifecycle successor physical owner",
                source,
            )
        })?;
    let old = decode_original_state(backend, observation.original())?;
    let next = decode_original_state(backend, observation.next())?;
    if old.phase != OwnerPhase::Preparing
        || next.phase != OwnerPhase::Handed
        || old.original != next.original
    {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    for path in [BIRTH, COMPLETE] {
        let anchor = canonical_original_anchor(backend, path)?
            .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Seal))?;
        if anchor != old.original {
            return Err(transition_error(
                OriginalInitializationTransitionError::Seal,
            ));
        }
    }
    require_empty_handed_original_root(owner, old.original.root)?;
    Ok(VerifiedOriginalInitializationSuccessor {
        owner,
        publication: observation.publication(),
        target,
        original_digest: aura_core::hash::hash(observation.original()),
        next_digest: aura_core::hash::hash(observation.next()),
    })
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn recover_original_handoff_successor(
    owner: &ProfileOwnedSecureStorage,
) -> Result<(), AuraError> {
    let backend = selected(owner)?;
    let Some(directory) = anchor_directory(backend, false)? else {
        return Ok(());
    };
    let Some(observation) = directory
        .observe_original_successor(std::path::Path::new(STATE), MAX_LEGACY_BYTES)
        .map_err(|source| source_error("observe original lifecycle handoff successor", source))?
    else {
        return Ok(());
    };
    let witness = verify_original_lifecycle_successor(owner, &observation)?;
    super::cutover::retain_journal(&witness, &observation)?;
    observation
        .publication()
        .publish_verified_initial_successor(&witness, MAX_LEGACY_BYTES)
        .map_err(|source| {
            source_error("acknowledge original lifecycle handoff successor", source)
        })?;
    super::cutover::finish_journal(&witness)
}

fn publish_state(
    backend: &FilesystemFallbackSecureStorageHandler,
    state: &OwnerState,
    initial: bool,
) -> Result<[u8; 32], AuraError> {
    if let Some((old, _)) = read_state(backend)? {
        if old.original != state.original
            || initial
            || old.phase == OwnerPhase::Handed && state.phase != OwnerPhase::Handed
        {
            return Err(invalid(
                "original lifecycle state cannot change origin or regress",
            ));
        }
    } else if !initial {
        return Err(recovery_error(
            AllocationLifetimeRecoveryError::LiveCheckpoint,
        ));
    }
    let directory = anchor_directory(backend, true)?
        .ok_or_else(|| invalid("original lifecycle directory absent"))?;
    let plaintext = Zeroizing::new(
        serde_json::to_vec(state)
            .map_err(|source| codec_error("encode original lifecycle state", source))?,
    );
    let bytes =
        backend.encrypt_fallback_record_with_protection(&location(STATE), &plaintext, false)?;
    let prepared = directory
        .prepare_private(std::path::Path::new(STATE), &bytes)
        .map_err(|source| source_error("prepare original lifecycle state", source))?;
    #[cfg(test)]
    prelink_process_checkpoint(STATE)?;
    #[cfg(test)]
    if !initial && state.phase == OwnerPhase::Handed {
        prelink_process_checkpoint("mutable-lifecycle-handed")?;
    }
    if !prepared
        .publish(initial)
        .map_err(|source| source_error("publish original lifecycle state", source))?
    {
        return Err(invalid("original lifecycle state publication conflict"));
    }
    prepared
        .acknowledge()
        .map_err(|source| source_error("acknowledge original lifecycle state", source))?;
    Ok(aura_core::hash::hash(&bytes))
}
pub(super) fn acknowledge_handoff_state(
    owned: &ProfileOwnedSecureStorage,
    root: [u8; 32],
) -> Result<[u8; 32], AuraError> {
    let backend = selected(owned)?;
    let (mut state, digest) = read_state(backend)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveCheckpoint))?;
    if state.original.root != root {
        return Err(invalid("original handoff lifecycle root mismatch"));
    }
    let (birth, _) = read_anchor(backend, BIRTH)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
    let (complete, _) = read_anchor(backend, COMPLETE)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::ReadinessSeal))?;
    if birth != state.original || complete != state.original {
        return Err(invalid("handoff lifecycle differs from original anchors"));
    }
    if state.phase == OwnerPhase::Handed {
        anchor_directory(backend, false)?
            .ok_or_else(|| invalid("original lifecycle directory absent"))?
            .acknowledge_entries()
            .map_err(|source| source_error("acknowledge retained handoff lifecycle", source))?;
        return Ok(digest);
    }
    state.phase = OwnerPhase::Handed;
    publish_state(backend, &state, false)
}

/// Only this private original selected owner can authenticate and anchor birth.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn original_birth(
    owned: &ProfileOwnedSecureStorage,
    has_lifetime_metadata: bool,
) -> Result<RootSeal, AuraError> {
    let backend = selected(owned)?;
    let birth = read_anchor(backend, BIRTH)?;
    let complete = read_anchor(backend, COMPLETE)?;
    let state = read_state(backend)?;
    let original = if let Some((state, _)) = state {
        if let Some((birth, _)) = &birth {
            if birth != &state.original {
                return Err(invalid("original lifecycle birth binding mismatch"));
            }
        } else if state.phase == OwnerPhase::Handed || has_lifetime_metadata {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::OriginalBirth,
            ));
        }
        if let Some((complete, _)) = &complete {
            if complete != &state.original {
                return Err(invalid("original lifecycle completion binding mismatch"));
            }
        } else if state.phase == OwnerPhase::Handed {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::ReadinessSeal,
            ));
        }
        if state.phase == OwnerPhase::Handed && !has_lifetime_metadata {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::OriginalBirth,
            ));
        }
        state.original
    } else {
        // Once BIRTH exists, missing lifecycle evidence is never pre-live.
        if birth.is_some() || complete.is_some() || has_lifetime_metadata {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::LiveCheckpoint,
            ));
        }
        let inventory = authenticate_legacy_inventory(backend)?;
        let mut root = [0; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut root)
            .map_err(|source| {
                AuraError::crypto_with_source("owned migration root entropy", Arc::new(source))
            })?;
        let state = OwnerState {
            version: 1,
            original: Anchor {
                version: 1,
                root,
                authenticated_inventory: inventory,
            },
            phase: OwnerPhase::Preparing,
        };
        publish_state(backend, &state, true)?;
        state.original
    };
    if birth.is_none() {
        publish_anchor(backend, BIRTH, &original)?;
    }
    #[cfg(test)]
    init_fault(owned, "migration-birth-anchor")?;
    Ok(RootSeal {
        version: 1,
        root: original.root,
    })
}

pub(super) fn complete_handoff(
    owned: &ProfileOwnedSecureStorage,
    root: [u8; 32],
    already_handed: bool,
) -> Result<([u8; 32], [u8; 32]), AuraError> {
    let backend = selected(owned)?;
    let (state, _) = read_state(backend)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::LiveCheckpoint))?;
    let (birth, birth_digest) = read_anchor(backend, BIRTH)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
    if birth.root != root || state.original != birth {
        return Err(invalid("original migration root binding mismatch"));
    }
    let complete_digest = match read_anchor(backend, COMPLETE)? {
        Some((complete, digest)) if complete == birth => digest,
        Some(_) => return Err(invalid("original migration handoff binding mismatch")),
        None if already_handed || state.phase == OwnerPhase::Handed => {
            return Err(recovery_error(
                AllocationLifetimeRecoveryError::ReadinessSeal,
            ))
        }
        None => publish_anchor(backend, COMPLETE, &birth)?,
    };
    Ok((birth_digest, complete_digest))
}
pub(super) fn require_original_anchors(root: &FilesystemLifetimeRoot) -> Result<(), AuraError> {
    let directory = root
        .provider_directory
        .child(std::path::Path::new(NAMESPACE), false)
        .map_err(|source| source_error("required original migration anchor custody", source))?;
    for (name, expected) in [
        (BIRTH, root.migration_birth_digest),
        (COMPLETE, root.migration_handoff_digest),
        (STATE, root.migration_lifecycle_digest),
    ] {
        let bytes = directory
            .read_bounded(std::path::Path::new(name), true, MAX_LEGACY_BYTES)
            .map_err(|source| {
                source_error("required original migration anchor continuity", source)
            })?
            .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
        if aura_core::hash::hash(&bytes) != expected {
            return Err(invalid("original immutable migration anchor changed"));
        }
    }
    directory
        .acknowledge_entries()
        .map_err(|source| source_error("acknowledge original migration anchor continuity", source))
}
#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::secure::{SecureStorageCapability, SecureStorageEffects};
    #[tokio::test]
    async fn actual_cumulative_legacy_ciphertext_bound_preserves_original_profile(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let lease = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(
                profile.path().to_path_buf(),
            )
            .acquire_owned_native()?,
        );
        let ordinary =
            ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(lease)?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &ordinary else {
            panic!("real selected profile owner");
        };
        let backend = selected(owned)?;
        let original_key = backend
            .owned_directory()?
            .read(std::path::Path::new(FALLBACK_WRAPPING_KEY_FILENAME), true)?
            .ok_or("retained original key")?;
        let payload = vec![71; MAX_LEGACY_BYTES - 512];
        for index in 0..4 {
            ordinary
                .secure_store(
                    &SecureStorageLocation::new("legacy", format!("record-{index}")),
                    &payload,
                    &[SecureStorageCapability::Write],
                )
                .await?;
        }
        authenticate_legacy_inventory(backend)
            .expect("four real encrypted records fit cumulative limit");
        ordinary
            .secure_store(
                &SecureStorageLocation::new("legacy", "record-4"),
                &payload,
                &[SecureStorageCapability::Write],
            )
            .await?;
        let failure = match original_birth(owned, false) {
            Ok(_) => panic!("fifth real record exceeds cumulative required-read budget"),
            Err(error) => error,
        };
        assert!(matches!(
            std::error::Error::source(&failure)
                .and_then(|cause| cause.downcast_ref::<LegacyMigrationLimitExceeded>()),
            Some(LegacyMigrationLimitExceeded {
                kind: "bytes",
                limit: MAX_LEGACY_TOTAL_BYTES,
                ..
            })
        ));
        assert!(read_state(backend)?.is_none());
        assert!(read_anchor(backend, BIRTH)?.is_none());
        assert_eq!(
            backend
                .owned_directory()?
                .read(std::path::Path::new(FALLBACK_WRAPPING_KEY_FILENAME), true)?
                .ok_or("original key retained")?,
            original_key
        );
        for index in 0..5 {
            assert_eq!(
                ordinary
                    .secure_retrieve(
                        &SecureStorageLocation::new("legacy", format!("record-{index}")),
                        &[SecureStorageCapability::Read]
                    )
                    .await?,
                payload
            );
        }
        Ok(())
    }
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn require_historical_cutover_origin(
    owner: &ProfileOwnedSecureStorage,
    root: [u8; 32],
) -> Result<(), AuraError> {
    let backend = selected_initial_provider(owner)?;
    let birth = canonical_original_anchor(backend, BIRTH)?
        .ok_or_else(|| recovery_error(AllocationLifetimeRecoveryError::OriginalBirth))?;
    let state = canonical_original_state(backend)?;
    if birth.root != root || state.original != birth {
        return Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        ));
    }
    Ok(())
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn verify_historical_lifecycle_cutover(
    owner: &ProfileOwnedSecureStorage,
    old: &[u8],
    next: &[u8],
) -> Result<[u8; 32], AuraError> {
    let backend = selected_initial_provider(owner)?;
    let old = decode_original_state(backend, old)?;
    let next = decode_original_state(backend, next)?;
    if old.phase != OwnerPhase::Preparing
        || next.phase != OwnerPhase::Handed
        || old.original != next.original
    {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    require_historical_cutover_origin(owner, old.original.root)?;
    Ok(old.original.root)
}
