//! Private second-birth append. No second allocation owner is issued here.
//! The V4 root anchors the desired two-entry roster before any per-ID secret.

use super::*;

const CLAIM_USER: &str = "allocation-lifetime-second-birth-claim:v1";
const CLAIM_MAGIC: &[u8] = b"AURA-PLATFORM-SECOND-BIRTH-CLAIM-V1";
const SECOND_ROSTER_FILE: &str = "platform_lifetime/second-roster.v4";
const INTENT_PREFIX: &str = "allocation-lifetime-birth-intent:v2:";
const RECORD_PREFIX: &str = "allocation-lifetime-birth-record:v2:";
const CLAIM_LEN: usize = CLAIM_MAGIC.len() + 32 * 5;

struct Claim {
    root: [u8; 32],
    allocation: [u8; 32],
    intent_digest: [u8; 32],
    profile: [u8; 32],
    service: [u8; 32],
}

impl Claim {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CLAIM_LEN);
        bytes.extend_from_slice(CLAIM_MAGIC);
        bytes.extend_from_slice(&self.root);
        bytes.extend_from_slice(&self.allocation);
        bytes.extend_from_slice(&self.intent_digest);
        bytes.extend_from_slice(&self.profile);
        bytes.extend_from_slice(&self.service);
        bytes
    }

    fn decode(bytes: &[u8], root: &RootRecord) -> Result<Self, AuraError> {
        if bytes.len() != CLAIM_LEN || !bytes.starts_with(CLAIM_MAGIC) {
            return Err(invalid("platform second-birth claim encoding is invalid"));
        }
        let offset = CLAIM_MAGIC.len();
        let claim = Self {
            root: bytes[offset..offset + 32].try_into().expect("bounded root"),
            allocation: bytes[offset + 32..offset + 64]
                .try_into()
                .expect("bounded ID"),
            intent_digest: bytes[offset + 64..offset + 96]
                .try_into()
                .expect("bounded digest"),
            profile: bytes[offset + 96..offset + 128]
                .try_into()
                .expect("bounded profile"),
            service: bytes[offset + 128..offset + 160]
                .try_into()
                .expect("bounded service"),
        };
        if claim.root != root.id
            || claim.allocation == root.birth_id
            || claim.allocation == [0; 32]
            || claim.intent_digest == [0; 32]
            || claim.profile != root.profile
            || claim.service != root.service
        {
            return Err(invalid(
                "platform second birth belongs to another root or allocation",
            ));
        }
        Ok(claim)
    }
}

pub(super) fn claim_present(selected: &PlatformSecureStorageHandler) -> Result<bool, AuraError> {
    Ok(read_entry(selected, CLAIM_USER)?.is_some())
}

#[cfg(test)]
pub(super) fn cleanup_fixture(service: &str) {
    if let Ok(claim_entry) = keyring::Entry::new(service, CLAIM_USER) {
        if let Ok(bytes) = claim_entry.get_secret() {
            if bytes.len() == CLAIM_LEN && bytes.starts_with(CLAIM_MAGIC) {
                let offset = CLAIM_MAGIC.len() + 32;
                if let Ok(id) = <[u8; 32]>::try_from(&bytes[offset..offset + 32]) {
                    for name in [user(INTENT_PREFIX, &id), user(RECORD_PREFIX, &id)] {
                        if let Ok(entry) = keyring::Entry::new(service, &name) {
                            let _ = entry.delete_credential();
                        }
                    }
                }
            }
        }
        let _ = claim_entry.delete_credential();
    }
}

fn user(prefix: &str, id: &[u8; 32]) -> String {
    format!("{prefix}{}", hex::encode(id))
}

fn roster(root: &RootRecord, claim: &Claim) -> Vec<u8> {
    let first = inventory_marker(root);
    let first_entry = &first[INVENTORY_MAGIC.len() + 2..];
    let mut second = Vec::with_capacity(ROSTER_ENTRY_LEN);
    second.extend_from_slice(&root.id);
    second.extend_from_slice(&claim.allocation);
    second.extend_from_slice(&claim.intent_digest);
    second.extend_from_slice(&root.profile);
    second.extend_from_slice(&root.service);
    let mut bytes = Vec::with_capacity(INVENTORY_MAGIC.len() + 2 + 2 * ROSTER_ENTRY_LEN);
    bytes.extend_from_slice(INVENTORY_MAGIC);
    bytes.extend_from_slice(&2u16.to_le_bytes());
    if root.birth_id < claim.allocation {
        bytes.extend_from_slice(first_entry);
        bytes.extend_from_slice(&second);
    } else {
        bytes.extend_from_slice(&second);
        bytes.extend_from_slice(first_entry);
    }
    bytes
}

fn verify_first(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<(), AuraError> {
    let observed = owned
        ._owner
        .directory
        .read_private_publication(Path::new(INVENTORY_FILE), MAX_INVENTORY_FILE)
        .map_err(io)?;
    if !matches!(observed, Some((ref bytes, None)) if *bytes == inventory_marker(root)) {
        return Err(invalid("platform second birth lacks original first roster"));
    }
    decision::validate_recovery(owned, selected, root)?;
    if !matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE) {
        let (bytes, _) = read_intent(selected, root)?;
        if read_entry(selected, RECORD_USER)?.as_deref() != Some(bytes.as_slice()) {
            return Err(invalid(
                "platform second birth found different first credential",
            ));
        }
    }
    Ok(())
}

fn marker_state(owned: &ProfileOwnedSecureStorage, expected: &[u8]) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(SECOND_ROSTER_FILE), MAX_INVENTORY_FILE)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == expected => Ok(true),
        _ => Err(invalid(
            "platform second roster differs from root-anchored append",
        )),
    }
}

fn publish_marker(owned: &ProfileOwnedSecureStorage, bytes: &[u8]) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(SECOND_ROSTER_FILE), bytes)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform second roster publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopAfter {
    Never,
    RootAnchor,
    Claim,
    Intent,
    Record,
}

fn replay(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    claim: &Claim,
    stop: StopAfter,
) -> Result<(), AuraError> {
    let expected = roster(root, claim);
    if aura_core::hash::hash(&expected) != root.roster_digest {
        return Err(invalid(
            "platform second roster differs from original root anchor",
        ));
    }
    let intent_user = user(INTENT_PREFIX, &claim.allocation);
    let record_user = user(RECORD_PREFIX, &claim.allocation);
    let bytes = read_entry(selected, &intent_user)?
        .ok_or_else(|| invalid("platform second birth original intent is missing"))?;
    if aura_core::hash::hash(&bytes) != claim.intent_digest {
        return Err(invalid("platform second birth original intent differs"));
    }
    let intent = BirthIntent::decode(selected, &bytes)?;
    if intent.root != root.id || intent.allocation != claim.allocation {
        return Err(invalid(
            "platform second birth original intent has foreign identity",
        ));
    }
    if stop == StopAfter::Intent {
        return Err(interrupted());
    }
    let marker = marker_state(owned, &expected)?;
    match read_entry(selected, &record_user)? {
        Some(observed) if observed == bytes && marker => {}
        Some(_) if !marker => {
            return Err(invalid("platform second record lacks final roster marker"));
        }
        Some(_) => {
            return Err(invalid(
                "platform second record differs from original intent",
            ))
        }
        None if marker => return Err(invalid("platform second record is missing after ACK")),
        None => {
            write_exact(selected, &record_user, &bytes)?;
            if stop == StopAfter::Record {
                return Err(interrupted());
            }
            publish_marker(owned, &expected)?;
        }
    }
    Ok(())
}

/// Append exactly one second birth. This returns no allocation capability.
#[allow(dead_code)]
pub(super) async fn append_second(
    owned: &ProfileOwnedSecureStorage,
    first: &PlatformBirthAdmission,
    scope: &[u8],
    secret: &[u8],
) -> Result<(), AuraError> {
    append_inner(owned, first, scope, secret, None, StopAfter::Never).await
}

async fn append_inner(
    owned: &ProfileOwnedSecureStorage,
    first: &PlatformBirthAdmission,
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
            "platform second birth scope or secret exceeds bound",
        ));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    if !Arc::ptr_eq(&first._owner, &owned._owner) {
        return Err(invalid(
            "platform second birth requires original profile owner",
        ));
    }
    let mut root = read_root(owned, selected)?;
    if root.id != first.root || root.birth_id != first.allocation {
        return Err(invalid(
            "platform second birth requires original first allocation",
        ));
    }
    if !matches!(
        root.phase,
        BIRTH_COMPLETE | DECISION_COMPLETE | TOMBSTONE_ACK_ERASURE_PENDING | ERASURE_COMPLETE
    ) {
        return Err(invalid(
            "platform second birth requires settled first allocation",
        ));
    }
    if root.roster_digest != aura_core::hash::hash(&inventory_marker(&root))
        || claim_present(selected)?
        || marker_state(owned, &[])?
    {
        return Err(invalid(
            "platform second birth already claimed or conflicted",
        ));
    }
    verify_first(owned, selected, &root)?;
    let mut id = fixed_id.unwrap_or([0; 32]);
    if fixed_id.is_none() {
        rand::rngs::OsRng
            .try_fill_bytes(&mut id)
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "platform second allocation entropy",
                    Arc::new(source),
                )
            })?;
    }
    if id == root.birth_id || id == [0; 32] {
        return Err(invalid(
            "platform second birth duplicates original identity",
        ));
    }
    let intent = BirthIntent {
        root: root.id,
        allocation: id,
        scope: scope.to_vec(),
        secret: Zeroizing::new(secret.to_vec()),
    };
    let bytes = intent.encode(selected)?;
    let claim = Claim {
        root: root.id,
        allocation: id,
        intent_digest: aura_core::hash::hash(&bytes),
        profile: root.profile,
        service: root.service,
    };
    let intent_user = user(INTENT_PREFIX, &id);
    let record_user = user(RECORD_PREFIX, &id);
    if read_entry(selected, &intent_user)?.is_some()
        || read_entry(selected, &record_user)?.is_some()
    {
        return Err(invalid(
            "platform second birth found existing per-ID credential",
        ));
    }
    root.roster_digest = aura_core::hash::hash(&roster(&root, &claim));
    write_keyring(selected, &root)?;
    if stop == StopAfter::RootAnchor {
        return Err(interrupted());
    }
    write_exact(selected, CLAIM_USER, &claim.encode())?;
    if stop == StopAfter::Claim {
        return Err(interrupted());
    }
    write_exact(selected, &intent_user, &bytes)?;
    replay(owned, selected, &root, &claim, stop)
}

/// Reconcile only the root-anchored second append; do not return an owner.
#[allow(dead_code)]
pub(super) async fn reconcile_second(owned: &ProfileOwnedSecureStorage) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = read_root(owned, selected)?;
    verify_first(owned, selected, &root)?;
    let bytes = read_entry(selected, CLAIM_USER)?
        .ok_or_else(|| invalid("platform second birth root anchor lacks claim"))?;
    let claim = Claim::decode(&bytes, &root)?;
    replay(owned, selected, &root, &claim, StopAfter::Never)
}

/// Custody of the complete, authenticated two-entry roster. This deliberately
/// exposes no per-entry decision or retirement method while V4 has no second
/// transition anchor. The original profile lease is retained, never reopened
/// as an independent storage owner.
#[allow(dead_code)]
pub(super) struct PlatformRosterAdmission {
    root: [u8; 32],
    allocations: [[u8; 32]; 2],
    _owner: Arc<crate::profile_storage::OwnedProfileLease>,
}

/// Recover both exact birth records under the selected namespace lock before
/// transferring the original profile's lifetime root once. A partial append
/// may be reconciled only from its root-anchored claim and sealed intent.
#[allow(dead_code)]
pub(super) async fn recover_roster(
    owned: &ProfileOwnedSecureStorage,
) -> Result<PlatformRosterAdmission, AuraError> {
    let (selected, owner) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = read_root(owned, selected)?;
    let bytes = read_entry(selected, CLAIM_USER)?
        .ok_or_else(|| invalid("platform whole roster lacks second claim"))?;
    let claim = Claim::decode(&bytes, &root)?;
    verify_first(owned, selected, &root)?;
    if root.phase == ERASURE_COMPLETE {
        let mut verified = root.clone();
        decision::recover_erasure(owned, selected, &mut verified)?;
    }
    replay(owned, selected, &root, &claim, StopAfter::Never)?;
    if !marker_state(owned, &roster(&root, &claim))? {
        return Err(invalid("platform whole roster is not acknowledged"));
    }
    owner
        .lifetime_root_claimed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("platform whole roster root already transferred"))?;
    let mut allocations = [root.birth_id, claim.allocation];
    allocations.sort();
    Ok(PlatformRosterAdmission {
        root: root.id,
        allocations,
        _owner: owner.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[tokio::test]
    async fn second_birth_uses_root_anchored_sorted_roster_and_per_id_records() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
        append_second(owned, &birth, b"second", b"second secret").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let claim = Claim::decode(&read_entry(selected, CLAIM_USER)?.expect("claim"), &root)?;
        let expected = roster(&root, &claim);
        assert_eq!(root.roster_digest, aura_core::hash::hash(&expected));
        assert!(marker_state(owned, &expected)?);
        let second_intent =
            read_entry(selected, &user(INTENT_PREFIX, &claim.allocation))?.expect("per-ID intent");
        assert_eq!(
            read_entry(selected, &user(RECORD_PREFIX, &claim.allocation))?,
            Some(second_intent)
        );
        assert!(
            read_live_secret(owned, &birth).await.is_err(),
            "no partial-roster owner"
        );
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        reconcile_second(owned).await?;
        assert!(
            recover_one(owned).await.is_err(),
            "whole-roster handoff remains closed"
        );
        Ok(())
    }

    #[tokio::test]
    async fn second_birth_interruptions_replay_or_fail_closed() -> TestResult {
        for stop in [
            StopAfter::RootAnchor,
            StopAfter::Claim,
            StopAfter::Intent,
            StopAfter::Record,
        ] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
            assert!(
                append_inner(owned, &birth, b"second", b"second secret", None, stop)
                    .await
                    .is_err()
            );
            drop(birth);
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let result = reconcile_second(owned).await;
            assert_eq!(
                result.is_ok(),
                stop == StopAfter::Intent,
                "boundary {stop:?}"
            );
            assert!(recover_one(owned).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn second_birth_rejects_duplicate_foreign_and_preexisting_address() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
        let (selected, _) = selected_owner(owned)?;
        let original = read_root(owned, selected)?.roster_digest;
        assert!(append_inner(
            owned,
            &birth,
            b"second",
            b"secret",
            Some(birth.allocation),
            StopAfter::Never
        )
        .await
        .is_err());
        let id = [99; 32];
        write_exact(selected, &user(RECORD_PREFIX, &id), b"occupied")?;
        assert!(append_inner(
            owned,
            &birth,
            b"second",
            b"secret",
            Some(id),
            StopAfter::Never
        )
        .await
        .is_err());
        assert_eq!(read_root(owned, selected)?.roster_digest, original);
        assert_eq!(
            read_entry(selected, &user(RECORD_PREFIX, &id))?.as_deref(),
            Some(b"occupied".as_slice())
        );
        let foreign = tempfile::tempdir()?;
        let foreign_selected = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(append_second(foreign_owned, &birth, b"second", b"secret")
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn second_birth_claim_intent_marker_and_record_loss_refuse_reopen() -> TestResult {
        for missing in [CLAIM_USER, "intent", "marker", "record"] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
            append_second(owned, &birth, b"second", b"second secret").await?;
            let (selected, _) = selected_owner(owned)?;
            let root = read_root(owned, selected)?;
            let claim = Claim::decode(&read_entry(selected, CLAIM_USER)?.expect("claim"), &root)?;
            match missing {
                "marker" => std::fs::remove_file(fixture.profile.path().join(SECOND_ROSTER_FILE))?,
                "intent" => selected
                    .entry_for_user(&user(INTENT_PREFIX, &claim.allocation))?
                    .delete_credential()?,
                "record" => selected
                    .entry_for_user(&user(RECORD_PREFIX, &claim.allocation))?
                    .delete_credential()?,
                _ => selected.entry_for_user(CLAIM_USER)?.delete_credential()?,
            }
            drop(birth);
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            assert!(reconcile_second(owned).await.is_err(), "loss of {missing}");
            assert!(
                recover_one(owned).await.is_err(),
                "no first owner on partial roster"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn whole_roster_recovers_both_exact_entries_and_transfers_once() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
        append_second(owned, &birth, b"second", b"second secret").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let claim = Claim::decode(&read_entry(selected, CLAIM_USER)?.expect("claim"), &root)?;
        let expected = [root.birth_id, claim.allocation];
        drop(birth);
        drop(empty);
        drop(first);

        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let roster = recover_roster(owned).await?;
        let mut sorted = expected;
        sorted.sort();
        assert_eq!(roster.root, root.id);
        assert_eq!(roster.allocations, sorted);
        assert!(Arc::ptr_eq(&roster._owner, &owned._owner));
        assert!(recover_roster(owned).await.is_err(), "one owner transfer");
        assert!(recover_one(owned).await.is_err(), "no first-only downgrade");
        Ok(())
    }

    #[tokio::test]
    async fn whole_roster_replays_only_root_anchored_pending_birth() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
        assert!(append_inner(
            owned,
            &birth,
            b"second",
            b"second secret",
            None,
            StopAfter::Intent,
        )
        .await
        .is_err());
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let roster = recover_roster(owned).await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let claim = Claim::decode(&read_entry(selected, CLAIM_USER)?.expect("claim"), &root)?;
        assert_eq!(roster.allocations.len(), 2);
        assert!(marker_state(owned, &self::roster(&root, &claim))?);
        assert_eq!(
            read_entry(selected, &user(RECORD_PREFIX, &claim.allocation))?,
            read_entry(selected, &user(INTENT_PREFIX, &claim.allocation))?
        );
        Ok(())
    }

    #[tokio::test]
    async fn whole_roster_refuses_lost_or_changed_entry_before_owner() -> TestResult {
        for loss in ["claim", "intent", "record", "marker", "first"] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
            append_second(owned, &birth, b"second", b"second secret").await?;
            let (selected, _) = selected_owner(owned)?;
            let root = read_root(owned, selected)?;
            let claim = Claim::decode(&read_entry(selected, CLAIM_USER)?.expect("claim"), &root)?;
            match loss {
                "claim" => selected.entry_for_user(CLAIM_USER)?.delete_credential()?,
                "intent" => selected
                    .entry_for_user(&user(INTENT_PREFIX, &claim.allocation))?
                    .delete_credential()?,
                "record" => selected
                    .entry_for_user(&user(RECORD_PREFIX, &claim.allocation))?
                    .delete_credential()?,
                "marker" => std::fs::remove_file(fixture.profile.path().join(SECOND_ROSTER_FILE))?,
                _ => selected.entry_for_user(RECORD_USER)?.delete_credential()?,
            }
            drop(birth);
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            assert!(recover_roster(owned).await.is_err(), "lost {loss}");
            assert!(
                !owned._owner.lifetime_root_claimed.load(Ordering::Acquire),
                "lost {loss} never hands out owner"
            );
        }
        Ok(())
    }
}
