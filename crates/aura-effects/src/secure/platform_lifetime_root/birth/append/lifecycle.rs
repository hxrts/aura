//! Original selected-provider second-entry transition. Every mutable step is
//! anchored in the V5 root before a per-ID write or credential deletion.

use super::super::decision::{DecisionRecord, NEGATIVE, POSITIVE};
use super::*;
use aura_core::effects::secret_lifetime::MAX_DECISION_BYTES;

const DECISION_PREFIX: &str = "allocation-lifetime-second-decision:v5:";
const TOMBSTONE_PREFIX: &str = "allocation-lifetime-second-tombstone:v5:";
const TOMBSTONE_MAGIC: &[u8] = b"AURA-PLATFORM-SECOND-TOMBSTONE-V5";
const TOMBSTONE_NONCE_DOMAIN: &[u8] = b"aura/platform-second-tombstone-nonce/v5";
const TOMBSTONE_MARKER_MAGIC: &[u8] = b"AURA-PLATFORM-SECOND-TOMBSTONE-MARKER-V5";
const MAX_MARKER_LEN: usize = 256;

fn decision_file(id: &[u8; 32]) -> String {
    format!("platform_lifetime/second-decision-{}.v5", hex::encode(id))
}

fn tombstone_file(id: &[u8; 32]) -> String {
    format!("platform_lifetime/second-tombstone-{}.v5", hex::encode(id))
}

#[cfg(test)]
pub(super) fn cleanup_fixture(service: &str, id: &[u8; 32]) {
    for name in [user(DECISION_PREFIX, id), user(TOMBSTONE_PREFIX, id)] {
        if let Ok(entry) = keyring::Entry::new(service, &name) {
            let _ = entry.delete_credential();
        }
    }
}

fn marker_state(
    owned: &ProfileOwnedSecureStorage,
    path: &str,
    expected: &[u8],
) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(path), MAX_MARKER_LEN)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == expected => Ok(true),
        _ => Err(invalid("platform second transition marker differs")),
    }
}

fn publish_marker(
    owned: &ProfileOwnedSecureStorage,
    path: &str,
    expected: &[u8],
) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(path), expected)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform second transition marker conflicted"));
    }
    staged.acknowledge().map_err(io)
}

pub(in crate::secure::platform_lifetime_root::birth) fn decision_record(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    claim: &Claim,
) -> Result<Option<DecisionRecord>, AuraError> {
    let bytes = read_entry(selected, &user(DECISION_PREFIX, &claim.allocation))?;
    if root.second_phase == SECOND_BIRTH_COMPLETE {
        if bytes.is_some() {
            return Err(invalid("platform second decision lacks root intent"));
        }
        return Ok(None);
    }
    let bytes = bytes.ok_or_else(|| invalid("platform second decision record is missing"))?;
    if aura_core::hash::hash(&bytes) != root.second_decision_digest {
        return Err(invalid("platform second decision differs from root intent"));
    }
    let record = DecisionRecord::decode(selected, &bytes)?;
    if record.root != root.id
        || record.allocation != claim.allocation
        || record.profile != root.profile
        || record.service != root.service
    {
        return Err(invalid("platform second decision has foreign identity"));
    }
    Ok(Some(record))
}

fn finish_decision(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    claim: &Claim,
) -> Result<DecisionRecord, AuraError> {
    let record = decision_record(selected, root, claim)?
        .ok_or_else(|| invalid("platform second decision has not started"))?;
    let path = decision_file(&claim.allocation);
    if !marker_state(owned, &path, &record.marker())? {
        if root.second_phase == SECOND_DECISION_PENDING {
            publish_marker(owned, &path, &record.marker())?;
        } else {
            return Err(invalid(
                "completed platform second decision marker is missing",
            ));
        }
    }
    if root.second_phase == SECOND_DECISION_PENDING {
        root.second_phase = SECOND_DECISION_COMPLETE;
        write_keyring(selected, root)?;
    }
    Ok(record)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecisionStop {
    Never,
    RootIntent,
    Record,
    Marker,
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn decide(
    owned: &ProfileOwnedSecureStorage,
    roster: &PlatformRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalRosterOwner::Two(roster),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn decide_three(
    owned: &ProfileOwnedSecureStorage,
    three: &super::third::PlatformThreeRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalRosterOwner::Three(three),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn decide_four(
    owned: &ProfileOwnedSecureStorage,
    four: &super::third::fourth::PlatformFourRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalRosterOwner::Four(four),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn decide_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &third::fourth::generic::PlatformGenericRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalRosterOwner::Generic(generic),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

async fn decide_inner(
    owned: &ProfileOwnedSecureStorage,
    roster: OriginalRosterOwner<'_>,
    positive: bool,
    bytes: &[u8],
    stop: DecisionStop,
) -> Result<(), AuraError> {
    if bytes.is_empty() || bytes.len() > MAX_DECISION_BYTES {
        return Err(invalid("platform second decision size is invalid"));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = read_root(owned, selected)?;
    let (mut root, claim) = super::bound_original_state(owned, selected, root, roster)?;
    if root.second_phase < SECOND_ERASURE_PENDING {
        super::replay(owned, selected, &root, &claim, StopAfter::Never)?;
    }
    if root.second_phase != SECOND_BIRTH_COMPLETE {
        let existing = finish_decision(owned, selected, &mut root, &claim)?;
        if existing.kind != if positive { POSITIVE } else { NEGATIVE }
            || existing.bytes.as_slice() != bytes
        {
            return Err(invalid("platform second decision contradicts original"));
        }
        return Ok(());
    }
    recover(owned, selected, &mut root, &claim)?;
    let path = decision_file(&claim.allocation);
    if read_entry(selected, &user(DECISION_PREFIX, &claim.allocation))?.is_some()
        || marker_state(owned, &path, &[])?
    {
        return Err(invalid(
            "platform second decision found unexplained evidence",
        ));
    }
    let record = DecisionRecord {
        kind: if positive { POSITIVE } else { NEGATIVE },
        root: root.id,
        allocation: claim.allocation,
        profile: root.profile,
        service: root.service,
        bytes: Zeroizing::new(bytes.to_vec()),
    };
    let encoded = record.encode(selected)?;
    root.second_phase = SECOND_DECISION_PENDING;
    root.second_decision_digest = aura_core::hash::hash(&encoded);
    write_keyring(selected, &root)?;
    if stop == DecisionStop::RootIntent {
        return Err(interrupted());
    }
    write_exact(
        selected,
        &user(DECISION_PREFIX, &claim.allocation),
        &encoded,
    )?;
    if stop == DecisionStop::Record {
        return Err(interrupted());
    }
    publish_marker(owned, &path, &record.marker())?;
    if stop == DecisionStop::Marker {
        return Err(interrupted());
    }
    finish_decision(owned, selected, &mut root, &claim)?;
    Ok(())
}

pub(in crate::secure::platform_lifetime_root::birth) fn bound_claim(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    roster: &PlatformRosterAdmission,
) -> Result<Claim, AuraError> {
    if !Arc::ptr_eq(&roster._owner, &owned._owner)
        || roster.root != root.id
        || root.third_phase != THIRD_UNCLAIMED
        || !owned._owner.lifetime_root_claimed.load(Ordering::Acquire)
    {
        return Err(invalid(
            "platform second transition lacks original roster owner",
        ));
    }
    let bytes = read_entry(selected, CLAIM_USER)?
        .ok_or_else(|| invalid("platform second claim is missing"))?;
    let claim = Claim::decode(&bytes, root)?;
    let mut expected = [root.birth_id, claim.allocation];
    expected.sort();
    let birth_roster = roster_bytes(root, &claim);
    super::validate_roster_layout(&birth_roster, root, &claim)?;
    if roster.allocations != expected || aura_core::hash::hash(&birth_roster) != root.roster_digest
    {
        return Err(invalid("platform second transition has foreign roster"));
    }
    Ok(claim)
}

fn roster_bytes(root: &RootRecord, claim: &Claim) -> Vec<u8> {
    super::roster(root, claim)
}

fn sealed_tombstone(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    claim: &Claim,
) -> Result<Vec<u8>, AuraError> {
    let key = Zeroizing::new(selected.record_key(false)?);
    let mut nonce_input = Vec::with_capacity(TOMBSTONE_NONCE_DOMAIN.len() + 96);
    nonce_input.extend_from_slice(TOMBSTONE_NONCE_DOMAIN);
    nonce_input.extend_from_slice(&root.id);
    nonce_input.extend_from_slice(&claim.allocation);
    nonce_input.extend_from_slice(&root.second_decision_digest);
    let digest = aura_core::hash::hash(&nonce_input);
    let mut plain = Zeroizing::new(Vec::with_capacity(192));
    plain.extend_from_slice(&root.id);
    plain.extend_from_slice(&claim.allocation);
    plain.extend_from_slice(&root.second_decision_digest);
    plain.extend_from_slice(&claim.intent_digest);
    plain.extend_from_slice(&root.profile);
    plain.extend_from_slice(&root.service);
    let ciphertext = ChaCha20Poly1305::new((&*key).into())
        .encrypt(
            Nonce::from_slice(&digest[..12]),
            Payload {
                msg: &plain,
                aad: TOMBSTONE_MAGIC,
            },
        )
        .map_err(|source| {
            AuraError::crypto_with_source("platform second tombstone encryption", Arc::new(source))
        })?;
    let mut encoded = Vec::with_capacity(TOMBSTONE_MAGIC.len() + 12 + ciphertext.len());
    encoded.extend_from_slice(TOMBSTONE_MAGIC);
    encoded.extend_from_slice(&digest[..12]);
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

fn tombstone_marker(root: &RootRecord, claim: &Claim, bytes: &[u8]) -> Vec<u8> {
    let mut marker = Vec::with_capacity(TOMBSTONE_MARKER_MAGIC.len() + 160);
    marker.extend_from_slice(TOMBSTONE_MARKER_MAGIC);
    marker.extend_from_slice(&root.id);
    marker.extend_from_slice(&claim.allocation);
    marker.extend_from_slice(&root.profile);
    marker.extend_from_slice(&root.service);
    marker.extend_from_slice(&aura_core::hash::hash(bytes));
    marker
}

fn finish_tombstone(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    claim: &Claim,
) -> Result<(), AuraError> {
    let decision = decision_record(selected, root, claim)?
        .ok_or_else(|| invalid("platform second denial lacks decision"))?;
    if decision.kind != NEGATIVE {
        return Err(invalid("platform second denial lacks negative decision"));
    }
    let bytes = sealed_tombstone(selected, root, claim)?;
    if aura_core::hash::hash(&bytes) != root.second_tombstone_digest {
        return Err(invalid(
            "platform second tombstone differs from root intent",
        ));
    }
    let user = user(TOMBSTONE_PREFIX, &claim.allocation);
    match read_entry(selected, &user)? {
        Some(observed) if observed == bytes => {}
        None if root.second_phase == SECOND_TOMBSTONE_PENDING => {
            write_exact(selected, &user, &bytes)?;
        }
        _ => return Err(invalid("platform second tombstone is missing or different")),
    }
    let path = tombstone_file(&claim.allocation);
    let marker = tombstone_marker(root, claim, &bytes);
    if !marker_state(owned, &path, &marker)? {
        if root.second_phase == SECOND_TOMBSTONE_PENDING {
            publish_marker(owned, &path, &marker)?;
        } else {
            return Err(invalid(
                "acknowledged platform second tombstone marker is missing",
            ));
        }
    }
    if root.second_phase == SECOND_TOMBSTONE_PENDING {
        root.second_phase = SECOND_TOMBSTONE_ACK_ERASURE_PENDING;
        write_keyring(selected, root)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DenialStop {
    Never,
    RootIntent,
    Tombstone,
    Marker,
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_negative(
    owned: &ProfileOwnedSecureStorage,
    roster: &PlatformRosterAdmission,
    exact_decision: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalRosterOwner::Two(roster),
        exact_decision,
        DenialStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_three(
    owned: &ProfileOwnedSecureStorage,
    three: &super::third::PlatformThreeRosterAdmission,
    exact_decision: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalRosterOwner::Three(three),
        exact_decision,
        DenialStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_four(
    owned: &ProfileOwnedSecureStorage,
    four: &super::third::fourth::PlatformFourRosterAdmission,
    exact_decision: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalRosterOwner::Four(four),
        exact_decision,
        DenialStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &third::fourth::generic::PlatformGenericRosterAdmission,
    bytes: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalRosterOwner::Generic(generic),
        bytes,
        DenialStop::Never,
    )
    .await
}

async fn deny_inner(
    owned: &ProfileOwnedSecureStorage,
    roster: OriginalRosterOwner<'_>,
    exact_decision: &[u8],
    stop: DenialStop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = read_root(owned, selected)?;
    let (mut root, claim) = super::bound_original_state(owned, selected, root, roster)?;
    if root.second_phase < SECOND_ERASURE_PENDING {
        super::replay(owned, selected, &root, &claim, StopAfter::Never)?;
    }
    let decision = finish_decision(owned, selected, &mut root, &claim)?;
    if decision.kind != NEGATIVE || decision.bytes.as_slice() != exact_decision {
        return Err(invalid(
            "platform second denial lacks exact negative decision",
        ));
    }
    if root.second_phase == SECOND_DECISION_COMPLETE {
        let bytes = sealed_tombstone(selected, &root, &claim)?;
        root.second_phase = SECOND_TOMBSTONE_PENDING;
        root.second_tombstone_digest = aura_core::hash::hash(&bytes);
        write_keyring(selected, &root)?;
        if stop == DenialStop::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.second_phase,
        SECOND_TOMBSTONE_PENDING | SECOND_TOMBSTONE_ACK_ERASURE_PENDING
    ) {
        return Err(invalid("platform second denial has invalid phase"));
    }
    if stop == DenialStop::Tombstone {
        let bytes = sealed_tombstone(selected, &root, &claim)?;
        write_exact(selected, &user(TOMBSTONE_PREFIX, &claim.allocation), &bytes)?;
        return Err(interrupted());
    }
    if stop == DenialStop::Marker {
        let bytes = sealed_tombstone(selected, &root, &claim)?;
        write_exact(selected, &user(TOMBSTONE_PREFIX, &claim.allocation), &bytes)?;
        publish_marker(
            owned,
            &tombstone_file(&claim.allocation),
            &tombstone_marker(&root, &claim, &bytes),
        )?;
        return Err(interrupted());
    }
    finish_tombstone(owned, selected, &mut root, &claim)
}

fn survivor(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    claim: &Claim,
    user: &str,
) -> Result<bool, AuraError> {
    match read_entry(selected, user)? {
        Some(bytes) if aura_core::hash::hash(&bytes) == claim.intent_digest => Ok(true),
        Some(_) => Err(invalid("platform second erasure found changed credential")),
        None if matches!(
            root.second_phase,
            SECOND_ERASURE_PENDING | SECOND_ERASURE_COMPLETE
        ) =>
        {
            Ok(false)
        }
        None => Err(invalid(
            "platform second erasure original credential is missing",
        )),
    }
}

fn delete_and_readback(
    selected: &PlatformSecureStorageHandler,
    user: &str,
) -> Result<(), AuraError> {
    let entry = selected.entry_for_user(user)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(source) => return Err(PlatformSecureStorageHandler::map_keyring_error(source)),
    }
    if read_entry(selected, user)?.is_some() {
        return Err(invalid(
            "platform second credential deletion was not acknowledged",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErasureStop {
    Never,
    RootIntent,
    RecordDelete,
    IntentDelete,
}

fn finish_erasure(
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    claim: &Claim,
    stop: ErasureStop,
) -> Result<(), AuraError> {
    let record_user = user(RECORD_PREFIX, &claim.allocation);
    let intent_user = user(INTENT_PREFIX, &claim.allocation);
    let record = survivor(selected, root, claim, &record_user)?;
    let intent = survivor(selected, root, claim, &intent_user)?;
    if root.second_phase == SECOND_ERASURE_COMPLETE {
        if record || intent {
            return Err(invalid("completed platform second credential reappeared"));
        }
        return Ok(());
    }
    if record {
        delete_and_readback(selected, &record_user)?;
    }
    if stop == ErasureStop::RecordDelete {
        return Err(interrupted());
    }
    if intent {
        delete_and_readback(selected, &intent_user)?;
    }
    if stop == ErasureStop::IntentDelete {
        return Err(interrupted());
    }
    root.second_phase = SECOND_ERASURE_COMPLETE;
    write_keyring(selected, root)
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn erase(
    owned: &ProfileOwnedSecureStorage,
    roster: &PlatformRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, OriginalRosterOwner::Two(roster), ErasureStop::Never).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_three(
    owned: &ProfileOwnedSecureStorage,
    three: &super::third::PlatformThreeRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, OriginalRosterOwner::Three(three), ErasureStop::Never).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_four(
    owned: &ProfileOwnedSecureStorage,
    four: &super::third::fourth::PlatformFourRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, OriginalRosterOwner::Four(four), ErasureStop::Never).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &third::fourth::generic::PlatformGenericRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(
        owned,
        OriginalRosterOwner::Generic(generic),
        ErasureStop::Never,
    )
    .await
}

async fn erase_inner(
    owned: &ProfileOwnedSecureStorage,
    roster: OriginalRosterOwner<'_>,
    stop: ErasureStop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = read_root(owned, selected)?;
    let (mut root, claim) = super::bound_original_state(owned, selected, root, roster)?;
    if root.second_phase < SECOND_ERASURE_PENDING {
        super::replay(owned, selected, &root, &claim, StopAfter::Never)?;
    }
    finish_decision(owned, selected, &mut root, &claim)?;
    finish_tombstone(owned, selected, &mut root, &claim)?;
    if root.second_phase == SECOND_TOMBSTONE_ACK_ERASURE_PENDING {
        if !survivor(
            selected,
            &root,
            &claim,
            &user(RECORD_PREFIX, &claim.allocation),
        )? || !survivor(
            selected,
            &root,
            &claim,
            &user(INTENT_PREFIX, &claim.allocation),
        )? {
            return Err(invalid(
                "platform second erasure lacks original credentials",
            ));
        }
        root.second_phase = SECOND_ERASURE_PENDING;
        write_keyring(selected, &root)?;
        if stop == ErasureStop::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.second_phase,
        SECOND_ERASURE_PENDING | SECOND_ERASURE_COMPLETE
    ) {
        return Err(invalid("platform second erasure lacks acknowledged denial"));
    }
    finish_erasure(selected, &mut root, &claim, stop)
}

/// Reconcile only authenticated original V5 stages. No caller-supplied bytes,
/// expiry, delete address or alternative provider can enter this path.
pub(super) fn recover(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    claim: &Claim,
) -> Result<(), AuraError> {
    if root.second_phase < SECOND_ERASURE_PENDING {
        super::replay(owned, selected, root, claim, StopAfter::Never)?;
    }
    if root.second_phase == SECOND_BIRTH_COMPLETE {
        if decision_record(selected, root, claim)?.is_some()
            || read_entry(selected, &user(TOMBSTONE_PREFIX, &claim.allocation))?.is_some()
            || marker_state(owned, &decision_file(&claim.allocation), &[])?
            || marker_state(owned, &tombstone_file(&claim.allocation), &[])?
        {
            return Err(invalid(
                "platform second transition evidence lacks root intent",
            ));
        }
        return Ok(());
    }
    finish_decision(owned, selected, root, claim)?;
    if root.second_phase >= SECOND_TOMBSTONE_PENDING {
        finish_tombstone(owned, selected, root, claim)?;
    } else if read_entry(selected, &user(TOMBSTONE_PREFIX, &claim.allocation))?.is_some()
        || marker_state(owned, &tombstone_file(&claim.allocation), &[])?
    {
        return Err(invalid("platform second tombstone lacks root intent"));
    }
    if root.second_phase >= SECOND_ERASURE_PENDING {
        finish_erasure(selected, root, claim, ErasureStop::Never)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) async fn read_live_secret(
    owned: &ProfileOwnedSecureStorage,
    roster: &PlatformRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalRosterOwner::Two(roster)).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn read_three_secret(
    owned: &ProfileOwnedSecureStorage,
    three: &super::third::PlatformThreeRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalRosterOwner::Three(three)).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn read_four_secret(
    owned: &ProfileOwnedSecureStorage,
    four: &super::third::fourth::PlatformFourRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalRosterOwner::Four(four)).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn read_generic_secret(
    owned: &ProfileOwnedSecureStorage,
    generic: &third::fourth::generic::PlatformGenericRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalRosterOwner::Generic(generic)).await
}

async fn read_owned_secret(
    owned: &ProfileOwnedSecureStorage,
    roster: OriginalRosterOwner<'_>,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = read_root(owned, selected)?;
    let (mut root, claim) = super::bound_original_state(owned, selected, root, roster)?;
    recover(owned, selected, &mut root, &claim)?;
    if root.second_phase >= SECOND_TOMBSTONE_PENDING {
        return Err(invalid(
            "platform second live read denied by original tombstone",
        ));
    }
    let bytes = read_entry(selected, &user(INTENT_PREFIX, &claim.allocation))?
        .ok_or_else(|| invalid("platform second original intent is missing"))?;
    if aura_core::hash::hash(&bytes) != claim.intent_digest
        || read_entry(selected, &user(RECORD_PREFIX, &claim.allocation))?.as_deref()
            != Some(bytes.as_slice())
    {
        return Err(invalid("platform second live credential differs"));
    }
    Ok(BirthIntent::decode(selected, &bytes)?.secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

    async fn two_births() -> TestResult<(
        Fixture,
        ProductionSecureStorageHandler,
        PlatformRosterAdmission,
    )> {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"first scope", b"first secret").await?;
        append_second(owned, &birth, b"second scope", b"second secret").await?;
        drop(birth);
        drop(empty);
        drop(first);
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let roster = recover_roster(owned).await?;
        Ok((fixture, selected, roster))
    }

    #[tokio::test]
    async fn second_positive_decision_is_exact_and_reopens() -> TestResult {
        let (fixture, selected, roster) = two_births().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        assert_eq!(&**read_live_secret(owned, &roster).await?, b"second secret");
        decide(owned, &roster, true, b"original approval").await?;
        decide(owned, &roster, true, b"original approval").await?;
        assert!(decide(owned, &roster, false, b"original approval")
            .await
            .is_err());
        assert!(decide(owned, &roster, true, b"other approval")
            .await
            .is_err());
        assert!(acknowledge_negative(owned, &roster, b"original approval")
            .await
            .is_err());
        drop(roster);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let roster = recover_roster(owned).await?;
        assert_eq!(&**read_live_secret(owned, &roster).await?, b"second secret");
        let (platform, _) = selected_owner(owned)?;
        assert_eq!(
            read_root(owned, platform)?.second_phase,
            SECOND_DECISION_COMPLETE
        );
        Ok(())
    }

    #[tokio::test]
    async fn second_negative_denial_erases_original_credentials_and_reopens() -> TestResult {
        let (fixture, selected, roster) = two_births().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        decide(owned, &roster, false, b"original refusal").await?;
        acknowledge_negative(owned, &roster, b"original refusal").await?;
        assert!(read_live_secret(owned, &roster).await.is_err());
        let (platform, _) = selected_owner(owned)?;
        let root = read_root(owned, platform)?;
        assert_eq!(root.second_phase, SECOND_TOMBSTONE_ACK_ERASURE_PENDING);
        let claim = Claim::decode(&read_entry(platform, CLAIM_USER)?.expect("claim"), &root)?;
        assert!(read_entry(platform, &user(INTENT_PREFIX, &claim.allocation))?.is_some());
        erase(owned, &roster).await?;
        erase(owned, &roster).await?;
        assert!(read_entry(platform, &user(INTENT_PREFIX, &claim.allocation))?.is_none());
        assert!(read_entry(platform, &user(RECORD_PREFIX, &claim.allocation))?.is_none());
        drop(roster);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let roster = recover_roster(owned).await?;
        assert!(read_live_secret(owned, &roster).await.is_err());
        let (platform, _) = selected_owner(owned)?;
        let root = read_root(owned, platform)?;
        assert_eq!(root.first_scope, b"first scope");
        assert_eq!(root.second_scope, b"second scope");
        assert_eq!(root.second_phase, SECOND_ERASURE_COMPLETE);
        write_exact(
            platform,
            &user(RECORD_PREFIX, &claim.allocation),
            b"reappeared",
        )?;
        drop(roster);
        drop(reopened);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(recover_roster(owned).await.is_err());
        assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
        Ok(())
    }

    #[tokio::test]
    async fn erased_second_entry_refuses_reanchored_missing_retained_scope() -> TestResult {
        let (fixture, selected, roster) = two_births().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        decide(owned, &roster, false, b"original refusal").await?;
        acknowledge_negative(owned, &roster, b"original refusal").await?;
        erase(owned, &roster).await?;
        let (platform, _) = selected_owner(owned)?;
        let mut root = read_root(owned, platform)?;
        let claim = Claim::decode(&read_entry(platform, CLAIM_USER)?.expect("claim"), &root)?;
        root.second_scope.clear();
        let changed = super::roster(&root, &claim);
        root.roster_digest = aura_core::hash::hash(&changed);
        write_keyring(platform, &root)?;
        std::fs::write(fixture.profile.path().join(SECOND_ROSTER_FILE), changed)?;
        drop(roster);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(recover_roster(owned).await.is_err());
        assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
        Ok(())
    }

    #[tokio::test]
    async fn second_decision_interruptions_replay_or_fail_closed() -> TestResult {
        for stop in [
            DecisionStop::RootIntent,
            DecisionStop::Record,
            DecisionStop::Marker,
        ] {
            let (fixture, selected, roster) = two_births().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            assert!(decide_inner(
                owned,
                OriginalRosterOwner::Two(&roster),
                false,
                b"refusal",
                stop
            )
            .await
            .is_err());
            drop(roster);
            drop(selected);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let outcome = recover_roster(owned).await;
            assert_eq!(
                outcome.is_ok(),
                stop != DecisionStop::RootIntent,
                "{stop:?}"
            );
            if stop == DecisionStop::RootIntent {
                assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
            } else {
                assert_eq!(
                    read_root(owned, selected_owner(owned)?.0)?.second_phase,
                    SECOND_DECISION_COMPLETE
                );
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn second_denial_and_erasure_interruptions_reopen_without_live_secret() -> TestResult {
        for stop in [
            DenialStop::RootIntent,
            DenialStop::Tombstone,
            DenialStop::Marker,
        ] {
            let (fixture, selected, roster) = two_births().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            decide(owned, &roster, false, b"refusal").await?;
            assert!(
                deny_inner(owned, OriginalRosterOwner::Two(&roster), b"refusal", stop)
                    .await
                    .is_err()
            );
            drop(roster);
            drop(selected);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let roster = recover_roster(owned).await?;
            assert!(read_live_secret(owned, &roster).await.is_err());
            assert_eq!(
                read_root(owned, selected_owner(owned)?.0)?.second_phase,
                SECOND_TOMBSTONE_ACK_ERASURE_PENDING
            );
        }
        for stop in [
            ErasureStop::RootIntent,
            ErasureStop::RecordDelete,
            ErasureStop::IntentDelete,
        ] {
            let (fixture, selected, roster) = two_births().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            decide(owned, &roster, false, b"refusal").await?;
            acknowledge_negative(owned, &roster, b"refusal").await?;
            assert!(erase_inner(owned, OriginalRosterOwner::Two(&roster), stop)
                .await
                .is_err());
            drop(roster);
            drop(selected);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let roster = recover_roster(owned).await?;
            assert!(read_live_secret(owned, &roster).await.is_err());
            assert_eq!(
                read_root(owned, selected_owner(owned)?.0)?.second_phase,
                SECOND_ERASURE_COMPLETE
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn second_wrong_owner_and_changed_evidence_refuse_without_deletion() -> TestResult {
        let (fixture, selected, roster) = two_births().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let foreign = tempfile::tempdir()?;
        let foreign_selected = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(decide(foreign_owned, &roster, false, b"refusal")
            .await
            .is_err());
        decide(owned, &roster, false, b"refusal").await?;
        let (platform, _) = selected_owner(owned)?;
        let root = read_root(owned, platform)?;
        let claim = Claim::decode(&read_entry(platform, CLAIM_USER)?.expect("claim"), &root)?;
        write_exact(
            platform,
            &user(RECORD_PREFIX, &claim.allocation),
            b"wrong record",
        )?;
        assert!(acknowledge_negative(owned, &roster, b"refusal")
            .await
            .is_err());
        assert!(erase(owned, &roster).await.is_err());
        assert_eq!(
            read_root(owned, platform)?.second_phase,
            SECOND_DECISION_COMPLETE
        );
        assert_eq!(
            read_entry(platform, &user(RECORD_PREFIX, &claim.allocation))?.as_deref(),
            Some(b"wrong record".as_slice())
        );
        Ok(())
    }

    #[tokio::test]
    async fn second_acknowledged_decision_or_tombstone_loss_refuses_recovery() -> TestResult {
        for lost in [
            "decision",
            "decision-marker",
            "tombstone",
            "tombstone-marker",
        ] {
            let (fixture, selected, roster) = two_births().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            decide(owned, &roster, false, b"refusal").await?;
            acknowledge_negative(owned, &roster, b"refusal").await?;
            let (platform, _) = selected_owner(owned)?;
            let root = read_root(owned, platform)?;
            let claim = Claim::decode(&read_entry(platform, CLAIM_USER)?.expect("claim"), &root)?;
            match lost {
                "decision" => platform
                    .entry_for_user(&user(DECISION_PREFIX, &claim.allocation))?
                    .delete_credential()?,
                "decision-marker" => std::fs::remove_file(
                    fixture
                        .profile
                        .path()
                        .join(decision_file(&claim.allocation)),
                )?,
                "tombstone" => platform
                    .entry_for_user(&user(TOMBSTONE_PREFIX, &claim.allocation))?
                    .delete_credential()?,
                _ => std::fs::remove_file(
                    fixture
                        .profile
                        .path()
                        .join(tombstone_file(&claim.allocation)),
                )?,
            }
            drop(roster);
            drop(selected);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            assert!(recover_roster(owned).await.is_err(), "loss of {lost}");
            assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
        }
        Ok(())
    }
}
