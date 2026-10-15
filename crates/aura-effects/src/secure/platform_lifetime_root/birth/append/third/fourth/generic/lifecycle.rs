//! Root-anchored transitions for tail allocations under the original roster lease.

use super::*;
use crate::secure::platform_lifetime_root::birth::decision::{DecisionRecord, NEGATIVE, POSITIVE};
use aura_core::effects::secret_lifetime::MAX_DECISION_BYTES;

const DECISION_PREFIX: &str = "allocation-lifetime-generic-decision:v13:";
const TOMBSTONE_PREFIX: &str = "allocation-lifetime-generic-tombstone:v13:";
const TOMBSTONE_MAGIC: &[u8] = b"AURA-PLATFORM-GENERIC-TOMBSTONE-V13";
const NONCE_DOMAIN: &[u8] = b"aura/platform-generic-tombstone-nonce/v13";

#[cfg(test)]
pub(super) fn cleanup_fixture(service: &str, id: &[u8; 32]) {
    for prefix in [DECISION_PREFIX, TOMBSTONE_PREFIX] {
        if let Ok(entry) = keyring::Entry::new(service, &user(prefix, id)) {
            let _ = entry.delete_credential();
        }
    }
}

pub(super) fn decision(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    entry: &Entry,
) -> Result<Option<DecisionRecord>, AuraError> {
    let observed = read_entry(selected, &user(DECISION_PREFIX, &entry.id))?;
    if entry.phase == LIVE {
        if observed.is_some()
            && !(root.generic_phase == GENERIC_DECISION_PENDING && root.generic_next_id == entry.id)
        {
            return Err(invalid("platform generic decision lacks root intent"));
        }
        return Ok(None);
    }
    let bytes = observed.ok_or_else(|| invalid("platform generic decision is missing"))?;
    if aura_core::hash::hash(&bytes) != entry.decision_digest {
        return Err(invalid("platform generic decision differs from roster"));
    }
    let record = DecisionRecord::decode(selected, &bytes)?;
    if record.root != root.id
        || record.allocation != entry.id
        || record.profile != root.profile
        || record.service != root.service
    {
        return Err(invalid("platform generic decision has foreign owner"));
    }
    Ok(Some(record))
}

fn tombstone(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    entry: &Entry,
) -> Result<Vec<u8>, AuraError> {
    let key = Zeroizing::new(selected.record_key(false)?);
    let mut nonce_input = Vec::with_capacity(NONCE_DOMAIN.len() + 96);
    nonce_input.extend_from_slice(NONCE_DOMAIN);
    nonce_input.extend_from_slice(&root.id);
    nonce_input.extend_from_slice(&entry.id);
    nonce_input.extend_from_slice(&entry.decision_digest);
    let nonce = aura_core::hash::hash(&nonce_input);
    let mut plain = Zeroizing::new(Vec::with_capacity(32 * 6 + entry.scope.len()));
    for field in [
        root.id,
        entry.id,
        entry.birth_digest,
        entry.decision_digest,
        root.profile,
        root.service,
    ] {
        plain.extend_from_slice(&field);
    }
    plain.extend_from_slice(&entry.scope);
    let ciphertext = ChaCha20Poly1305::new((&*key).into())
        .encrypt(
            Nonce::from_slice(&nonce[..12]),
            Payload {
                msg: &plain,
                aad: TOMBSTONE_MAGIC,
            },
        )
        .map_err(|source| {
            AuraError::crypto_with_source("platform generic tombstone encryption", Arc::new(source))
        })?;
    let mut encoded = Vec::with_capacity(TOMBSTONE_MAGIC.len() + 12 + ciphertext.len());
    encoded.extend_from_slice(TOMBSTONE_MAGIC);
    encoded.extend_from_slice(&nonce[..12]);
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

pub(super) fn validate_entry(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    entry: &Entry,
) -> Result<(), AuraError> {
    let pending_target = root.generic_next_id == entry.id;
    let erasing = pending_target && root.generic_phase == GENERIC_ERASURE_PENDING;
    let intent = read_entry(selected, &user(INTENT_PREFIX, &entry.id))?;
    let record = read_entry(selected, &user(RECORD_PREFIX, &entry.id))?;
    for bytes in [&intent, &record] {
        match bytes {
            Some(bytes) if aura_core::hash::hash(bytes) == entry.birth_digest => {
                let decoded = BirthIntent::decode(selected, bytes)?;
                if decoded.root != root.id
                    || decoded.allocation != entry.id
                    || decoded.scope != entry.scope
                {
                    return Err(invalid("platform generic original scope differs"));
                }
            }
            None if entry.phase == ERASED || erasing => {}
            _ => return Err(invalid("platform generic selected credential differs")),
        }
    }
    if entry.phase == ERASED && (intent.is_some() || record.is_some()) {
        return Err(invalid("platform generic erased credential reappeared"));
    }
    if !erasing && entry.phase != ERASED && (intent.is_none() || record.is_none()) {
        return Err(invalid("platform generic selected credential is missing"));
    }
    let decision = decision(selected, root, entry)?;
    let observed_tombstone = read_entry(selected, &user(TOMBSTONE_PREFIX, &entry.id))?;
    if entry.phase == LIVE {
        if observed_tombstone.is_some() {
            return Err(invalid("platform generic tombstone lacks decision"));
        }
    } else if entry.phase == DECIDED {
        if observed_tombstone.is_some()
            && !(pending_target && root.generic_phase == GENERIC_DENIAL_PENDING)
        {
            return Err(invalid("platform generic tombstone lacks root intent"));
        }
    } else {
        if decision
            .as_ref()
            .is_none_or(|record| record.kind != NEGATIVE)
        {
            return Err(invalid("platform generic denial lacks negative decision"));
        }
        let expected = tombstone(selected, root, entry)?;
        if aura_core::hash::hash(&expected) != entry.tombstone_digest
            || observed_tombstone.as_deref() != Some(expected.as_slice())
        {
            return Err(invalid("platform generic sealed denial differs"));
        }
    }
    if entry.phase == DECIDED
        && decision
            .as_ref()
            .is_none_or(|record| record.kind != POSITIVE && record.kind != NEGATIVE)
    {
        return Err(invalid("platform generic decision is invalid"));
    }
    Ok(())
}

fn entry<'a>(entries: &'a [Entry], id: &[u8; 32]) -> Result<&'a Entry, AuraError> {
    entries
        .iter()
        .find(|entry| &entry.id == id)
        .ok_or_else(|| invalid("platform generic allocation is not owned"))
}

fn target(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    current: &[Entry],
    next: &[Entry],
) -> Result<(Manifest, Vec<PagePublication>), AuraError> {
    let manifest = read_manifest(
        owned,
        root,
        root.generic_revision,
        root.generic_current_digest,
    )?;
    let (target, pages) = next_manifest(root, &manifest, current, next)?;
    if aura_core::hash::hash(&manifest_bytes(root, &target)?) != root.generic_target_digest {
        return Err(invalid(
            "platform generic target transition differs from root",
        ));
    }
    Ok((target, pages))
}

fn finish(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    current: &[Entry],
    next: &[Entry],
    stop: Stop,
) -> Result<(), AuraError> {
    let (target, pages) = target(owned, root, current, next)?;
    for (reference, entries) in &pages {
        publish_page(owned, root, reference, entries)?;
    }
    publish(
        owned,
        target.revision as usize,
        &manifest_bytes(root, &target)?,
    )?;
    if stop == Stop::Marker {
        return Err(interrupted());
    }
    root.generic_phase = GENERIC_ACTIVE;
    root.generic_count = target.count as u16;
    root.generic_revision = target.revision;
    root.generic_current_digest = root.generic_target_digest;
    root.generic_target_digest = [0; 32];
    root.generic_next_id = [0; 32];
    root.generic_next_intent_digest = [0; 32];
    write_keyring(selected, root)
}

pub(super) fn complete_pending(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    stop: Stop,
) -> Result<(), AuraError> {
    let fourth = original_four(owned, root)?.remove(0);
    let current = read_current(owned, selected, root, &fourth)?;
    let original = entry(&current, &root.generic_next_id)?.clone();
    let mut changed = original.clone();
    match root.generic_phase {
        GENERIC_DECISION_PENDING => {
            if original.phase != LIVE {
                return Err(invalid("platform generic decision has invalid source"));
            }
            let bytes = read_entry(selected, &user(DECISION_PREFIX, &original.id))?
                .ok_or_else(|| invalid("platform generic root claim lacks decision"))?;
            if aura_core::hash::hash(&bytes) != root.generic_next_intent_digest {
                return Err(invalid(
                    "platform generic decision differs from root intent",
                ));
            }
            let record = DecisionRecord::decode(selected, &bytes)?;
            if record.root != root.id
                || record.allocation != original.id
                || record.profile != root.profile
                || record.service != root.service
            {
                return Err(invalid("platform generic decision has foreign owner"));
            }
            changed.phase = DECIDED;
            changed.decision_digest = root.generic_next_intent_digest;
        }
        GENERIC_DENIAL_PENDING => {
            if original.phase != DECIDED
                || decision(selected, root, &original)?
                    .as_ref()
                    .is_none_or(|record| record.kind != NEGATIVE)
            {
                return Err(invalid("platform generic denial lacks negative decision"));
            }
            let bytes = tombstone(selected, root, &original)?;
            if aura_core::hash::hash(&bytes) != root.generic_next_intent_digest {
                return Err(invalid("platform generic denial differs from root intent"));
            }
            let user = user(TOMBSTONE_PREFIX, &original.id);
            match read_entry(selected, &user)? {
                Some(observed) if observed == bytes => {}
                None => write_exact(selected, &user, &bytes)?,
                _ => return Err(invalid("platform generic denial evidence differs")),
            }
            if stop == Stop::Record {
                return Err(interrupted());
            }
            changed.phase = DENIED;
            changed.tombstone_digest = root.generic_next_intent_digest;
        }
        GENERIC_ERASURE_PENDING => {
            if original.phase != DENIED || root.generic_next_intent_digest != original.birth_digest
            {
                return Err(invalid("platform generic erasure lacks original denial"));
            }
            for (index, prefix) in [RECORD_PREFIX, INTENT_PREFIX].iter().enumerate() {
                let user = user(prefix, &original.id);
                if read_entry(selected, &user)?.is_some() {
                    let entry = selected.entry_for_user(&user)?;
                    match entry.delete_credential() {
                        Ok(()) | Err(keyring::Error::NoEntry) => {}
                        Err(source) => {
                            return Err(PlatformSecureStorageHandler::map_keyring_error(source))
                        }
                    }
                    if read_entry(selected, &user)?.is_some() {
                        return Err(invalid("platform generic erasure readback failed"));
                    }
                }
                if stop == Stop::Record && index == 0 {
                    return Err(interrupted());
                }
                if stop == Stop::Intent && index == 1 {
                    return Err(interrupted());
                }
            }
            changed.phase = ERASED;
        }
        _ => return Err(invalid("platform generic transition phase is invalid")),
    }
    let mut next = current.clone();
    let position = next
        .binary_search_by_key(&original.id, |entry| entry.id)
        .map_err(|_| invalid("platform generic transition identity is absent"))?;
    next[position] = changed;
    finish(owned, selected, root, &current, &next, stop)
}

async fn start(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
    phase: u8,
    proof: [u8; 32],
    next_phase: u8,
    publication: Option<(&str, &[u8])>,
    stop: Stop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (mut root, current, _) = verify_owner(owned, selected, generic)?;
    let original = entry(&current, id)?;
    if root.generic_revision as usize >= MAX_GENERIC_REVISION {
        return Err(invalid("platform generic revision exhausted"));
    }
    if original.phase != phase {
        return Err(invalid("platform generic transition has invalid source"));
    }
    let mut next = current.clone();
    let position = next
        .binary_search_by_key(id, |entry| entry.id)
        .map_err(|_| invalid("platform generic transition identity is absent"))?;
    next[position].phase = next_phase;
    match next_phase {
        DECIDED => next[position].decision_digest = proof,
        DENIED => next[position].tombstone_digest = proof,
        ERASED => {}
        _ => return Err(invalid("platform generic transition target is invalid")),
    }
    let manifest = read_manifest(
        owned,
        &root,
        root.generic_revision,
        root.generic_current_digest,
    )?;
    let (target, _) = next_manifest(&root, &manifest, &current, &next)?;
    root.generic_phase = match next_phase {
        DECIDED => GENERIC_DECISION_PENDING,
        DENIED => GENERIC_DENIAL_PENDING,
        ERASED => GENERIC_ERASURE_PENDING,
        _ => unreachable!(),
    };
    root.generic_next_id = *id;
    root.generic_next_intent_digest = proof;
    root.generic_target_digest = aura_core::hash::hash(&manifest_bytes(&root, &target)?);
    write_keyring(selected, &root)?;
    if stop == Stop::Root {
        return Err(interrupted());
    }
    if let Some((prefix, bytes)) = publication {
        write_exact(selected, &user(prefix, id), bytes)?;
        if stop == Stop::Record {
            return Err(interrupted());
        }
    }
    complete_pending(owned, selected, &mut root, stop)
}

#[allow(dead_code)]
pub(super) async fn decide(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
    positive: bool,
    exact: &[u8],
) -> Result<(), AuraError> {
    decide_inner(owned, generic, id, positive, exact, Stop::Never).await
}

pub(super) async fn decide_inner(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
    positive: bool,
    exact: &[u8],
    stop: Stop,
) -> Result<(), AuraError> {
    if exact.is_empty() || exact.len() > MAX_DECISION_BYTES {
        return Err(invalid("platform generic decision size is invalid"));
    }
    let (selected, _) = selected_owner(owned)?;
    let (current_root, entries, _) = verify_owner(owned, selected, generic)?;
    let source = entry(&entries, id)?;
    if source.phase != LIVE {
        let existing = decision(selected, &current_root, source)?;
        if existing.as_ref().is_some_and(|record| {
            record.kind == if positive { POSITIVE } else { NEGATIVE }
                && record.bytes.as_slice() == exact
        }) {
            return Ok(());
        }
        return Err(invalid("platform generic decision contradicts original"));
    }
    let record = DecisionRecord {
        kind: if positive { POSITIVE } else { NEGATIVE },
        root: generic.root,
        allocation: *id,
        profile: current_root.profile,
        service: current_root.service,
        bytes: Zeroizing::new(exact.to_vec()),
    };
    let encoded = record.encode(selected)?;
    start(
        owned,
        generic,
        id,
        LIVE,
        aura_core::hash::hash(&encoded),
        DECIDED,
        Some((DECISION_PREFIX, &encoded)),
        stop,
    )
    .await
}

#[allow(dead_code)]
pub(super) async fn acknowledge_negative(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
    exact: &[u8],
) -> Result<(), AuraError> {
    deny_inner(owned, generic, id, exact, Stop::Never).await
}

pub(super) async fn deny_inner(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
    exact: &[u8],
    stop: Stop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let (root, current, _) = verify_owner(owned, selected, generic)?;
    let source = entry(&current, id)?;
    let record = decision(selected, &root, source)?
        .ok_or_else(|| invalid("platform generic denial lacks decision"))?;
    if record.kind != NEGATIVE || record.bytes.as_slice() != exact {
        return Err(invalid(
            "platform generic denial lacks exact negative decision",
        ));
    }
    if matches!(source.phase, DENIED | ERASED) {
        return Ok(());
    }
    let proof = aura_core::hash::hash(&tombstone(selected, &root, source)?);
    start(owned, generic, id, DECIDED, proof, DENIED, None, stop).await
}

#[allow(dead_code)]
pub(super) async fn erase(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
) -> Result<(), AuraError> {
    erase_inner(owned, generic, id, Stop::Never).await
}

pub(super) async fn erase_inner(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
    stop: Stop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let (_, current, _) = verify_owner(owned, selected, generic)?;
    let source = entry(&current, id)?;
    if source.phase == ERASED {
        return Ok(());
    }
    start(
        owned,
        generic,
        id,
        DENIED,
        source.birth_digest,
        ERASED,
        None,
        stop,
    )
    .await
}
