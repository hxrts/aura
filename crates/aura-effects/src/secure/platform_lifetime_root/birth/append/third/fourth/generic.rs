//! Private V12 count-bounded tail roster. The selected root authenticates each
//! immutable generation before a new per-ID credential can be written.
//! Generic decision/denial/erasure mutation remains closed.

use super::*;

const MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-GENERIC-PAGE-V12";
const MANIFEST_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-GENERIC-MANIFEST-V12";
const HEADER: usize = 32 * 3 + 2;
const ENTRY_HEADER: usize = 32 * 4 + 1 + 2;
const MAX_COUNT: usize = aura_core::effects::secret_lifetime::MAX_PROFILE_ALLOCATION_COUNT;
const PAGE_SIZE: usize = 64;
const MAX_FILE: usize = MAGIC.len() + HEADER + PAGE_SIZE * (ENTRY_HEADER + MAX_SCOPE_BYTES);
const REF_LEN: usize = 32 + 32 + 2 + 2;
const MAX_PAGES: usize = 2 * MAX_COUNT.div_ceil(PAGE_SIZE);
const MAX_MANIFEST: usize = MANIFEST_MAGIC.len() + HEADER + 2 + MAX_PAGES * REF_LEN;
const LIVE: u8 = 0;
const DECIDED: u8 = 1;
const DENIED: u8 = 2;
const ERASED: u8 = 3;

#[cfg(test)]
pub(super) fn cleanup_fixture(owned: &ProfileOwnedSecureStorage, root: &RootRecord, service: &str) {
    let mut ids = Vec::new();
    if root.generic_next_id != [0; 32] {
        ids.push(root.generic_next_id);
    }
    for count in 4..=(root.generic_count as usize + 1).max(4) {
        if let Ok(Some((bytes, None))) = owned
            ._owner
            .directory
            .read_private_publication(Path::new(&path(count)), MAX_MANIFEST)
        {
            if let Ok(manifest) = decode_manifest(root, &bytes) {
                if let Ok(entries) = read_all(owned, root, &manifest) {
                    ids.extend(
                        entries
                            .into_iter()
                            .filter(|entry| entry.id != root.tail_next_id)
                            .map(|entry| entry.id),
                    );
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    for id in ids {
        for name in [user(INTENT_PREFIX, &id), user(RECORD_PREFIX, &id)] {
            if let Ok(entry) = keyring::Entry::new(service, &name) {
                let _ = entry.delete_credential();
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    id: [u8; 32],
    birth_digest: [u8; 32],
    phase: u8,
    decision_digest: [u8; 32],
    tombstone_digest: [u8; 32],
    scope: Vec<u8>,
}

fn path(count: usize) -> String {
    format!("platform_lifetime/generic-manifest-{count}.v12")
}

fn page_path(generation: u16, slot: u16) -> String {
    format!("platform_lifetime/generic-page-{generation}-{slot}.v12")
}

fn capacity(count: usize) -> Result<(), AuraError> {
    if !(4..MAX_COUNT).contains(&count) {
        return Err(invalid("platform generic roster capacity exhausted"));
    }
    Ok(())
}

fn encode(root: &RootRecord, entries: &[Entry]) -> Result<Vec<u8>, AuraError> {
    if !(1..=PAGE_SIZE).contains(&entries.len()) {
        return Err(invalid("platform generic page count is invalid"));
    }
    let mut bytes = Vec::with_capacity(MAGIC.len() + HEADER + entries.len() * (ENTRY_HEADER + 16));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&root.id);
    bytes.extend_from_slice(&root.profile);
    bytes.extend_from_slice(&root.service);
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut previous = None;
    for entry in entries {
        if entry.id == [0; 32]
            || entry.birth_digest == [0; 32]
            || entry.scope.is_empty()
            || entry.scope.len() > MAX_SCOPE_BYTES
            || previous.is_some_and(|id| id >= entry.id)
            || !matches!(entry.phase, LIVE | DECIDED | DENIED | ERASED)
            || (entry.phase == LIVE
                && (entry.decision_digest != [0; 32] || entry.tombstone_digest != [0; 32]))
            || (entry.phase == DECIDED
                && (entry.decision_digest == [0; 32] || entry.tombstone_digest != [0; 32]))
            || (entry.phase >= DENIED
                && (entry.decision_digest == [0; 32] || entry.tombstone_digest == [0; 32]))
        {
            return Err(invalid("platform generic roster entry is invalid"));
        }
        bytes.extend_from_slice(&entry.id);
        bytes.extend_from_slice(&entry.birth_digest);
        bytes.push(entry.phase);
        bytes.extend_from_slice(&entry.decision_digest);
        bytes.extend_from_slice(&entry.tombstone_digest);
        bytes.extend_from_slice(&(entry.scope.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&entry.scope);
        previous = Some(entry.id);
    }
    if bytes.len() > MAX_FILE {
        return Err(invalid("platform generic roster exceeds byte bound"));
    }
    Ok(bytes)
}

fn decode(root: &RootRecord, bytes: &[u8]) -> Result<Vec<Entry>, AuraError> {
    if bytes.len() < MAGIC.len() + HEADER || bytes.len() > MAX_FILE || !bytes.starts_with(MAGIC) {
        return Err(invalid(
            "platform generic roster version or length is invalid",
        ));
    }
    let base = MAGIC.len();
    if bytes[base..base + 32] != root.id
        || bytes[base + 32..base + 64] != root.profile
        || bytes[base + 64..base + 96] != root.service
    {
        return Err(invalid("platform generic roster has foreign owner"));
    }
    let count = u16::from_le_bytes([bytes[base + 96], bytes[base + 97]]) as usize;
    if !(1..=PAGE_SIZE).contains(&count) {
        return Err(invalid("platform generic page count is invalid"));
    }
    let mut cursor = base + HEADER;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let header = cursor
            .checked_add(ENTRY_HEADER)
            .ok_or_else(|| invalid("platform generic roster length overflow"))?;
        if header > bytes.len() {
            return Err(invalid("platform generic roster entry truncated"));
        }
        let scope_len = u16::from_le_bytes([bytes[cursor + 129], bytes[cursor + 130]]) as usize;
        let end = header
            .checked_add(scope_len)
            .ok_or_else(|| invalid("platform generic roster scope overflow"))?;
        if end > bytes.len() {
            return Err(invalid("platform generic roster scope truncated"));
        }
        entries.push(Entry {
            id: fixed_bytes(&bytes[cursor..cursor + 32])?,
            birth_digest: fixed_bytes(&bytes[cursor + 32..cursor + 64])?,
            phase: bytes[cursor + 64],
            decision_digest: fixed_bytes(&bytes[cursor + 65..cursor + 97])?,
            tombstone_digest: fixed_bytes(&bytes[cursor + 97..cursor + 129])?,
            scope: bytes[header..end].to_vec(),
        });
        cursor = end;
    }
    if cursor != bytes.len() || encode(root, &entries)? != bytes {
        return Err(invalid("platform generic roster is noncanonical"));
    }
    Ok(entries)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PageRef {
    max_id: [u8; 32],
    digest: [u8; 32],
    generation: u16,
    slot: u16,
}

#[derive(Clone)]
struct Manifest {
    count: usize,
    pages: Vec<PageRef>,
}

type PagePublication = (PageRef, Vec<Entry>);

fn manifest_bytes(root: &RootRecord, manifest: &Manifest) -> Result<Vec<u8>, AuraError> {
    if !(4..=MAX_COUNT).contains(&manifest.count)
        || manifest.pages.is_empty()
        || manifest.pages.len() > MAX_PAGES
    {
        return Err(invalid("platform generic manifest count is invalid"));
    }
    let mut bytes =
        Vec::with_capacity(MANIFEST_MAGIC.len() + HEADER + 2 + manifest.pages.len() * REF_LEN);
    bytes.extend_from_slice(MANIFEST_MAGIC);
    bytes.extend_from_slice(&root.id);
    bytes.extend_from_slice(&root.profile);
    bytes.extend_from_slice(&root.service);
    bytes.extend_from_slice(&(manifest.count as u16).to_le_bytes());
    bytes.extend_from_slice(&(manifest.pages.len() as u16).to_le_bytes());
    let mut prior = None;
    for page in &manifest.pages {
        if page.digest == [0; 32]
            || page.generation < 4
            || page.generation as usize > manifest.count
            || prior.is_some_and(|id| id >= page.max_id)
        {
            return Err(invalid(
                "platform generic manifest page reference is invalid",
            ));
        }
        bytes.extend_from_slice(&page.max_id);
        bytes.extend_from_slice(&page.digest);
        bytes.extend_from_slice(&page.generation.to_le_bytes());
        bytes.extend_from_slice(&page.slot.to_le_bytes());
        prior = Some(page.max_id);
    }
    Ok(bytes)
}

fn decode_manifest(root: &RootRecord, bytes: &[u8]) -> Result<Manifest, AuraError> {
    let base = MANIFEST_MAGIC.len();
    if !bytes.starts_with(MANIFEST_MAGIC)
        || bytes.len() < base + HEADER + 2
        || bytes.len() > MAX_MANIFEST
        || bytes[base..base + 32] != root.id
        || bytes[base + 32..base + 64] != root.profile
        || bytes[base + 64..base + 96] != root.service
    {
        return Err(invalid(
            "platform generic manifest version or owner differs",
        ));
    }
    let count = u16::from_le_bytes([bytes[base + 96], bytes[base + 97]]) as usize;
    let pages_len = u16::from_le_bytes([bytes[base + 98], bytes[base + 99]]) as usize;
    if bytes.len() != base + HEADER + 2 + pages_len * REF_LEN {
        return Err(invalid("platform generic manifest length differs"));
    }
    let mut pages = Vec::with_capacity(pages_len);
    let mut cursor = base + HEADER + 2;
    for _ in 0..pages_len {
        pages.push(PageRef {
            max_id: fixed_bytes(&bytes[cursor..cursor + 32])?,
            digest: fixed_bytes(&bytes[cursor + 32..cursor + 64])?,
            generation: u16::from_le_bytes([bytes[cursor + 64], bytes[cursor + 65]]),
            slot: u16::from_le_bytes([bytes[cursor + 66], bytes[cursor + 67]]),
        });
        cursor += REF_LEN;
    }
    let manifest = Manifest { count, pages };
    if manifest_bytes(root, &manifest)? != bytes {
        return Err(invalid("platform generic manifest is noncanonical"));
    }
    Ok(manifest)
}

fn marker(
    owned: &ProfileOwnedSecureStorage,
    count: usize,
    expected: &[u8],
) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(&path(count)), MAX_MANIFEST)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == expected => Ok(true),
        _ => Err(invalid("platform generic roster marker differs")),
    }
}

fn publish(
    owned: &ProfileOwnedSecureStorage,
    count: usize,
    expected: &[u8],
) -> Result<(), AuraError> {
    if marker(owned, count, expected)? {
        return Ok(());
    }
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(&path(count)), expected)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform generic roster publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

fn read_page(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    reference: &PageRef,
) -> Result<Vec<Entry>, AuraError> {
    let (bytes, None) = owned
        ._owner
        .directory
        .read_private_publication(
            Path::new(&page_path(reference.generation, reference.slot)),
            MAX_FILE,
        )
        .map_err(io)?
        .ok_or_else(|| invalid("platform generic page is missing"))?
    else {
        return Err(invalid("platform generic page is incomplete"));
    };
    if aura_core::hash::hash(&bytes) != reference.digest {
        return Err(invalid("platform generic page differs from manifest"));
    }
    let entries = decode(root, &bytes)?;
    if entries.last().map(|entry| entry.id) != Some(reference.max_id) {
        return Err(invalid("platform generic page range differs"));
    }
    Ok(entries)
}

fn publish_page(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    reference: &PageRef,
    entries: &[Entry],
) -> Result<(), AuraError> {
    let bytes = encode(root, entries)?;
    if aura_core::hash::hash(&bytes) != reference.digest
        || entries.last().map(|entry| entry.id) != Some(reference.max_id)
    {
        return Err(invalid("platform generic target page differs"));
    }
    let path = page_path(reference.generation, reference.slot);
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(&path), MAX_FILE)
        .map_err(io)?
    {
        Some((observed, None)) if observed == bytes => return Ok(()),
        Some(_) => return Err(invalid("platform generic page publication conflicted")),
        None => {}
    }
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(&path), &bytes)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform generic page publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

fn read_manifest(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    count: usize,
    digest: [u8; 32],
) -> Result<Manifest, AuraError> {
    let (bytes, None) = owned
        ._owner
        .directory
        .read_private_publication(Path::new(&path(count)), MAX_MANIFEST)
        .map_err(io)?
        .ok_or_else(|| invalid("platform generic manifest is missing"))?
    else {
        return Err(invalid("platform generic manifest is incomplete"));
    };
    if aura_core::hash::hash(&bytes) != digest {
        return Err(invalid("platform generic manifest differs from root"));
    }
    let manifest = decode_manifest(root, &bytes)?;
    if manifest.count != count {
        return Err(invalid("platform generic manifest count differs"));
    }
    Ok(manifest)
}

fn read_all(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    manifest: &Manifest,
) -> Result<Vec<Entry>, AuraError> {
    let mut all = Vec::with_capacity(manifest.count - 3);
    let mut prior = None;
    let mut addresses = std::collections::HashSet::new();
    for reference in &manifest.pages {
        if !addresses.insert((reference.generation, reference.slot)) {
            return Err(invalid("platform generic duplicate page address"));
        }
        let page = read_page(owned, root, reference)?;
        if prior.is_some_and(|id| id >= page[0].id) {
            return Err(invalid("platform generic page ranges overlap"));
        }
        prior = Some(reference.max_id);
        all.extend(page);
    }
    if all.len() + 3 != manifest.count {
        return Err(invalid("platform generic manifest entry count differs"));
    }
    Ok(all)
}

fn next_manifest(
    root: &RootRecord,
    current: &Manifest,
    old: &[Entry],
    next: &[Entry],
) -> Result<(Manifest, Vec<PagePublication>), AuraError> {
    if next.len() != old.len() + 1 || current.count != old.len() + 3 {
        return Err(invalid("platform generic append count differs"));
    }
    let id = next
        .iter()
        .find(|entry| {
            old.binary_search_by_key(&entry.id, |prior| prior.id)
                .is_err()
        })
        .ok_or_else(|| invalid("platform generic append identity is absent"))?
        .id;
    let index = current
        .pages
        .iter()
        .position(|page| id <= page.max_id)
        .unwrap_or(current.pages.len() - 1);
    let mut start = 0;
    for page in &current.pages[..index] {
        let end = old
            .binary_search_by_key(&page.max_id, |entry| entry.id)
            .map_err(|_| invalid("platform generic page boundary is absent"))?
            + 1;
        start = end;
    }
    let end = old
        .binary_search_by_key(&current.pages[index].max_id, |entry| entry.id)
        .map_err(|_| invalid("platform generic changed page boundary is absent"))?
        + 1;
    let mut changed = old[start..end].to_vec();
    let inserted = next
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| invalid("platform generic new entry is missing"))?
        .clone();
    changed.push(inserted);
    changed.sort_by_key(|entry| entry.id);
    let groups: Vec<Vec<Entry>> = if changed.len() <= PAGE_SIZE {
        vec![changed]
    } else {
        vec![
            changed[..changed.len() / 2].to_vec(),
            changed[changed.len() / 2..].to_vec(),
        ]
    };
    let generation = (current.count + 1) as u16;
    let mut pages = current.pages.clone();
    let mut publications = Vec::new();
    for (slot, group) in groups.into_iter().enumerate() {
        let reference = PageRef {
            max_id: group
                .last()
                .ok_or_else(|| invalid("platform generic changed page is empty"))?
                .id,
            digest: aura_core::hash::hash(&encode(root, &group)?),
            generation,
            slot: slot as u16,
        };
        publications.push((reference, group));
    }
    pages.splice(
        index..=index,
        publications.iter().map(|(reference, _)| reference.clone()),
    );
    let manifest = Manifest {
        count: current.count + 1,
        pages,
    };
    manifest_bytes(root, &manifest)?;
    Ok((manifest, publications))
}

fn original_four(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
) -> Result<Vec<Entry>, AuraError> {
    let (bytes, None) = owned
        ._owner
        .directory
        .read_private_publication(Path::new(FOURTH_ROSTER_FILE), MAX_V10_ROSTER_FILE)
        .map_err(io)?
        .ok_or_else(|| invalid("platform original fourth roster is missing"))?
    else {
        return Err(invalid("platform original fourth roster is incomplete"));
    };
    if aura_core::hash::hash(&bytes) != root.tail_roster_digest {
        return Err(invalid("platform original fourth roster differs"));
    }
    let parts = entries(&bytes, ROSTER_MAGIC_V10, root)?;
    if parts.len() != 4 {
        return Err(invalid("platform original fourth roster count differs"));
    }
    let part = parts
        .into_iter()
        .find(|part| part[32..64] == root.tail_next_id)
        .ok_or_else(|| invalid("platform original fourth identity is missing"))?;
    let phase = match root.tail_phase {
        TAIL_APPEND_COMPLETE => LIVE,
        TAIL_DECISION_COMPLETE => DECIDED,
        TAIL_ERASURE_COMPLETE => ERASED,
        _ => return Err(invalid("platform original fourth transition is unsettled")),
    };
    Ok(vec![Entry {
        id: root.tail_next_id,
        birth_digest: root.tail_next_intent_digest,
        phase,
        decision_digest: root.tail_decision_digest,
        tombstone_digest: root.tail_tombstone_digest,
        scope: part[ROSTER_ENTRY_HEADER_LEN..].to_vec(),
    }])
}

fn validate_four(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(Entry, [u8; 32]), AuraError> {
    let (claim, scope) = prior(owned, selected, root)?;
    replay(owned, selected, root, &claim, &scope, StopAfter::Never)?;
    lifecycle::recover(owned, selected, root)?;
    let fourth = original_four(owned, root)?.remove(0);
    if fourth.phase == DECIDED {
        let decision = lifecycle::decision_record(selected, root)?
            .ok_or_else(|| invalid("platform fourth positive decision is missing"))?;
        if decision.kind != super::super::super::super::decision::POSITIVE {
            return Err(invalid("platform fourth negative decision is not terminal"));
        }
    }
    Ok((fourth, claim.allocation))
}

fn read_current(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    fourth: &Entry,
) -> Result<Vec<Entry>, AuraError> {
    let count = root.generic_count as usize;
    if !(4..=MAX_COUNT).contains(&count) {
        return Err(invalid("platform generic root count is invalid"));
    }
    let manifest = read_manifest(owned, root, count, root.generic_current_digest)?;
    let entries = read_all(owned, root, &manifest)?;
    if entries.len() != count - 3
        || entries.iter().find(|entry| entry.id == fourth.id) != Some(fourth)
    {
        return Err(invalid(
            "platform generic roster lost original fourth proof",
        ));
    }
    for entry in entries.iter().filter(|entry| entry.id != fourth.id) {
        if entry.phase != LIVE
            || entry.decision_digest != [0; 32]
            || entry.tombstone_digest != [0; 32]
        {
            return Err(invalid(
                "unsupported generic transition cannot authorize recovery",
            ));
        }
        let intent = read_entry(selected, &user(INTENT_PREFIX, &entry.id))?
            .ok_or_else(|| invalid("platform generic sealed intent is missing"))?;
        if aura_core::hash::hash(&intent) != entry.birth_digest
            || read_entry(selected, &user(RECORD_PREFIX, &entry.id))?.as_deref()
                != Some(intent.as_slice())
        {
            return Err(invalid("platform generic selected credential differs"));
        }
        let decoded = BirthIntent::decode(selected, &intent)?;
        if decoded.root != root.id || decoded.allocation != entry.id || decoded.scope != entry.scope
        {
            return Err(invalid("platform generic original scope differs"));
        }
    }
    if root.generic_phase == GENERIC_ACTIVE
        && count < MAX_COUNT
        && owned
            ._owner
            .directory
            .read_private_publication(Path::new(&path(count + 1)), MAX_MANIFEST)
            .map_err(io)?
            .is_some()
    {
        return Err(invalid(
            "platform generic roster has future generation after root rollback",
        ));
    }
    Ok(entries)
}

fn candidate(
    root: &RootRecord,
    second_id: [u8; 32],
    current: &[Entry],
    intent: &BirthIntent,
    digest: [u8; 32],
) -> Result<Vec<Entry>, AuraError> {
    capacity(current.len() + 3)?;
    if intent.root != root.id
        || intent.allocation == [0; 32]
        || intent.allocation == root.birth_id
        || intent.allocation == second_id
        || intent.allocation == root.third_id
        || current.iter().any(|entry| entry.id == intent.allocation)
    {
        return Err(invalid(
            "platform generic next allocation is foreign or duplicate",
        ));
    }
    let mut next = current.to_vec();
    next.push(Entry {
        id: intent.allocation,
        birth_digest: digest,
        phase: LIVE,
        decision_digest: [0; 32],
        tombstone_digest: [0; 32],
        scope: intent.scope.clone(),
    });
    next.sort_by_key(|entry| entry.id);
    if next.len() > MAX_COUNT - 3 {
        return Err(invalid("platform generic roster capacity exhausted"));
    }
    Ok(next)
}

fn complete_append(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    second_id: [u8; 32],
    current: &[Entry],
    stop: Stop,
) -> Result<(), AuraError> {
    let id = root.generic_next_id;
    let intent_user = user(INTENT_PREFIX, &id);
    let record_user = user(RECORD_PREFIX, &id);
    let bytes = read_entry(selected, &intent_user)?
        .ok_or_else(|| invalid("platform generic root claim lacks sealed intent"))?;
    if aura_core::hash::hash(&bytes) != root.generic_next_intent_digest {
        return Err(invalid("platform generic sealed intent differs from root"));
    }
    let intent = BirthIntent::decode(selected, &bytes)?;
    if intent.allocation != id {
        return Err(invalid("platform generic intent has foreign allocation"));
    }
    let next = candidate(
        root,
        second_id,
        current,
        &intent,
        root.generic_next_intent_digest,
    )?;
    let manifest = read_manifest(
        owned,
        root,
        root.generic_count as usize,
        root.generic_current_digest,
    )?;
    let (target, changed_pages) = next_manifest(root, &manifest, current, &next)?;
    let roster = manifest_bytes(root, &target)?;
    if aura_core::hash::hash(&roster) != root.generic_target_digest {
        return Err(invalid(
            "platform generic target manifest differs from root",
        ));
    }
    match read_entry(selected, &record_user)? {
        Some(observed) if observed == bytes => {}
        Some(_) => return Err(invalid("platform generic selected record differs")),
        None if marker(owned, next.len() + 3, &roster)? => {
            return Err(invalid("platform generic record lost after ACK"))
        }
        None => write_exact(selected, &record_user, &bytes)?,
    }
    if stop == Stop::Record {
        return Err(interrupted());
    }
    for (reference, entries) in &changed_pages {
        publish_page(owned, root, reference, entries)?;
    }
    publish(owned, next.len() + 3, &roster)?;
    if stop == Stop::Marker {
        return Err(interrupted());
    }
    root.generic_phase = GENERIC_ACTIVE;
    root.generic_count = (next.len() + 3) as u16;
    root.generic_current_digest = root.generic_target_digest;
    root.generic_target_digest = [0; 32];
    root.generic_next_id = [0; 32];
    root.generic_next_intent_digest = [0; 32];
    write_keyring(selected, root)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Never,
    Root,
    Intent,
    Record,
    Marker,
}

fn reconcile_locked(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    stop: Stop,
) -> Result<Vec<Entry>, AuraError> {
    let (fourth, second_id) = validate_four(owned, selected, root)?;
    if root.generic_phase == GENERIC_INIT_PENDING {
        let initial = vec![fourth.clone()];
        let reference = PageRef {
            max_id: fourth.id,
            digest: aura_core::hash::hash(&encode(root, &initial)?),
            generation: 4,
            slot: 0,
        };
        let bytes = manifest_bytes(
            root,
            &Manifest {
                count: 4,
                pages: vec![reference.clone()],
            },
        )?;
        if aura_core::hash::hash(&bytes) != root.generic_target_digest {
            return Err(invalid("platform generic initial roster differs from root"));
        }
        publish_page(owned, root, &reference, &initial)?;
        publish(owned, 4, &bytes)?;
        if stop == Stop::Marker {
            return Err(interrupted());
        }
        root.generic_phase = GENERIC_ACTIVE;
        root.generic_count = 4;
        root.generic_current_digest = root.generic_target_digest;
        root.generic_target_digest = [0; 32];
        write_keyring(selected, root)?;
    }
    if root.generic_phase == GENERIC_ACTIVE {
        return read_current(owned, selected, root, &fourth);
    }
    if root.generic_phase == GENERIC_APPEND_PENDING {
        let current = read_current(owned, selected, root, &fourth)?;
        if stop == Stop::Intent {
            return Err(interrupted());
        }
        complete_append(owned, selected, root, second_id, &current, stop)?;
        return read_current(owned, selected, root, &fourth);
    }
    Err(invalid(
        "platform generic roster has no original root claim",
    ))
}

/// One private original profile owner after the authenticated four-entry handoff.
#[allow(dead_code)]
pub(super) struct PlatformGenericRosterAdmission {
    root: [u8; 32],
    _owner: Arc<crate::profile_storage::OwnedProfileLease>,
}

#[allow(dead_code)]
pub(super) async fn initialize(
    owned: &ProfileOwnedSecureStorage,
    four: PlatformFourRosterAdmission,
) -> Result<PlatformGenericRosterAdmission, (AuraError, PlatformFourRosterAdmission)> {
    let result = initialize_borrowed(owned, &four, Stop::Never).await;
    result.map_err(|error| (error, four))
}

async fn initialize_borrowed(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
    stop: Stop,
) -> Result<PlatformGenericRosterAdmission, AuraError> {
    let (selected, owner) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = verify_four(owned, selected, four)?;
    if root.generic_phase != GENERIC_UNCLAIMED {
        return Err(invalid("platform generic roster was already claimed"));
    }
    let (fourth, _) = validate_four(owned, selected, &mut root)?;
    let reference = PageRef {
        max_id: fourth.id,
        digest: aura_core::hash::hash(&encode(&root, &[fourth])?),
        generation: 4,
        slot: 0,
    };
    let initial = manifest_bytes(
        &root,
        &Manifest {
            count: 4,
            pages: vec![reference],
        },
    )?;
    if marker(owned, 4, &[])? {
        return Err(invalid("platform generic roster has unexplained marker"));
    }
    root.generic_phase = GENERIC_INIT_PENDING;
    root.generic_target_digest = aura_core::hash::hash(&initial);
    write_keyring(selected, &root)?;
    if stop == Stop::Root {
        return Err(interrupted());
    }
    reconcile_locked(owned, selected, &mut root, stop)?;
    Ok(PlatformGenericRosterAdmission {
        root: root.id,
        _owner: owner.clone(),
    })
}

#[allow(dead_code)]
pub(super) async fn recover_generic(
    owned: &ProfileOwnedSecureStorage,
) -> Result<PlatformGenericRosterAdmission, AuraError> {
    let (selected, owner) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    if owner.lifetime_root_claimed.load(Ordering::Acquire) {
        return Err(invalid("platform generic root already transferred"));
    }
    let mut root = read_root(owned, selected)?;
    reconcile_locked(owned, selected, &mut root, Stop::Never)?;
    owner
        .lifetime_root_claimed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("platform generic root already transferred"))?;
    Ok(PlatformGenericRosterAdmission {
        root: root.id,
        _owner: owner.clone(),
    })
}

fn verify_owner(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    generic: &PlatformGenericRosterAdmission,
) -> Result<(RootRecord, Vec<Entry>, [u8; 32]), AuraError> {
    let (_, owner) = selected_owner(owned)?;
    if !Arc::ptr_eq(owner, &generic._owner) || !owner.lifetime_root_claimed.load(Ordering::Acquire)
    {
        return Err(invalid(
            "platform generic owner lacks original profile lease",
        ));
    }
    let mut root = read_root(owned, selected)?;
    if root.id != generic.root {
        return Err(invalid("platform generic owner has foreign root"));
    }
    let entries = reconcile_locked(owned, selected, &mut root, Stop::Never)?;
    let (claim, _) = prior(owned, selected, &mut root)?;
    Ok((root, entries, claim.allocation))
}

#[allow(dead_code)]
pub(super) async fn append(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    scope: &[u8],
    secret: &[u8],
) -> Result<[u8; 32], AuraError> {
    append_inner(owned, generic, scope, secret, None, Stop::Never).await
}

async fn append_inner(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    scope: &[u8],
    secret: &[u8],
    fixed_id: Option<[u8; 32]>,
    stop: Stop,
) -> Result<[u8; 32], AuraError> {
    if scope.is_empty()
        || scope.len() > MAX_SCOPE_BYTES
        || secret.is_empty()
        || secret.len() > MAX_SECRET_BYTES
    {
        return Err(invalid(
            "platform generic birth scope or secret exceeds bound",
        ));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (mut root, current, second_id) = verify_owner(owned, selected, generic)?;
    capacity(root.generic_count as usize)?;
    let mut id = fixed_id.unwrap_or([0; 32]);
    if fixed_id.is_none() {
        rand::rngs::OsRng
            .try_fill_bytes(&mut id)
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "platform generic allocation entropy",
                    Arc::new(source),
                )
            })?;
    }
    let intent = BirthIntent {
        root: root.id,
        allocation: id,
        scope: scope.to_vec(),
        secret: Zeroizing::new(secret.to_vec()),
    };
    let bytes = intent.encode(selected)?;
    let digest = aura_core::hash::hash(&bytes);
    let next = candidate(&root, second_id, &current, &intent, digest)?;
    if read_entry(selected, &user(INTENT_PREFIX, &id))?.is_some()
        || read_entry(selected, &user(RECORD_PREFIX, &id))?.is_some()
    {
        return Err(invalid(
            "platform generic allocation has existing credential",
        ));
    }
    root.generic_phase = GENERIC_APPEND_PENDING;
    let manifest = read_manifest(
        owned,
        &root,
        root.generic_count as usize,
        root.generic_current_digest,
    )?;
    let (target, _) = next_manifest(&root, &manifest, &current, &next)?;
    root.generic_target_digest = aura_core::hash::hash(&manifest_bytes(&root, &target)?);
    root.generic_next_id = id;
    root.generic_next_intent_digest = digest;
    write_keyring(selected, &root)?;
    if stop == Stop::Root {
        return Err(interrupted());
    }
    write_exact(selected, &user(INTENT_PREFIX, &id), &bytes)?;
    reconcile_locked(owned, selected, &mut root, stop)?;
    Ok(id)
}

#[allow(dead_code)]
pub(super) async fn read_live(
    owned: &ProfileOwnedSecureStorage,
    generic: &PlatformGenericRosterAdmission,
    id: &[u8; 32],
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (_, entries, _) = verify_owner(owned, selected, generic)?;
    let entry = entries
        .iter()
        .find(|entry| &entry.id == id)
        .ok_or_else(|| invalid("platform generic allocation is not owned"))?;
    if !matches!(entry.phase, LIVE | DECIDED) {
        return Err(invalid("platform generic allocation is not live"));
    }
    let bytes = read_entry(selected, &user(INTENT_PREFIX, id))?
        .ok_or_else(|| invalid("platform generic original intent is missing"))?;
    let intent = BirthIntent::decode(selected, &bytes)?;
    if intent.scope != entry.scope {
        return Err(invalid("platform generic original scope differs"));
    }
    Ok(intent.secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn initial(
        fixture: &Fixture,
    ) -> Result<
        (
            ProductionSecureStorageHandler,
            PlatformGenericRosterAdmission,
        ),
        AuraError,
    > {
        let (storage, four) = super::super::lifecycle::tests::first_four(fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let generic = initialize(owned, four).await.map_err(|(error, _)| error)?;
        Ok((storage, generic))
    }

    #[tokio::test]
    async fn generic_fifth_and_repeated_birth_preserve_original_lease_and_reopen() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let fifth = append(owned, &generic, b"fifth scope", b"fifth secret").await?;
        assert_eq!(
            &**read_live(owned, &generic, &fifth).await?,
            b"fifth secret"
        );
        let sixth = append(owned, &generic, b"sixth scope", b"sixth secret").await?;
        assert_eq!(
            &**read_live(owned, &generic, &sixth).await?,
            b"sixth secret"
        );
        let (selected, _) = selected_owner(owned)?;
        assert_eq!(read_root(owned, selected)?.generic_count, 6);
        assert!(
            recover_generic(owned).await.is_err(),
            "one original transfer"
        );
        drop(generic);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let recovered = recover_generic(owned).await?;
        assert_eq!(
            &**read_live(owned, &recovered, &fifth).await?,
            b"fifth secret"
        );
        assert_eq!(
            &**read_live(owned, &recovered, &sixth).await?,
            b"sixth secret"
        );
        assert!(
            recover_four_roster(owned).await.is_err(),
            "stale four owner"
        );
        Ok(())
    }

    #[tokio::test]
    async fn generic_codec_accepts_maximum_and_refuses_max_plus_one() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let mut current = read_current(
            owned,
            selected,
            &root,
            &original_four(owned, &root)?.remove(0),
        )?;
        for index in 1..MAX_COUNT {
            if current.len() == MAX_COUNT - 3 {
                break;
            }
            let mut id = [0u8; 32];
            id[28..].copy_from_slice(&(index as u32).to_be_bytes());
            if [root.birth_id, root.third_id, root.tail_next_id].contains(&id)
                || current.iter().any(|entry| entry.id == id)
            {
                continue;
            }
            current.push(Entry {
                id,
                birth_digest: [index as u8 | 1; 32],
                phase: LIVE,
                decision_digest: [0; 32],
                tombstone_digest: [0; 32],
                scope: vec![1],
            });
        }
        current.sort_by_key(|entry| entry.id);
        assert_eq!(current.len() + 3, MAX_COUNT);
        let mut pages = Vec::new();
        for (slot, group) in current.chunks(PAGE_SIZE).enumerate() {
            let bytes = encode(&root, group)?;
            assert_eq!(decode(&root, &bytes)?, group);
            pages.push(PageRef {
                max_id: group.last().expect("nonempty").id,
                digest: aura_core::hash::hash(&bytes),
                generation: MAX_COUNT as u16,
                slot: slot as u16,
            });
        }
        let manifest = Manifest {
            count: MAX_COUNT,
            pages,
        };
        let bytes = manifest_bytes(&root, &manifest)?;
        assert_eq!(decode_manifest(&root, &bytes)?.count, MAX_COUNT);
        assert!(bytes.len() <= MAX_MANIFEST);
        let largest_page = (0..PAGE_SIZE)
            .map(|index| {
                let mut id = [0u8; 32];
                id[28..].copy_from_slice(&(index as u32 + 1).to_be_bytes());
                Entry {
                    id,
                    birth_digest: [1; 32],
                    phase: LIVE,
                    decision_digest: [0; 32],
                    tombstone_digest: [0; 32],
                    scope: vec![1; MAX_SCOPE_BYTES],
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(encode(&root, &largest_page)?.len(), MAX_FILE);
        for (reference, group) in manifest.pages.iter().zip(current.chunks(PAGE_SIZE)) {
            publish_page(owned, &root, reference, group)?;
        }
        assert_eq!(read_all(owned, &root, &manifest)?, current);
        assert!(capacity(MAX_COUNT - 1).is_ok());
        assert!(capacity(MAX_COUNT).is_err());
        current.push(Entry {
            id: [255; 32],
            birth_digest: [1; 32],
            phase: LIVE,
            decision_digest: [0; 32],
            tombstone_digest: [0; 32],
            scope: vec![1],
        });
        current.sort_by_key(|entry| entry.id);
        assert!(manifest_bytes(
            &root,
            &Manifest {
                count: MAX_COUNT + 1,
                pages: manifest.pages
            }
        )
        .is_err());
        drop(generic);
        Ok(())
    }

    #[tokio::test]
    async fn generic_init_and_append_replay_only_root_anchored_intent() -> TestResult {
        for stop in [Stop::Root, Stop::Marker] {
            let fixture = Fixture::new()?;
            let (storage, four) = super::super::lifecycle::tests::first_four(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            assert!(initialize_borrowed(owned, &four, stop).await.is_err());
            drop(four);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            recover_generic(owned).await?;
        }
        for stop in [Stop::Intent, Stop::Record, Stop::Marker] {
            let fixture = Fixture::new()?;
            let (storage, generic) = initial(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            assert!(append_inner(
                owned,
                &generic,
                b"fifth scope",
                b"fifth secret",
                Some([91; 32]),
                stop
            )
            .await
            .is_err());
            assert!(recover_generic(owned).await.is_err());
            let (selected, _) = selected_owner(owned)?;
            assert_eq!(
                read_root(owned, selected)?.generic_phase,
                GENERIC_APPEND_PENDING,
                "foreign recovery must not finish the original owner's transition"
            );
            drop(generic);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_generic(owned).await?;
            assert_eq!(
                &**read_live(owned, &recovered, &[91; 32]).await?,
                b"fifth secret"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn generic_root_only_and_foreign_owner_refuse_before_new_record() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let foreign = tempfile::tempdir()?;
        let foreign_storage = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_storage else {
            unreachable!()
        };
        assert!(append(foreign_owned, &generic, b"foreign", b"secret")
            .await
            .is_err());
        assert!(append_inner(
            owned,
            &generic,
            b"fifth scope",
            b"fifth secret",
            Some([92; 32]),
            Stop::Root
        )
        .await
        .is_err());
        let (selected, _) = selected_owner(owned)?;
        assert!(read_entry(selected, &user(RECORD_PREFIX, &[92; 32]))?.is_none());
        drop(generic);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_generic(owned).await.is_err(),
            "missing original sealed intent"
        );
        Ok(())
    }

    #[tokio::test]
    async fn generic_recovery_refuses_root_rollback_and_changed_record() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let old = read_root(owned, selected)?;
        let fifth = append(owned, &generic, b"fifth", b"secret").await?;
        let exact = read_entry(selected, &user(RECORD_PREFIX, &fifth))?.expect("record");
        write_exact(selected, &user(RECORD_PREFIX, &fifth), b"changed")?;
        assert!(recover_generic(owned).await.is_err());
        write_exact(selected, &user(RECORD_PREFIX, &fifth), &exact)?;
        write_keyring(selected, &old)?;
        drop(generic);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_generic(owned).await.is_err(),
            "future marker reveals root rollback"
        );
        Ok(())
    }

    #[tokio::test]
    async fn generic_paged_append_reuses_unchanged_pages_and_bounds_maximum() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let fourth = original_four(owned, &root)?.remove(0);
        let mut old = vec![fourth];
        for index in 1..=PAGE_SIZE {
            let mut id = [0u8; 32];
            id[28..].copy_from_slice(&(index as u32).to_be_bytes());
            if old.iter().any(|entry| entry.id == id) {
                continue;
            }
            old.push(Entry {
                id,
                birth_digest: [index as u8 | 1; 32],
                phase: LIVE,
                decision_digest: [0; 32],
                tombstone_digest: [0; 32],
                scope: vec![1],
            });
        }
        old.sort_by_key(|entry| entry.id);
        let first = old[..PAGE_SIZE / 2].to_vec();
        let second = old[PAGE_SIZE / 2..].to_vec();
        let manifest = Manifest {
            count: old.len() + 3,
            pages: [first, second]
                .into_iter()
                .enumerate()
                .map(|(slot, page)| {
                    Ok(PageRef {
                        max_id: page.last().expect("nonempty").id,
                        digest: aura_core::hash::hash(&encode(&root, &page)?),
                        generation: (old.len() + 3) as u16,
                        slot: slot as u16,
                    })
                })
                .collect::<Result<_, AuraError>>()?,
        };
        let mut next = old.clone();
        let mut id = [0u8; 32];
        id[28..].copy_from_slice(&1000u32.to_be_bytes());
        next.push(Entry {
            id,
            birth_digest: [3; 32],
            phase: LIVE,
            decision_digest: [0; 32],
            tombstone_digest: [0; 32],
            scope: vec![1],
        });
        next.sort_by_key(|entry| entry.id);
        let (updated, publications) = next_manifest(&root, &manifest, &old, &next)?;
        assert_eq!(updated.pages[0], manifest.pages[0]);
        assert_eq!(publications.len(), 1);
        assert!(encode(&root, &publications[0].1)?.len() <= MAX_FILE);
        assert!(manifest_bytes(&root, &updated)?.len() <= MAX_MANIFEST);
        drop(generic);
        Ok(())
    }

    #[tokio::test]
    async fn generic_full_page_splits_under_authenticated_manifest() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let mut old = vec![original_four(owned, &root)?.remove(0)];
        for index in 1..=PAGE_SIZE {
            let mut id = [0u8; 32];
            id[28..].copy_from_slice(&(index as u32).to_be_bytes());
            if old.iter().any(|entry| entry.id == id) {
                continue;
            }
            old.push(Entry {
                id,
                birth_digest: [1; 32],
                phase: LIVE,
                decision_digest: [0; 32],
                tombstone_digest: [0; 32],
                scope: vec![1],
            });
            if old.len() == PAGE_SIZE {
                break;
            }
        }
        old.sort_by_key(|entry| entry.id);
        assert_eq!(old.len(), PAGE_SIZE);
        let manifest = Manifest {
            count: PAGE_SIZE + 3,
            pages: vec![PageRef {
                max_id: old.last().expect("page").id,
                digest: aura_core::hash::hash(&encode(&root, &old)?),
                generation: (PAGE_SIZE + 3) as u16,
                slot: 0,
            }],
        };
        let mut next = old.clone();
        next.push(Entry {
            id: [255; 32],
            birth_digest: [2; 32],
            phase: LIVE,
            decision_digest: [0; 32],
            tombstone_digest: [0; 32],
            scope: vec![1],
        });
        next.sort_by_key(|entry| entry.id);
        let (target, changed) = next_manifest(&root, &manifest, &old, &next)?;
        assert_eq!(changed.len(), 2);
        assert_eq!(target.pages.len(), 2);
        for (reference, entries) in &changed {
            publish_page(owned, &root, reference, entries)?;
        }
        assert_eq!(read_all(owned, &root, &target)?, next);
        drop(generic);
        Ok(())
    }

    #[tokio::test]
    async fn generic_reopen_refuses_missing_page_or_manifest() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, generic) = initial(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append(owned, &generic, b"fifth", b"secret").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let manifest = read_manifest(owned, &root, 5, root.generic_current_digest)?;
        drop(generic);
        drop(storage);
        for file in [
            page_path(
                manifest.pages.last().expect("page").generation,
                manifest.pages.last().expect("page").slot,
            ),
            path(5),
        ] {
            let file = fixture.profile.path().join(file);
            let original = std::fs::read(&file)?;
            std::fs::remove_file(&file)?;
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            assert!(recover_generic(owned).await.is_err());
            drop(reopened);
            std::fs::write(&file, original)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
        }
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        recover_generic(owned).await?;
        Ok(())
    }
}
