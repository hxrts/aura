//! Protected original initialization cutover evidence; no public constructor.
use super::*;

const JOURNAL_LIMIT: usize = 65536;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalCutoverJournal {
    version: u16,
    target: PathBuf,
    staged: String,
    original: Vec<u8>,
    next: Vec<u8>,
    original_identity: (u64, u64),
    next_identity: (u64, u64),
}
#[derive(Clone, Copy)]
pub(crate) struct CutoverIdentities {
    pub(crate) original: (u64, u64),
    pub(crate) next: (u64, u64),
}
fn encode_cutover<T: Serialize>(
    path: &str,
    key: &[u8; 32],
    value: &T,
) -> Result<Vec<u8>, AuraError> {
    let plaintext = encode_protected_json(
        value,
        JOURNAL_LIMIT - ROOT_MAGIC.len() - 28,
        "encode protected original cutover",
    )?;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|source| AuraError::crypto_with_source("cutover entropy", Arc::new(source)))?;
    let ciphertext = ChaCha20Poly1305::new(key.into())
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &[ROOT_MAGIC, path.as_bytes()].concat(),
            },
        )
        .map_err(|source| {
            AuraError::crypto_with_source("protect original cutover", Arc::new(source))
        })?;
    let mut bytes = Vec::with_capacity(ROOT_MAGIC.len() + 12 + ciphertext.len());
    bytes.extend_from_slice(ROOT_MAGIC);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&ciphertext);
    Ok(bytes)
}
pub(super) fn journal_path(target: &std::path::Path) -> Result<PathBuf, AuraError> {
    use std::os::unix::ffi::OsStrExt;
    let name = target
        .file_name()
        .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Phase))?;
    Ok(target
        .parent()
        .unwrap_or(std::path::Path::new(""))
        .join(format!(
            ".aura-initial-cutover-v1-{}",
            hex::encode(aura_core::hash::hash(name.as_bytes()))
        )))
}
fn read_journal(
    owner: &ProfileOwnedSecureStorage,
    target: &std::path::Path,
) -> Result<Option<OriginalCutoverJournal>, AuraError> {
    let backend = selected_initial_provider(owner)?;
    let directory = backend.owned_directory()?;
    let path = journal_path(target)?;
    let Some((bytes, staged)) = directory
        .read_private_publication(&path, JOURNAL_LIMIT)
        .map_err(|e| source_error("required initial cutover journal read", e))?
    else {
        return Ok(None);
    };
    let path_string = path
        .to_str()
        .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Phase))?;
    let journal: OriginalCutoverJournal = decode_root(path_string, &backend.wrapping_key, &bytes)?;
    if journal.version != 1
        || journal.target != target
        || journal.original.len() > MAX_INDEX_BYTES
        || journal.next.len() > MAX_INDEX_BYTES
    {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    // Only authenticate here. Initial proof factory must validate original phases
    // before ACKing a journal stage or using it to authorize cutover.
    let _ = staged;
    Ok(Some(journal))
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "VerifiedOriginalInitializationSuccessor",
    family = "runtime_helper"
)]
pub(super) fn retain_journal(
    witness: &VerifiedOriginalInitializationSuccessor<'_>,
    observation: &crate::profile_directory::ObservedOriginalSuccessor,
) -> Result<(), AuraError> {
    witness
        .require_publication(observation.publication())
        .map_err(|e| source_error("required cutover witness", e))?;
    let backend = selected_initial_provider(witness.owner)?;
    let directory = backend.owned_directory()?;
    if let Some(existing) = read_journal(witness.owner, &witness.target)? {
        if existing.original != observation.original()
            || existing.next != observation.next()
            || existing.staged != observation.publication().initial_stage_name()
        {
            return Err(transition_error(
                OriginalInitializationTransitionError::Seal,
            ));
        }
        let path = journal_path(&witness.target)?;
        if let Some((_, Some(publication))) = directory
            .read_private_publication(&path, JOURNAL_LIMIT)
            .map_err(|e| source_error("required cutover journal stage", e))?
        {
            publication
                .publish_original(JOURNAL_LIMIT)
                .map_err(|e| source_error("required proved cutover journal promotion", e))?;
        }
        return Ok(());
    }
    let (original_identity, next_identity) = observation
        .publication()
        .initial_cutover_identity(MAX_INDEX_BYTES)
        .map_err(|e| source_error("required cutover original inode", e))?;
    let journal = OriginalCutoverJournal {
        version: 1,
        target: witness.target.clone(),
        staged: observation.publication().initial_stage_name().into(),
        original: observation.original().to_vec(),
        next: observation.next().to_vec(),
        original_identity,
        next_identity,
    };
    let path = journal_path(&witness.target)?;
    let encoded = encode_cutover(
        path.to_str()
            .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Phase))?,
        &backend.wrapping_key,
        &journal,
    )?;
    if encoded.len() > JOURNAL_LIMIT {
        return Err(transition_error(
            OriginalInitializationTransitionError::Phase,
        ));
    }
    let publication = directory
        .prepare_private(&path, &encoded)
        .map_err(|e| source_error("required cutover journal preparation", e))?;
    #[cfg(test)]
    initialization_cutover_checkpoint("journal-stage")
        .map_err(|e| source_error("cutover stage test checkpoint", e))?;
    let (observed, proof) = directory
        .read_private_publication(&path, JOURNAL_LIMIT)
        .map_err(|e| source_error("required exact cutover journal observation", e))?
        .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Seal))?;
    if observed != encoded {
        return Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        ));
    }
    let proof =
        proof.ok_or_else(|| transition_error(OriginalInitializationTransitionError::Seal))?;
    proof
        .publish_original(JOURNAL_LIMIT)
        .map_err(|e| source_error("required original cutover journal publication", e))?;
    drop(publication);
    #[cfg(test)]
    initialization_cutover_checkpoint("journal")
        .map_err(|e| source_error("cutover test checkpoint", e))?;
    Ok(())
}
pub(super) fn require_witness_journal(
    witness: &VerifiedOriginalInitializationSuccessor<'_>,
) -> Result<CutoverIdentities, AuraError> {
    let backend = selected_initial_provider(witness.owner)?;
    let directory = backend.owned_directory()?;
    if directory
        .read_canonical_private_publication(&journal_path(&witness.target)?, JOURNAL_LIMIT)
        .map_err(|e| source_error("required canonical cutover journal", e))?
        .is_none()
    {
        return Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        ));
    }
    let journal = read_journal(witness.owner, &witness.target)?
        .ok_or_else(|| transition_error(OriginalInitializationTransitionError::Seal))?;
    if aura_core::hash::hash(&journal.original) != witness.original_digest
        || aura_core::hash::hash(&journal.next) != witness.next_digest
        || journal.staged != witness.publication.initial_stage_name()
    {
        return Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        ));
    }
    // A borrowed witness cannot authorize a later lifecycle. Recheck the full
    // independent original phase proof under the same actual selected owner.
    let observation = directory
        .observe_retained_initial_successor(
            &witness.target,
            journal.original.clone(),
            journal.next.clone(),
            journal.staged.clone(),
        )
        .map_err(|e| source_error("required current original cutover binding", e))?;
    let current = if witness.target == std::path::Path::new(INDEX) {
        verify_original_index_successor(witness.owner, &observation)?
    } else {
        legacy_migration::verify_original_lifecycle_successor(witness.owner, &observation)?
    };
    if current.original_digest != witness.original_digest
        || current.next_digest != witness.next_digest
    {
        return Err(transition_error(
            OriginalInitializationTransitionError::Seal,
        ));
    }
    Ok(CutoverIdentities {
        original: journal.original_identity,
        next: journal.next_identity,
    })
}
pub(super) fn recover_retained(owner: &ProfileOwnedSecureStorage) -> Result<(), AuraError> {
    let backend = selected_initial_provider(owner)?;
    let directory = backend.owned_directory()?;
    for target in [
        PathBuf::from(INDEX),
        PathBuf::from(".aura-allocation-lifetime-owner-v1").join("lifecycle"),
    ] {
        let Some(journal) = read_journal(owner, &target)? else {
            continue;
        };
        let observation = directory
            .observe_retained_initial_successor(
                &target,
                journal.original,
                journal.next,
                journal.staged,
            )
            .map_err(|e| source_error("required retained cutover observation", e))?;
        let witness = if target == std::path::Path::new(INDEX) {
            verify_original_index_successor(owner, &observation)?
        } else {
            legacy_migration::verify_original_lifecycle_successor(owner, &observation)?
        };
        retain_journal(&witness, &observation)?;
        observation
            .publication()
            .publish_verified_initial_successor(&witness, MAX_INDEX_BYTES)
            .map_err(|e| source_error("required retained initial cutover", e))?;
        finish_journal(&witness)?;
    }
    Ok(())
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "VerifiedOriginalInitializationSuccessor",
    family = "runtime_helper"
)]
pub(super) fn finish_journal(
    witness: &VerifiedOriginalInitializationSuccessor<'_>,
) -> Result<(), AuraError> {
    witness
        .publication
        .archive_initial_cutover_journal(witness)
        .map_err(|e| source_error("required original cutover archive ACK", e))
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileOwnedSecureStorage",
    family = "runtime_helper"
)]
pub(super) fn validate_archived(owner: &ProfileOwnedSecureStorage) -> Result<(), AuraError> {
    let backend = selected_initial_provider(owner)?;
    let directory = backend.owned_directory()?;
    for target in [
        PathBuf::from(INDEX),
        PathBuf::from(".aura-allocation-lifetime-owner-v1").join("lifecycle"),
    ] {
        let path = journal_path(&target)?;
        let mut authenticated_stages = Vec::new();
        if let Some(active) = read_journal(owner, &target)? {
            authenticated_stages.push(active.staged);
        }
        let mut domain_error = None;
        let native =
            directory.visit_initial_cutover_history(&target, JOURNAL_LIMIT, |bytes, staged| {
                let validation = (|| {
                    let journal: OriginalCutoverJournal = decode_root(
                        path.to_str().ok_or_else(|| {
                            transition_error(OriginalInitializationTransitionError::Phase)
                        })?,
                        &backend.wrapping_key,
                        &bytes,
                    )?;
                    if journal.version != 1 || journal.target != target || journal.staged != staged
                    {
                        return Err(transition_error(
                            OriginalInitializationTransitionError::Seal,
                        ));
                    }
                    directory
                        .require_archived_initial_custody(
                            &target,
                            &journal.staged,
                            &journal.original,
                            &journal.next,
                            CutoverIdentities {
                                original: journal.original_identity,
                                next: journal.next_identity,
                            },
                            MAX_INDEX_BYTES,
                        )
                        .map_err(|source| {
                            source_error("required authenticated archived inode custody", source)
                        })?;
                    authenticated_stages.push(journal.staged.clone());
                    let root = if target == std::path::Path::new(INDEX) {
                        let old: Index =
                            decode_root(INDEX, &backend.wrapping_key, &journal.original)?;
                        let next: Index = decode_root(INDEX, &backend.wrapping_key, &journal.next)?;
                        require_empty_initial_index(&old, old.root)?;
                        require_empty_initial_index(&next, old.root)?;
                        if !matches!(
                            (&old.phase, &next.phase),
                            (Phase::Preparing, Phase::Ready) | (Phase::Ready, Phase::Handed)
                        ) {
                            return Err(transition_error(
                                OriginalInitializationTransitionError::Phase,
                            ));
                        }
                        old.root
                    } else {
                        legacy_migration::verify_historical_lifecycle_cutover(
                            owner,
                            &journal.original,
                            &journal.next,
                        )?
                    };
                    legacy_migration::require_historical_cutover_origin(owner, root)?;
                    let born: RootSeal =
                        read(directory, BORN, &backend.wrapping_key)?.ok_or_else(|| {
                            recovery_error(AllocationLifetimeRecoveryError::OriginalBirth)
                        })?;
                    require_seal(&born, root)
                })();
                match validation {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        domain_error = Some(error);
                        Err(std::io::Error::other("private cutover validation stopped"))
                    }
                }
            });
        if let Some(error) = domain_error {
            return Err(error);
        }
        native
            .map_err(|error| source_error("required original cutover history inventory", error))?;
        directory
            .require_initial_custody_origins(&target, &authenticated_stages)
            .map_err(|source| {
                source_error("required authenticated initial custody origin", source)
            })?;
    }
    Ok(())
}
