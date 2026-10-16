//! One private V10 tail append under the original three-entry owner. The
//! bounded roster codec is reusable; this packet admits exactly count four.

use super::*;

const ROSTER_MAGIC_V10: &[u8] = b"AURA-PLATFORM-LIFETIME-BIRTH-ROSTER-V10";
const FOURTH_ROSTER_FILE: &str = "platform_lifetime/roster-4.v10";
const MAX_V10_ROSTER_FILE: usize = ROSTER_MAGIC_V10.len()
    + 2
    + aura_core::effects::secret_lifetime::MAX_PROFILE_ALLOCATION_COUNT
        * (ROSTER_ENTRY_HEADER_LEN + MAX_SCOPE_BYTES);
pub(in crate::secure::platform_lifetime_root::birth) mod generic;
pub(in crate::secure::platform_lifetime_root::birth) mod lifecycle;

#[cfg(test)]
pub(super) fn cleanup_generic_fixture(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    service: &str,
) {
    generic::cleanup_fixture(owned, root, service);
}

#[cfg(test)]
pub(super) fn cleanup_fixture(service: &str, id: &[u8; 32]) {
    lifecycle::cleanup_fixture(service, id);
    for name in [user(INTENT_PREFIX, id), user(RECORD_PREFIX, id)] {
        if let Ok(entry) = keyring::Entry::new(service, &name) {
            let _ = entry.delete_credential();
        }
    }
}

fn entries(bytes: &[u8], magic: &[u8], root: &RootRecord) -> Result<Vec<Vec<u8>>, AuraError> {
    if bytes.len() < magic.len() + 2
        || !bytes.starts_with(magic)
        || bytes.len() > MAX_V10_ROSTER_FILE
    {
        return Err(invalid("platform tail roster version or length is invalid"));
    }
    let count = u16::from_le_bytes([bytes[magic.len()], bytes[magic.len() + 1]]) as usize;
    if count == 0 || count > aura_core::effects::secret_lifetime::MAX_PROFILE_ALLOCATION_COUNT {
        return Err(invalid("platform tail roster count is invalid"));
    }
    let mut cursor = magic.len() + 2;
    let mut previous: Option<[u8; 32]> = None;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let header_end = cursor
            .checked_add(ROSTER_ENTRY_HEADER_LEN)
            .ok_or_else(|| invalid("platform tail roster length overflow"))?;
        if header_end > bytes.len() {
            return Err(invalid("platform tail roster entry is truncated"));
        }
        let identity = &bytes[cursor..cursor + ROSTER_ENTRY_LEN];
        let allocation: [u8; 32] = fixed_bytes(&identity[32..64])?;
        let scope_len = u16::from_le_bytes([bytes[cursor + 161], bytes[cursor + 162]]) as usize;
        let end = header_end
            .checked_add(scope_len)
            .ok_or_else(|| invalid("platform tail roster scope overflow"))?;
        if scope_len == 0
            || scope_len > MAX_SCOPE_BYTES
            || end > bytes.len()
            || previous.is_some_and(|prior| prior >= allocation)
            || identity[0..32] != root.id
            || identity[96..128] != root.profile
            || identity[128..160] != root.service
            || bytes[cursor + 160] != BIRTH_ACKNOWLEDGED
        {
            return Err(invalid(
                "platform tail roster has foreign or duplicate entry",
            ));
        }
        out.push(bytes[cursor..end].to_vec());
        previous = Some(allocation);
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err(invalid("platform tail roster has trailing bytes"));
    }
    Ok(out)
}

fn candidate(
    root: &RootRecord,
    claim: &Claim,
    third_scope: &[u8],
    fourth_scope: &[u8],
) -> Result<Vec<u8>, AuraError> {
    let prior = third_roster(root, claim, third_scope)?;
    let mut current = entries(&prior, ROSTER_MAGIC, root)?;
    if current.len() != 3 || fourth_scope.is_empty() || fourth_scope.len() > MAX_SCOPE_BYTES {
        return Err(invalid("platform fourth birth lacks exact previous roster"));
    }
    let mut fourth = Vec::with_capacity(ROSTER_ENTRY_HEADER_LEN + fourth_scope.len());
    fourth.extend_from_slice(&root.id);
    fourth.extend_from_slice(&root.tail_next_id);
    fourth.extend_from_slice(&root.tail_next_intent_digest);
    fourth.extend_from_slice(&root.profile);
    fourth.extend_from_slice(&root.service);
    fourth.push(BIRTH_ACKNOWLEDGED);
    fourth.extend_from_slice(&(fourth_scope.len() as u16).to_le_bytes());
    fourth.extend_from_slice(fourth_scope);
    current.push(fourth);
    current.sort_by(|left, right| left[32..64].cmp(&right[32..64]));
    if current
        .windows(2)
        .any(|pair| pair[0][32..64] == pair[1][32..64])
    {
        return Err(invalid(
            "platform fourth birth duplicates original allocation",
        ));
    }
    let mut bytes = Vec::with_capacity(prior.len() + ROSTER_ENTRY_HEADER_LEN + fourth_scope.len());
    bytes.extend_from_slice(ROSTER_MAGIC_V10);
    bytes.extend_from_slice(&4u16.to_le_bytes());
    for entry in current {
        bytes.extend_from_slice(&entry);
    }
    if bytes.len() > MAX_V10_ROSTER_FILE || entries(&bytes, ROSTER_MAGIC_V10, root)?.len() != 4 {
        return Err(invalid(
            "platform fourth roster exceeds authenticated bound",
        ));
    }
    Ok(bytes)
}

fn marker_state(owned: &ProfileOwnedSecureStorage, expected: &[u8]) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(FOURTH_ROSTER_FILE), MAX_V10_ROSTER_FILE)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == expected => Ok(true),
        _ => Err(invalid(
            "platform fourth roster marker differs from root anchor",
        )),
    }
}

fn publish_marker(owned: &ProfileOwnedSecureStorage, bytes: &[u8]) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(FOURTH_ROSTER_FILE), bytes)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform fourth roster publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopAfter {
    Never,
    RootAnchor,
    Intent,
    Record,
    Marker,
}

fn replay(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    claim: &Claim,
    third_scope: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    if !matches!(root.tail_phase, TAIL_APPEND_PENDING..=TAIL_ERASURE_COMPLETE)
        || (root.tail_phase == TAIL_APPEND_PENDING
            && (root.tail_count != 3 || root.tail_roster_digest != root.third_roster_digest))
        || (root.tail_phase >= TAIL_APPEND_COMPLETE
            && (root.tail_count != 4 || root.tail_roster_digest != root.tail_target_digest))
        || root.tail_next_id == [0; 32]
        || root.tail_next_id == root.birth_id
        || root.tail_next_id == claim.allocation
        || root.tail_next_id == root.third_id
    {
        return Err(invalid("platform fourth birth has no original root claim"));
    }
    let intent_user = user(INTENT_PREFIX, &root.tail_next_id);
    let record_user = user(RECORD_PREFIX, &root.tail_next_id);
    let retained = owned
        ._owner
        .directory
        .read_private_publication(Path::new(FOURTH_ROSTER_FILE), MAX_V10_ROSTER_FILE)
        .map_err(io)?;
    let bytes = read_entry(selected, &intent_user)?;
    let fourth_scope = match &bytes {
        Some(bytes) => {
            if aura_core::hash::hash(bytes) != root.tail_next_intent_digest {
                return Err(invalid("platform fourth birth sealed intent differs"));
            }
            let intent = BirthIntent::decode(selected, bytes)?;
            if intent.root != root.id || intent.allocation != root.tail_next_id {
                return Err(invalid("platform fourth birth has foreign identity"));
            }
            intent.scope
        }
        None if root.tail_phase >= TAIL_ERASURE_PENDING => {
            let (marker, None) = retained
                .as_ref()
                .ok_or_else(|| invalid("platform fourth retained roster is missing"))?
            else {
                return Err(invalid("platform fourth retained roster is incomplete"));
            };
            let roster = entries(marker, ROSTER_MAGIC_V10, root)?;
            let fourth = roster
                .iter()
                .find(|entry| entry[32..64] == root.tail_next_id)
                .ok_or_else(|| invalid("platform fourth retained scope is missing"))?;
            fourth[ROSTER_ENTRY_HEADER_LEN..].to_vec()
        }
        None => return Err(invalid("platform fourth birth sealed intent is missing")),
    };
    let expected = candidate(root, claim, third_scope, &fourth_scope)?;
    if aura_core::hash::hash(&expected) != root.tail_target_digest {
        return Err(invalid("platform fourth roster differs from root target"));
    }
    if stop == StopAfter::Intent {
        return Err(interrupted());
    }
    let marker = marker_state(owned, &expected)?;
    if root.tail_phase >= TAIL_ERASURE_PENDING {
        if !marker {
            return Err(invalid("platform fourth retained roster ACK is missing"));
        }
        return Ok(());
    }
    match (read_entry(selected, &record_user)?, bytes.as_ref()) {
        (Some(observed), Some(bytes)) if observed == *bytes => {}
        (Some(_), None) => return Err(invalid("erased platform fourth record reappeared")),
        (Some(_), Some(_)) => {
            return Err(invalid("platform fourth record differs from sealed intent"))
        }
        (None, _) if marker => return Err(invalid("platform fourth record is missing after ACK")),
        (None, _) if root.tail_phase >= TAIL_APPEND_COMPLETE => {
            return Err(invalid("completed platform fourth record is missing"));
        }
        (None, Some(bytes)) => write_exact(selected, &record_user, bytes)?,
        (None, None) => return Err(invalid("platform fourth original intent is missing")),
    }
    if stop == StopAfter::Record {
        return Err(interrupted());
    }
    if !marker {
        if root.tail_phase >= TAIL_APPEND_COMPLETE {
            return Err(invalid("completed platform fourth ACK is missing"));
        }
        publish_marker(owned, &expected)?;
    }
    if stop == StopAfter::Marker {
        return Err(interrupted());
    }
    if root.tail_phase == TAIL_APPEND_PENDING {
        root.tail_phase = TAIL_APPEND_COMPLETE;
        root.tail_count = 4;
        root.tail_roster_digest = root.tail_target_digest;
        write_keyring(selected, root)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn append_fourth(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
    scope: &[u8],
    secret: &[u8],
) -> Result<(), AuraError> {
    append_inner(owned, three, scope, secret, None, StopAfter::Never).await
}

async fn append_inner(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
    scope: &[u8],
    secret: &[u8],
    fixed_id: Option<[u8; 32]>,
    stop: StopAfter,
) -> Result<(), AuraError> {
    if scope.is_empty()
        || scope.len() > MAX_SCOPE_BYTES
        || secret.is_empty()
        || secret.len() > MAX_SECRET_BYTES
    {
        return Err(invalid(
            "platform fourth birth scope or secret exceeds bound",
        ));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (mut root, claim, third_scope) = verify_owner(owned, selected, three)?;
    if root.tail_phase != TAIL_IDLE
        || root.tail_count != 3
        || root.tail_roster_digest != root.third_roster_digest
        || marker_state(owned, &[])?
    {
        return Err(invalid(
            "platform fourth birth already claimed or conflicted",
        ));
    }
    let mut id = fixed_id.unwrap_or([0; 32]);
    if fixed_id.is_none() {
        rand::rngs::OsRng
            .try_fill_bytes(&mut id)
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "platform fourth allocation entropy",
                    Arc::new(source),
                )
            })?;
    }
    if id == [0; 32] || [root.birth_id, claim.allocation, root.third_id].contains(&id) {
        return Err(invalid(
            "platform fourth birth duplicates original identity",
        ));
    }
    let intent = BirthIntent {
        root: root.id,
        allocation: id,
        scope: scope.to_vec(),
        secret: Zeroizing::new(secret.to_vec()),
    };
    let bytes = intent.encode(selected)?;
    if read_entry(selected, &user(INTENT_PREFIX, &id))?.is_some()
        || read_entry(selected, &user(RECORD_PREFIX, &id))?.is_some()
    {
        return Err(invalid("platform fourth birth found existing credential"));
    }
    root.tail_phase = TAIL_APPEND_PENDING;
    root.tail_next_id = id;
    root.tail_next_intent_digest = aura_core::hash::hash(&bytes);
    root.tail_target_digest =
        aura_core::hash::hash(&candidate(&root, &claim, &third_scope, scope)?);
    // The original selected root authenticates the exact target roster
    // before any fourth per-ID credential can be published.
    write_keyring(selected, &root)?;
    if stop == StopAfter::RootAnchor {
        return Err(interrupted());
    }
    write_exact(selected, &user(INTENT_PREFIX, &id), &bytes)?;
    replay(owned, selected, &mut root, &claim, &third_scope, stop)
}

fn prior(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(Claim, Vec<u8>), AuraError> {
    let claim = prior_claim(owned, selected, root)?;
    let third_scope = recover(owned, selected, root, &claim)?;
    if root.tail_count < 3 {
        return Err(invalid(
            "platform fourth birth lacks original three-entry ACK",
        ));
    }
    Ok((claim, third_scope))
}

#[allow(dead_code)]
pub(super) async fn reconcile_fourth(owned: &ProfileOwnedSecureStorage) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = read_root(owned, selected)?;
    let (claim, scope) = prior(owned, selected, &mut root)?;
    replay(owned, selected, &mut root, &claim, &scope, StopAfter::Never)
}

/// Private four-entry custody; no decision or erasure API is issued here.
#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) struct PlatformFourRosterAdmission {
    root: [u8; 32],
    fourth: [u8; 32],
    _owner: Arc<crate::profile_storage::OwnedProfileLease>,
    generic_transferred: std::sync::atomic::AtomicBool,
}

fn verify_four(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    four: &PlatformFourRosterAdmission,
) -> Result<RootRecord, AuraError> {
    verify_four_prior(owned, selected, four).map(|(root, _, _)| root)
}

pub(in crate::secure::platform_lifetime_root::birth) fn verify_four_prior(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    four: &PlatformFourRosterAdmission,
) -> Result<(RootRecord, Claim, Vec<u8>), AuraError> {
    let (_, owner) = selected_owner(owned)?;
    if !Arc::ptr_eq(owner, &four._owner)
        || !owner.lifetime_root_claimed.load(Ordering::Acquire)
        || four.generic_transferred.load(Ordering::Acquire)
    {
        return Err(invalid(
            "platform fourth owner lacks original profile lease",
        ));
    }
    let mut root = read_root(owned, selected)?;
    if root.generic_phase != GENERIC_UNCLAIMED {
        return Err(invalid(
            "platform fourth owner was transferred to generic roster",
        ));
    }
    if root.id != four.root || root.tail_next_id != four.fourth {
        return Err(invalid(
            "platform fourth owner has foreign root or allocation",
        ));
    }
    let (claim, scope) = prior(owned, selected, &mut root)?;
    replay(owned, selected, &mut root, &claim, &scope, StopAfter::Never)?;
    lifecycle::recover(owned, selected, &mut root)?;
    if root.tail_phase < TAIL_APPEND_COMPLETE || root.tail_count != 4 {
        return Err(invalid("platform fourth owner lacks complete roster ACK"));
    }
    Ok((root, claim, scope))
}

pub(in crate::secure::platform_lifetime_root::birth) async fn owned_references(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
) -> Result<Vec<aura_core::effects::secret_lifetime::SecretAllocationReference>, AuraError> {
    use aura_core::effects::secret_lifetime::SecretAllocationReference;
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, claim, third_scope) = verify_four_prior(owned, selected, four)?;
    let (bytes, None) = owned
        ._owner
        .directory
        .read_private_publication(Path::new(FOURTH_ROSTER_FILE), MAX_V10_ROSTER_FILE)
        .map_err(io)?
        .ok_or_else(|| invalid("platform fourth roster ACK is missing"))?
    else {
        return Err(invalid("platform fourth roster ACK is incomplete"));
    };
    let fourth = entries(&bytes, ROSTER_MAGIC_V10, &root)?
        .into_iter()
        .find(|entry| entry[32..64] == root.tail_next_id)
        .ok_or_else(|| invalid("platform fourth retained scope is missing"))?;
    Ok(vec![
        SecretAllocationReference {
            allocation: root.birth_id,
            scope: root.first_scope,
        },
        SecretAllocationReference {
            allocation: claim.allocation,
            scope: root.second_scope,
        },
        SecretAllocationReference {
            allocation: root.third_id,
            scope: third_scope,
        },
        SecretAllocationReference {
            allocation: root.tail_next_id,
            scope: fourth[ROSTER_ENTRY_HEADER_LEN..].to_vec(),
        },
    ])
}

pub(in crate::secure::platform_lifetime_root::birth) async fn owned_state(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
) -> Result<aura_core::effects::secret_lifetime::SecretLifetimeState, AuraError> {
    use aura_core::effects::secret_lifetime::SecretLifetimeState;
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = verify_four(owned, selected, four)?;
    if root.tail_phase == TAIL_DECISION_PENDING {
        return Err(invalid("platform fourth decision is not acknowledged"));
    }
    let Some(record) = lifecycle::decision_record(selected, &root)? else {
        return Ok(SecretLifetimeState::Live);
    };
    match record.kind {
        decision::POSITIVE => Ok(SecretLifetimeState::Positive {
            decision: record.bytes.to_vec(),
        }),
        decision::NEGATIVE if root.tail_phase == TAIL_ERASURE_COMPLETE => {
            Ok(SecretLifetimeState::Retired {
                decision: record.bytes.to_vec(),
            })
        }
        decision::NEGATIVE => Ok(SecretLifetimeState::Negative {
            decision: record.bytes.to_vec(),
        }),
        _ => Err(invalid("platform fourth decision kind is invalid")),
    }
}

#[allow(dead_code)]
pub(super) async fn promote_after_fourth(
    owned: &ProfileOwnedSecureStorage,
    three: PlatformThreeRosterAdmission,
) -> Result<PlatformFourRosterAdmission, (AuraError, PlatformThreeRosterAdmission)> {
    let result = promote_borrowed(owned, &three).await;
    result.map_err(|error| (error, three))
}

pub(in crate::secure::platform_lifetime_root::birth) async fn promote_borrowed(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
) -> Result<PlatformFourRosterAdmission, AuraError> {
    let (selected, owner) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    if !Arc::ptr_eq(&three._owner, owner) || !owner.lifetime_root_claimed.load(Ordering::Acquire) {
        return Err(invalid(
            "platform fourth handoff lacks original three-entry owner",
        ));
    }
    let mut root = read_root(owned, selected)?;
    if root.generic_phase != GENERIC_UNCLAIMED {
        return Err(invalid("platform fourth handoff was already transferred"));
    }
    if root.id != three.root {
        return Err(invalid("platform fourth handoff has foreign original root"));
    }
    let (claim, scope) = prior(owned, selected, &mut root)?;
    let mut ids = [root.birth_id, claim.allocation, root.third_id];
    ids.sort();
    if three.allocations != ids {
        return Err(invalid("platform fourth handoff has foreign prior roster"));
    }
    replay(owned, selected, &mut root, &claim, &scope, StopAfter::Never)?;
    lifecycle::recover(owned, selected, &mut root)?;
    if root.tail_phase < TAIL_APPEND_COMPLETE || root.tail_count != 4 {
        return Err(invalid("platform fourth handoff lacks complete ACK"));
    }
    Ok(PlatformFourRosterAdmission {
        root: root.id,
        fourth: root.tail_next_id,
        _owner: owner.clone(),
        generic_transferred: std::sync::atomic::AtomicBool::new(false),
    })
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn recover_four_roster(
    owned: &ProfileOwnedSecureStorage,
) -> Result<PlatformFourRosterAdmission, AuraError> {
    let (selected, owner) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = read_root(owned, selected)?;
    if root.generic_phase != GENERIC_UNCLAIMED {
        return Err(invalid("platform fourth roster was already transferred"));
    }
    let (claim, scope) = prior(owned, selected, &mut root)?;
    replay(owned, selected, &mut root, &claim, &scope, StopAfter::Never)?;
    lifecycle::recover(owned, selected, &mut root)?;
    owner
        .lifetime_root_claimed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("platform fourth roster root already transferred"))?;
    Ok(PlatformFourRosterAdmission {
        root: root.id,
        fourth: root.tail_next_id,
        _owner: owner.clone(),
        generic_transferred: std::sync::atomic::AtomicBool::new(false),
    })
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn read_fourth_secret(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = verify_four(owned, selected, four)?;
    if root.tail_phase >= TAIL_TOMBSTONE_PENDING {
        return Err(invalid(
            "platform fourth live read denied by original tombstone",
        ));
    }
    let bytes = read_entry(selected, &user(INTENT_PREFIX, &root.tail_next_id))?
        .ok_or_else(|| invalid("platform fourth original intent is missing"))?;
    if aura_core::hash::hash(&bytes) != root.tail_next_intent_digest
        || read_entry(selected, &user(RECORD_PREFIX, &root.tail_next_id))?.as_deref()
            != Some(bytes.as_slice())
    {
        return Err(invalid("platform fourth live credential differs"));
    }
    Ok(BirthIntent::decode(selected, &bytes)?.secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    pub(super) async fn first_three(
        fixture: &Fixture,
    ) -> Result<(ProductionSecureStorageHandler, PlatformThreeRosterAdmission), AuraError> {
        let (storage, two) = super::super::tests::first_two(fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_third(owned, &two, b"third scope", b"third secret").await?;
        let three = promote_after_third(owned, two)
            .await
            .map_err(|(error, _)| error)?;
        Ok((storage, three))
    }

    #[tokio::test]
    async fn fourth_birth_retains_exact_scope_owner_and_reopens_once() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_fourth(owned, &three, b"fourth retained scope", b"fourth secret").await?;
        assert!(
            super::super::lifecycle::decide(owned, &three, true, b"stale")
                .await
                .is_err()
        );
        let four = promote_after_fourth(owned, three)
            .await
            .map_err(|(error, _)| error)?;
        assert_eq!(&**read_fourth_secret(owned, &four).await?, b"fourth secret");
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        assert_eq!(root.tail_phase, TAIL_APPEND_COMPLETE);
        assert_eq!(root.tail_count, 4);
        assert_eq!(root.tail_roster_digest, root.tail_target_digest);
        assert!(recover_four_roster(owned).await.is_err(), "one root owner");
        drop(four);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let recovered = recover_four_roster(owned).await?;
        assert_eq!(
            &**read_fourth_secret(owned, &recovered).await?,
            b"fourth secret"
        );
        assert!(recover_four_roster(owned).await.is_err(), "one transfer");
        Ok(())
    }

    #[tokio::test]
    async fn fourth_birth_replays_only_root_anchored_intent_at_crash_edges() -> TestResult {
        for stop in [StopAfter::Intent, StopAfter::Record, StopAfter::Marker] {
            let fixture = Fixture::new()?;
            let (storage, three) = first_three(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            assert!(append_inner(
                owned,
                &three,
                b"fourth scope",
                b"fourth secret",
                Some([91; 32]),
                stop,
            )
            .await
            .is_err());
            drop(three);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let four = recover_four_roster(owned).await?;
            assert_eq!(&**read_fourth_secret(owned, &four).await?, b"fourth secret");
        }
        Ok(())
    }

    #[tokio::test]
    async fn fourth_birth_root_only_refuses_reopen_and_second_append() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        assert!(append_inner(
            owned,
            &three,
            b"fourth scope",
            b"fourth secret",
            Some([92; 32]),
            StopAfter::RootAnchor,
        )
        .await
        .is_err());
        assert!(append_fourth(owned, &three, b"new scope", b"secret")
            .await
            .is_err());
        drop(three);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_four_roster(owned).await.is_err(),
            "no sealed intent"
        );
        let (selected, _) = selected_owner(owned)?;
        assert!(read_entry(selected, &user(RECORD_PREFIX, &[92; 32]))?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn fourth_birth_refuses_foreign_owner_and_duplicate_before_mutation() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        assert!(append_inner(
            owned,
            &three,
            b"fourth scope",
            b"fourth secret",
            Some(root.third_id),
            StopAfter::Never,
        )
        .await
        .is_err());
        assert_eq!(read_root(owned, selected)?.tail_phase, TAIL_IDLE);
        let foreign = tempfile::tempdir()?;
        let foreign_storage = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_storage else {
            unreachable!()
        };
        assert!(append_fourth(foreign_owned, &three, b"fourth", b"secret")
            .await
            .is_err());
        assert_eq!(read_root(owned, selected)?.tail_phase, TAIL_IDLE);
        Ok(())
    }

    #[tokio::test]
    async fn fourth_birth_refuses_changed_intent_record_and_ack() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_fourth(owned, &three, b"fourth scope", b"fourth secret").await?;
        let four = promote_after_fourth(owned, three)
            .await
            .map_err(|(error, _)| error)?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let intent_user = user(INTENT_PREFIX, &root.tail_next_id);
        let record_user = user(RECORD_PREFIX, &root.tail_next_id);
        let exact = read_entry(selected, &intent_user)?.expect("sealed intent");
        write_exact(selected, &intent_user, b"changed intent")?;
        assert!(read_fourth_secret(owned, &four).await.is_err());
        write_exact(selected, &intent_user, &exact)?;
        write_exact(selected, &record_user, b"changed record")?;
        assert!(read_fourth_secret(owned, &four).await.is_err());
        write_exact(selected, &record_user, &exact)?;
        std::fs::remove_file(fixture.profile.path().join(FOURTH_ROSTER_FILE))?;
        assert!(read_fourth_secret(owned, &four).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn fourth_roster_codec_refuses_overbound_duplicate_reordered_and_foreign_entries(
    ) -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, _three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let mut root = read_root(owned, selected)?;
        let claim = prior_claim(owned, selected, &mut root)?;
        let third_scope = recover(owned, selected, &mut root, &claim)?;
        root.tail_next_id = [91; 32];
        root.tail_next_intent_digest = [92; 32];
        let valid = candidate(&root, &claim, &third_scope, b"fourth scope")?;
        let original = entries(&valid, ROSTER_MAGIC_V10, &root)?;
        assert_eq!(original.len(), 4);
        let mut overbound = valid;
        overbound[ROSTER_MAGIC_V10.len()..ROSTER_MAGIC_V10.len() + 2]
            .copy_from_slice(&4097u16.to_le_bytes());
        assert!(entries(&overbound, ROSTER_MAGIC_V10, &root).is_err());
        let encode = |parts: &[Vec<u8>]| {
            let mut bytes = ROSTER_MAGIC_V10.to_vec();
            bytes.extend_from_slice(&(parts.len() as u16).to_le_bytes());
            for part in parts {
                bytes.extend_from_slice(part);
            }
            bytes
        };
        let mut reordered = original.clone();
        reordered.swap(0, 1);
        assert!(entries(&encode(&reordered), ROSTER_MAGIC_V10, &root).is_err());
        let mut duplicate = original.clone();
        duplicate[1][32..64].copy_from_slice(&original[0][32..64]);
        assert!(entries(&encode(&duplicate), ROSTER_MAGIC_V10, &root).is_err());
        let mut foreign = original.clone();
        foreign[0][0] ^= 1;
        assert!(entries(&encode(&foreign), ROSTER_MAGIC_V10, &root).is_err());
        let mut empty_scope = original;
        empty_scope[0][161..163].copy_from_slice(&0u16.to_le_bytes());
        assert!(entries(&encode(&empty_scope), ROSTER_MAGIC_V10, &root).is_err());
        Ok(())
    }
}
