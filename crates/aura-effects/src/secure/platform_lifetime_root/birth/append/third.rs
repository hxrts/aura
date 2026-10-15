//! Root-anchored third birth. No third allocation or decision owner is issued.

use super::*;

const THIRD_ROSTER_FILE: &str = "platform_lifetime/third-roster.v8";

fn third_roster(root: &RootRecord, claim: &Claim, scope: &[u8]) -> Result<Vec<u8>, AuraError> {
    let prior = roster(root, claim);
    validate_roster_layout(&prior, root, claim)?;
    let mut entries = Vec::with_capacity(3);
    let mut cursor = ROSTER_MAGIC.len() + 2;
    for _ in 0..2 {
        let header_end = cursor + ROSTER_ENTRY_HEADER_LEN;
        let scope_len = u16::from_le_bytes([prior[cursor + 161], prior[cursor + 162]]) as usize;
        let end = header_end + scope_len;
        entries.push(prior[cursor..end].to_vec());
        cursor = end;
    }
    if cursor != prior.len() || scope.is_empty() || scope.len() > MAX_SCOPE_BYTES {
        return Err(invalid("platform third birth roster scope is invalid"));
    }
    let mut third = Vec::with_capacity(ROSTER_ENTRY_HEADER_LEN + scope.len());
    third.extend_from_slice(&root.id);
    third.extend_from_slice(&root.third_id);
    third.extend_from_slice(&root.third_intent_digest);
    third.extend_from_slice(&root.profile);
    third.extend_from_slice(&root.service);
    third.push(BIRTH_ACKNOWLEDGED);
    third.extend_from_slice(&(scope.len() as u16).to_le_bytes());
    third.extend_from_slice(scope);
    entries.push(third);
    entries.sort_by(|left, right| left[32..64].cmp(&right[32..64]));
    if entries
        .windows(2)
        .any(|pair| pair[0][32..64] == pair[1][32..64])
    {
        return Err(invalid("platform third birth duplicates roster identity"));
    }
    let mut bytes = Vec::with_capacity(prior.len() + ROSTER_ENTRY_HEADER_LEN + scope.len());
    bytes.extend_from_slice(ROSTER_MAGIC);
    bytes.extend_from_slice(&3u16.to_le_bytes());
    for entry in entries {
        bytes.extend_from_slice(&entry);
    }
    if bytes.len() > MAX_ROSTER_FILE || aura_core::hash::hash(&bytes) != root.third_roster_digest {
        return Err(invalid(
            "platform third roster differs from original root anchor",
        ));
    }
    Ok(bytes)
}

fn marker_state(owned: &ProfileOwnedSecureStorage, expected: &[u8]) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(THIRD_ROSTER_FILE), MAX_ROSTER_FILE)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == expected => Ok(true),
        _ => Err(invalid(
            "platform third roster marker differs from root anchor",
        )),
    }
}

fn publish_marker(owned: &ProfileOwnedSecureStorage, bytes: &[u8]) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(THIRD_ROSTER_FILE), bytes)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform third roster publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

fn prior_claim(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<Claim, AuraError> {
    let bytes = read_entry(selected, CLAIM_USER)?
        .ok_or_else(|| invalid("platform third birth lacks original second claim"))?;
    let claim = Claim::decode(&bytes, root)?;
    let prior = roster(root, &claim);
    validate_roster_layout(&prior, root, &claim)?;
    verify_first(owned, selected, root)?;
    lifecycle::recover(owned, selected, &mut root.clone(), &claim)?;
    if !super::marker_state(owned, &prior)? {
        return Err(invalid("platform third birth lacks second roster ACK"));
    }
    Ok(claim)
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
    stop: StopAfter,
) -> Result<(), AuraError> {
    let intent_user = user(INTENT_PREFIX, &root.third_id);
    let record_user = user(RECORD_PREFIX, &root.third_id);
    let bytes = read_entry(selected, &intent_user)?
        .ok_or_else(|| invalid("platform third birth original sealed intent is missing"))?;
    if aura_core::hash::hash(&bytes) != root.third_intent_digest {
        return Err(invalid(
            "platform third birth original sealed intent differs",
        ));
    }
    let intent = BirthIntent::decode(selected, &bytes)?;
    if intent.root != root.id || intent.allocation != root.third_id {
        return Err(invalid("platform third birth intent has foreign identity"));
    }
    let expected = third_roster(root, claim, &intent.scope)?;
    if stop == StopAfter::Intent {
        return Err(interrupted());
    }
    let marker = marker_state(owned, &expected)?;
    match read_entry(selected, &record_user)? {
        Some(observed) if observed == bytes => {}
        Some(_) => return Err(invalid("platform third record differs from sealed intent")),
        None if marker => return Err(invalid("platform third record is missing after ACK")),
        None if root.third_phase == THIRD_BIRTH_COMPLETE => {
            return Err(invalid("completed platform third record is missing"));
        }
        None => write_exact(selected, &record_user, &bytes)?,
    }
    if stop == StopAfter::Record {
        return Err(interrupted());
    }
    if !marker {
        if root.third_phase == THIRD_BIRTH_COMPLETE {
            return Err(invalid("completed platform third ACK is missing"));
        }
        publish_marker(owned, &expected)?;
    }
    if stop == StopAfter::Marker {
        return Err(interrupted());
    }
    if root.third_phase == THIRD_BIRTH_PENDING {
        root.third_phase = THIRD_BIRTH_COMPLETE;
        write_keyring(selected, root)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) async fn append_third(
    owned: &ProfileOwnedSecureStorage,
    roster_owner: &PlatformRosterAdmission,
    scope: &[u8],
    secret: &[u8],
) -> Result<(), AuraError> {
    append_inner(owned, roster_owner, scope, secret, None, StopAfter::Never).await
}

async fn append_inner(
    owned: &ProfileOwnedSecureStorage,
    roster_owner: &PlatformRosterAdmission,
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
            "platform third birth scope or secret exceeds bound",
        ));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = verify_roster_owner(owned, selected, roster_owner)?;
    let claim = prior_claim(owned, selected, &root)?;
    if marker_state(owned, &[])? {
        return Err(invalid("platform third birth already acknowledged"));
    }
    let mut id = fixed_id.unwrap_or([0; 32]);
    if fixed_id.is_none() {
        rand::rngs::OsRng
            .try_fill_bytes(&mut id)
            .map_err(|source| {
                AuraError::crypto_with_source("platform third allocation entropy", Arc::new(source))
            })?;
    }
    if id == [0; 32] || id == root.birth_id || id == claim.allocation {
        return Err(invalid("platform third birth duplicates original identity"));
    }
    let intent = BirthIntent {
        root: root.id,
        allocation: id,
        scope: scope.to_vec(),
        secret: Zeroizing::new(secret.to_vec()),
    };
    let bytes = intent.encode(selected)?;
    let intent_user = user(INTENT_PREFIX, &id);
    let record_user = user(RECORD_PREFIX, &id);
    if read_entry(selected, &intent_user)?.is_some()
        || read_entry(selected, &record_user)?.is_some()
    {
        return Err(invalid("platform third birth found existing credential"));
    }
    root.third_phase = THIRD_BIRTH_PENDING;
    root.third_id = id;
    root.third_intent_digest = aura_core::hash::hash(&bytes);
    // The candidate roster is authenticated in the original keyring root
    // before either third per-ID credential is published.
    let mut candidate = root.clone();
    candidate.third_roster_digest = [0; 32];
    root.third_roster_digest = candidate_digest(&candidate, &claim, scope)?;
    write_keyring(selected, &root)?;
    if stop == StopAfter::RootAnchor {
        return Err(interrupted());
    }
    write_exact(selected, &intent_user, &bytes)?;
    replay(owned, selected, &mut root, &claim, stop)
}

fn candidate_digest(root: &RootRecord, claim: &Claim, scope: &[u8]) -> Result<[u8; 32], AuraError> {
    // Build against a temporary digest, then bind the exact candidate bytes.
    let mut bound = root.clone();
    let prior = roster(&bound, claim);
    validate_roster_layout(&prior, &bound, claim)?;
    let mut entries = Vec::with_capacity(3);
    let mut cursor = ROSTER_MAGIC.len() + 2;
    for _ in 0..2 {
        let end = cursor
            + ROSTER_ENTRY_HEADER_LEN
            + u16::from_le_bytes([prior[cursor + 161], prior[cursor + 162]]) as usize;
        entries.push(prior[cursor..end].to_vec());
        cursor = end;
    }
    let mut third = Vec::new();
    third.extend_from_slice(&root.id);
    third.extend_from_slice(&root.third_id);
    third.extend_from_slice(&root.third_intent_digest);
    third.extend_from_slice(&root.profile);
    third.extend_from_slice(&root.service);
    third.push(BIRTH_ACKNOWLEDGED);
    third.extend_from_slice(&(scope.len() as u16).to_le_bytes());
    third.extend_from_slice(scope);
    entries.push(third);
    entries.sort_by(|left, right| left[32..64].cmp(&right[32..64]));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(ROSTER_MAGIC);
    bytes.extend_from_slice(&3u16.to_le_bytes());
    for entry in entries {
        bytes.extend_from_slice(&entry);
    }
    bound.third_roster_digest = aura_core::hash::hash(&bytes);
    third_roster(&bound, claim, scope)?;
    Ok(bound.third_roster_digest)
}

#[allow(dead_code)]
pub(super) async fn reconcile_third(owned: &ProfileOwnedSecureStorage) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = read_root(owned, selected)?;
    if root.third_phase == THIRD_UNCLAIMED {
        return Err(invalid("platform third birth has no root-anchored claim"));
    }
    let claim = prior_claim(owned, selected, &root)?;
    replay(owned, selected, &mut root, &claim, StopAfter::Never)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn first_two(
        fixture: &Fixture,
    ) -> Result<(ProductionSecureStorageHandler, PlatformRosterAdmission), AuraError> {
        let storage = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let first = allocate_one(owned, &empty, b"first scope", b"first secret").await?;
        let roster = append_second_owned(owned, first, b"second scope", b"second secret")
            .await
            .map_err(|(error, _)| error)?;
        Ok((storage, roster))
    }

    #[tokio::test]
    async fn third_birth_anchors_exact_scope_and_ack_before_completion() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, roster) = first_two(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_third(owned, &roster, b"third scope", b"third secret").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        assert_eq!(root.third_phase, THIRD_BIRTH_COMPLETE);
        let claim = prior_claim(owned, selected, &root)?;
        let expected = third_roster(&root, &claim, b"third scope")?;
        assert!(marker_state(owned, &expected)?);
        assert_eq!(
            read_entry(selected, &user(RECORD_PREFIX, &root.third_id))?,
            read_entry(selected, &user(INTENT_PREFIX, &root.third_id))?
        );
        assert!(append_third(owned, &roster, b"fourth", b"secret")
            .await
            .is_err());
        assert!(verify_roster_owner(owned, selected, &roster).is_err());
        drop(roster);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        reconcile_third(owned).await?;
        assert!(
            recover_roster(owned).await.is_err(),
            "no two-entry downgrade"
        );
        Ok(())
    }

    #[tokio::test]
    async fn third_birth_interruption_replays_only_original_sealed_intent() -> TestResult {
        for stop in [StopAfter::Intent, StopAfter::Record, StopAfter::Marker] {
            let fixture = Fixture::new()?;
            let (storage, roster) = first_two(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            assert!(append_inner(
                owned,
                &roster,
                b"third scope",
                b"third secret",
                Some([93; 32]),
                stop,
            )
            .await
            .is_err());
            drop(roster);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            reconcile_third(owned).await?;
            let (selected, _) = selected_owner(owned)?;
            assert_eq!(
                read_root(owned, selected)?.third_phase,
                THIRD_BIRTH_COMPLETE
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn third_birth_refuses_unrecoverable_anchor_and_missing_ack() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, roster) = first_two(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        assert!(append_inner(
            owned,
            &roster,
            b"third scope",
            b"third secret",
            Some([94; 32]),
            StopAfter::RootAnchor,
        )
        .await
        .is_err());
        assert!(reconcile_third(owned).await.is_err(), "no sealed intent");
        assert!(append_third(owned, &roster, b"other", b"secret")
            .await
            .is_err());
        assert!(read_entry(selected_owner(owned)?.0, &user(RECORD_PREFIX, &[94; 32]))?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn third_birth_refuses_foreign_owner_and_missing_ack_after_completion() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, roster) = first_two(&fixture).await?;
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
        assert!(append_third(foreign_owned, &roster, b"third", b"secret")
            .await
            .is_err());
        append_third(owned, &roster, b"third scope", b"third secret").await?;
        std::fs::remove_file(fixture.profile.path().join(THIRD_ROSTER_FILE))?;
        assert!(
            reconcile_third(owned).await.is_err(),
            "completed ACK loss must refuse"
        );
        Ok(())
    }

    #[tokio::test]
    async fn third_birth_refuses_changed_intent_or_record_after_reopen() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, roster) = first_two(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_third(owned, &roster, b"third scope", b"third secret").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let intent_user = user(INTENT_PREFIX, &root.third_id);
        let record_user = user(RECORD_PREFIX, &root.third_id);
        let original = read_entry(selected, &intent_user)?.expect("sealed intent");
        write_exact(selected, &intent_user, b"changed intent")?;
        assert!(reconcile_third(owned).await.is_err());
        write_exact(selected, &intent_user, &original)?;
        write_exact(selected, &record_user, b"changed record")?;
        assert!(reconcile_third(owned).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn third_birth_duplicate_identity_refuses_before_root_mutation() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, roster) = first_two(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        assert!(append_inner(
            owned,
            &roster,
            b"third scope",
            b"third secret",
            Some(root.birth_id),
            StopAfter::Never,
        )
        .await
        .is_err());
        assert_eq!(read_root(owned, selected)?.third_phase, THIRD_UNCLAIMED);
        Ok(())
    }
}
